//! System configuration and market category definitions.
//!
//! This module contains all configuration constants and category mappings
//! for the Polymarket sports arbitrage trading system.

/// Polymarket WebSocket URL
pub const POLYMARKET_WS_URL: &str = "wss://ws-subscriptions-clob.polymarket.com/ws/market";

/// Gamma API base URL (Polymarket market data)
pub const GAMMA_API_BASE: &str = "https://gamma-api.polymarket.com";

/// Arb threshold: alert when total cost < this (e.g., 0.992 = 0.8% profit)
pub const ARB_THRESHOLD: f64 = 0.992;

/// Polymarket ping interval (seconds) - keep connection alive
pub const POLY_PING_INTERVAL_SECS: u64 = 30;

/// WebSocket reconnect delay (seconds)
pub const WS_RECONNECT_DELAY_SECS: u64 = 5;

/// Minimum liquidity to consider a market (USD)
pub const MIN_LIQUIDITY_USD: f64 = 500.0;

/// Minimum 24h volume to consider a market active (USD)
/// Set to $1000 - bot will rank by volume and take top 500 for WebSocket
pub const MIN_VOLUME_24H_USD: f64 = 1000.0;

/// Market refresh interval in seconds (2 hours = 7200s)
/// Bot will re-discover markets every 2 hours to find new opportunities
#[allow(dead_code)]
pub const MARKET_REFRESH_INTERVAL_SECS: u64 = 2 * 60 * 60;

/// Market categories to monitor
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MarketCategory {
    Crypto,
    Sports,
    Politics,
    Business,
}

impl MarketCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            MarketCategory::Crypto => "crypto",
            MarketCategory::Sports => "sports",
            MarketCategory::Politics => "politics",
            MarketCategory::Business => "business",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "crypto" | "cryptocurrency" => Some(MarketCategory::Crypto),
            "sports" => Some(MarketCategory::Sports),
            "politics" | "political" => Some(MarketCategory::Politics),
            "business" | "finance" | "economics" => Some(MarketCategory::Business),
            _ => None,
        }
    }
}

impl std::fmt::Display for MarketCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Get enabled market categories from environment variable
/// Format: ENABLED_CATEGORIES="crypto,sports,politics"
/// If not set, defaults to sports only
pub fn get_enabled_categories() -> Vec<MarketCategory> {
    if let Ok(categories_str) = std::env::var("ENABLED_CATEGORIES") {
        if categories_str.is_empty() {
            // Default to sports only
            return vec![MarketCategory::Sports];
        }

        categories_str
            .split(',')
            .filter_map(|s| MarketCategory::from_str(s.trim()))
            .collect()
    } else {
        // Default: sports only
        vec![MarketCategory::Sports]
    }
}

/// Price logging enabled (set PRICE_LOGGING=1 to enable)
#[allow(dead_code)]
pub fn price_logging_enabled() -> bool {
    static CACHED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *CACHED.get_or_init(|| {
        std::env::var("PRICE_LOGGING")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false)
    })
}
