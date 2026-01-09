//! Polymarket-Only Multi-Market Trading System
//!
//! A high-performance, production-ready trading system for Polymarket prediction markets.
//! This system monitors price discrepancies within Polymarket markets across multiple
//! categories (crypto, sports, politics, business), executing arbitrage opportunities
//! in real-time.
//!
//! ## Strategy
//!
//! The core arbitrage strategy exploits the fundamental property of prediction markets:
//! YES + NO = $1.00 (guaranteed). Arbitrage opportunities exist when:
//!
//! ```
//! Best YES ask + Best NO ask < $1.00 (on same market)
//! ```
//!
//! ## Architecture
//!
//! - **Real-time price monitoring** via WebSocket connections to both platforms
//! - **Lock-free orderbook cache** using atomic operations for zero-copy updates
//! - **SIMD-accelerated arbitrage detection** for sub-millisecond latency
//! - **Concurrent order execution** with automatic position reconciliation
//! - **Circuit breaker protection** with configurable risk limits
//! - **Market discovery system** with intelligent caching and incremental updates

mod circuit_breaker;
mod config;
mod discovery;
mod execution;
mod polymarket;
mod polymarket_clob;
mod position_tracker;
mod types;

use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

use circuit_breaker::{CircuitBreaker, CircuitBreakerConfig};
use config::{ARB_THRESHOLD, WS_RECONNECT_DELAY_SECS, MARKET_REFRESH_INTERVAL_SECS, get_enabled_categories};
use discovery::DiscoveryClient;
use execution::{ExecutionEngine, create_execution_channel, run_execution_loop};
use polymarket_clob::{PolymarketAsyncClient, PreparedCreds, SharedAsyncClient};
use position_tracker::{PositionTracker, create_position_channel, position_writer_loop};
use types::{GlobalState, PriceCents};

/// Polymarket CLOB API host
const POLY_CLOB_HOST: &str = "https://clob.polymarket.com";
/// Polygon chain ID
const POLYGON_CHAIN_ID: u64 = 137;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("prediction_market_arbitrage=info".parse().unwrap()),
        )
        .init();

    info!("🚀 Polymarket Multi-Market Trading System v3.0");
    info!("   Profit threshold: <{:.1}¢ ({:.1}% minimum profit)",
          ARB_THRESHOLD * 100.0, (1.0 - ARB_THRESHOLD) * 100.0);

    let enabled_categories = get_enabled_categories();
    info!("   Monitored categories: {:?}", enabled_categories.iter().map(|c| c.as_str()).collect::<Vec<_>>());

    // Check for dry run mode
    let dry_run = std::env::var("DRY_RUN").map(|v| v == "1" || v == "true").unwrap_or(true);
    if dry_run {
        info!("   Mode: DRY RUN (set DRY_RUN=0 to execute)");
    } else {
        warn!("   Mode: LIVE EXECUTION");
    }

    // Load Polymarket credentials
    dotenvy::dotenv().ok();
    let poly_private_key = std::env::var("POLY_PRIVATE_KEY")
        .context("POLY_PRIVATE_KEY not set")?;
    let poly_funder = std::env::var("POLY_FUNDER")
        .context("POLY_FUNDER not set (your wallet address)")?;

    // Create async Polymarket client and derive API credentials
    info!("[POLYMARKET] Creating async client and deriving API credentials...");
    let poly_async_client = PolymarketAsyncClient::new(
        POLY_CLOB_HOST,
        POLYGON_CHAIN_ID,
        &poly_private_key,
        &poly_funder,
    )?;
    let api_creds = poly_async_client.derive_api_key(0).await?;
    let prepared_creds = PreparedCreds::from_api_creds(&api_creds)?;
    let poly_async = Arc::new(SharedAsyncClient::new(poly_async_client, prepared_creds, POLYGON_CHAIN_ID));

    // Load neg_risk cache from Python script output
    match poly_async.load_cache(".clob_market_cache.json") {
        Ok(count) => info!("[POLYMARKET] Loaded {} neg_risk entries from cache", count),
        Err(e) => warn!("[POLYMARKET] Could not load neg_risk cache: {}", e),
    }

    info!("[POLYMARKET] Client ready for {}", &poly_funder[..10]);

    // Run discovery
    info!("🔍 Polymarket market discovery...");

    let discovery = DiscoveryClient::new();
    let result = discovery.discover(&enabled_categories).await;

    info!("📊 Market discovery complete:");
    info!("   - Total markets found: {}", result.total_found);

    if !result.errors.is_empty() {
        warn!("⚠️  Discovery errors:");
        for err in &result.errors {
            warn!("   - {}", err);
        }
    }

    if result.pairs.is_empty() {
        error!("❌ No market pairs found! Check API connection, category filters, and market availability.");
        error!("   Review the discovery logs above for detailed information.");
        return Ok(());
    }

    // Display discovered markets
    info!("📋 Discovered markets:");
    for (i, pair) in result.pairs.iter().take(10).enumerate() {
        info!("   {}. {} | {} | Liquidity: ${:.0}",
              i + 1,
              pair.category,
              pair.description,
              pair.liquidity);
    }
    if result.pairs.len() > 10 {
        info!("   ... and {} more markets", result.pairs.len() - 10);
    }

    // Build global state
    let initial_markets = result.pairs.clone();
    let state = Arc::new({
        let mut s = GlobalState::new();
        for pair in result.pairs {
            s.add_pair(pair);
        }
        info!("📡 Global state initialized: tracking {} markets", s.market_count());
        s
    });

    // Initialize execution infrastructure
    let (exec_tx, exec_rx) = create_execution_channel();
    let circuit_breaker = Arc::new(CircuitBreaker::new(CircuitBreakerConfig::from_env()));

    let position_tracker = Arc::new(RwLock::new(PositionTracker::new()));
    let (position_channel, position_rx) = create_position_channel();

    tokio::spawn(position_writer_loop(position_rx, position_tracker));

    let threshold_cents: PriceCents = ((ARB_THRESHOLD * 100.0).round() as u16).max(1);
    info!("   Execution threshold: {} cents", threshold_cents);

    let engine = Arc::new(ExecutionEngine::new(
        poly_async.clone(),
        state.clone(),
        circuit_breaker.clone(),
        position_channel,
        dry_run,
    ));

    let exec_handle = tokio::spawn(run_execution_loop(exec_rx, engine));

    // === MARKET REFRESH: Auto-restart on market changes ===
    // Background task that checks for new markets every MARKET_REFRESH_INTERVAL_SECS
    // If markets change, bot gracefully exits and should be auto-restarted by systemd/screen
    let refresh_discovery = DiscoveryClient::new();
    let refresh_categories = enabled_categories.clone();
    let refresh_initial_markets = initial_markets;
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(
            tokio::time::Duration::from_secs(MARKET_REFRESH_INTERVAL_SECS)
        );

        // Skip first tick (we just did discovery)
        interval.tick().await;

        loop {
            interval.tick().await;

            info!("🔄 Market refresh check: discovering new markets...");
            let new_result = refresh_discovery.discover(&refresh_categories).await;

            if new_result.pairs.is_empty() {
                warn!("⚠️  Market refresh returned no markets - skipping update");
                continue;
            }

            // Compare markets by sorting both lists by pair_id
            let mut old_ids: Vec<Arc<str>> = refresh_initial_markets.iter()
                .map(|p| p.pair_id.clone())
                .collect();
            old_ids.sort();

            let mut new_ids: Vec<Arc<str>> = new_result.pairs.iter()
                .map(|p| p.pair_id.clone())
                .collect();
            new_ids.sort();

            if old_ids != new_ids {
                let added = new_ids.iter()
                    .filter(|id| !old_ids.contains(id))
                    .count();
                let removed = old_ids.iter()
                    .filter(|id| !new_ids.contains(id))
                    .count();

                warn!("🔄 Market changes detected!");
                warn!("   Markets changed: {} added, {} removed", added, removed);
                warn!("   Old count: {}, New count: {}", old_ids.len(), new_ids.len());
                warn!("   Bot will exit and auto-restart with new markets...");

                // Give time to flush logs
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

                // Exit cleanly - systemd/screen will auto-restart
                std::process::exit(0);
            } else {
                info!("✅ Market refresh: no changes detected ({} markets)", old_ids.len());
            }
        }
    });

    // === DYNAMIC POSITION SIZING: Update limits based on balance ===
    // Background task that updates position sizing daily based on USDC balance
    // Set BALANCE_USD in .env or it defaults to $1000
    let balance_circuit_breaker = circuit_breaker.clone();
    tokio::spawn(async move {
        // Initial balance update
        let initial_balance = std::env::var("BALANCE_USD")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(1000.0);

        info!("💰 Initial balance: ${:.2} - updating position limits...", initial_balance);
        balance_circuit_breaker.update_limits_from_balance(initial_balance).await;

        // Update every 24 hours
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(24 * 60 * 60));
        interval.tick().await; // Skip first tick

        loop {
            interval.tick().await;

            let balance = std::env::var("BALANCE_USD")
                .ok()
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(1000.0);

            info!("💰 Daily balance check: ${:.2} - updating position limits...", balance);
            balance_circuit_breaker.update_limits_from_balance(balance).await;
        }
    });

    // === TEST MODE: Synthetic arbitrage injection ===
    // TEST_ARB=1 to enable
    let test_arb = std::env::var("TEST_ARB").map(|v| v == "1" || v == "true").unwrap_or(false);
    if test_arb {
        let test_state = state.clone();
        let test_exec_tx = exec_tx.clone();
        let test_dry_run = dry_run;

        tokio::spawn(async move {
            use types::{FastExecutionRequest, ArbType};

            // Wait for WebSocket connections to establish and populate orderbooks
            info!("[TEST] Injecting synthetic arbitrage opportunity in 10 seconds...");
            tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

            // Poly-only arbitrage
            let arb_type = ArbType::PolyOnly;
            let (yes_price, no_price, description) = (48, 50, "P_yes=48¢ + P_no=50¢ + fee=0¢ = 98¢ → 2¢ profit (NO FEES!)");

            // Find first market with valid state
            let market_count = test_state.market_count();
            for market_id in 0..market_count {
                if let Some(market) = test_state.get_by_id(market_id as u16) {
                    if let Some(pair) = &market.pair {
                        // SIZE: 1000 cents = 10 contracts (Poly $1 min requires ~3 contracts at 40¢)
                        let fake_req = FastExecutionRequest {
                            market_id: market_id as u16,
                            yes_price,
                            no_price,
                            yes_size: 1000,  // 1000¢ = 10 contracts
                            no_size: 1000,   // 1000¢ = 10 contracts
                            arb_type,
                            detected_ns: 0,
                        };

                        warn!("[TEST] 🧪 Injecting synthetic {:?} arbitrage for: {}", arb_type, pair.description);
                        warn!("[TEST]    Scenario: {}", description);
                        warn!("[TEST]    Position size capped to 10 contracts for safety");
                        warn!("[TEST]    Execution mode: DRY_RUN={}", test_dry_run);

                        if let Err(e) = test_exec_tx.send(fake_req).await {
                            error!("[TEST] Failed to send fake arb: {}", e);
                        }
                        break;
                    }
                }
            }
        });
    }

    // Initialize Polymarket WebSocket connection
    let poly_state = state.clone();
    let poly_exec_tx = exec_tx.clone();
    let poly_threshold = threshold_cents;
    let poly_handle = tokio::spawn(async move {
        loop {
            if let Err(e) = polymarket::run_ws(poly_state.clone(), poly_exec_tx.clone(), poly_threshold).await {
                error!("[POLYMARKET] WebSocket disconnected: {} - reconnecting...", e);
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(WS_RECONNECT_DELAY_SECS)).await;
        }
    });

    // System health monitoring and arbitrage diagnostics
    let heartbeat_state = state.clone();
    let heartbeat_threshold = threshold_cents;
    let heartbeat_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            let market_count = heartbeat_state.market_count();
            let mut with_prices = 0;
            // Track best arbitrage opportunity: (total_cost, market_id, p_yes, p_no)
            let mut best_arb: Option<(u16, u16, u16, u16)> = None;

            for market in heartbeat_state.markets.iter().take(market_count) {
                let (p_yes, p_no, _, _) = market.poly.load();
                let has_prices = p_yes > 0 && p_no > 0;
                if has_prices {
                    with_prices += 1;

                    let cost = p_yes + p_no; // No fees on Polymarket!

                    if best_arb.is_none() || cost < best_arb.as_ref().unwrap().0 {
                        best_arb = Some((cost, market.market_id, p_yes, p_no));
                    }
                }
            }

            info!("💓 System heartbeat | Markets: {} total, {} with prices | threshold={}¢",
                  market_count, with_prices, heartbeat_threshold);

            if let Some((cost, market_id, p_yes, p_no)) = best_arb {
                let gap = cost as i16 - heartbeat_threshold as i16;
                let desc = heartbeat_state.get_by_id(market_id)
                    .and_then(|m| m.pair.as_ref())
                    .map(|p| &*p.description)
                    .unwrap_or("Unknown");
                let leg_breakdown = format!("P_yes({}¢) + P_no({}¢) = {}¢", p_yes, p_no, cost);
                if gap <= 10 {
                    info!("   📊 Best opportunity: {} | {} | gap={:+}¢",
                          desc, leg_breakdown, gap);
                } else {
                    info!("   📊 Best opportunity: {} | {} | gap={:+}¢ (market efficient)",
                          desc, leg_breakdown, gap);
                }
            } else if with_prices == 0 {
                warn!("   ⚠️  No markets with prices - verify WebSocket connection");
            }
        }
    });

    // Main event loop - run until termination
    info!("✅ All systems operational - entering main event loop");
    let _ = tokio::join!(poly_handle, heartbeat_handle, exec_handle);

    Ok(())
}
