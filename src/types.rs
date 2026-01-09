//! Core type definitions and data structures for the arbitrage trading system.
//!
//! This module provides the foundational types for market state management,
//! orderbook representation, and arbitrage opportunity detection.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use rustc_hash::FxHashMap;

// === Market Types ===

/// A Polymarket trading market
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketPair {
    /// Unique identifier for this market
    pub pair_id: Arc<str>,
    /// Market category (e.g., "sports", "crypto", "politics")
    pub category: Arc<str>,
    /// Human-readable market description
    pub description: Arc<str>,
    /// Polymarket market slug
    pub poly_slug: Arc<str>,
    /// Polymarket YES outcome token address
    pub poly_yes_token: Arc<str>,
    /// Polymarket NO outcome token address
    pub poly_no_token: Arc<str>,
    /// Liquidity in USD (for sorting/filtering)
    pub liquidity: f64,
}

/// Price representation in cents (1-99 for $0.01-$0.99), 0 indicates no price available
pub type PriceCents = u16;

/// Size representation in cents (dollar amount × 100), maximum ~$655k per side
pub type SizeCents = u16;

/// Maximum number of concurrently tracked markets
pub const MAX_MARKETS: usize = 1024;

/// Sentinel value indicating no price is currently available
pub const NO_PRICE: PriceCents = 0;

/// Pack orderbook state into a single u64 for atomic operations.
/// Bit layout: [yes_ask:16][no_ask:16][yes_size:16][no_size:16]
#[inline(always)]
pub fn pack_orderbook(yes_ask: PriceCents, no_ask: PriceCents, yes_size: SizeCents, no_size: SizeCents) -> u64 {
    ((yes_ask as u64) << 48) | ((no_ask as u64) << 32) | ((yes_size as u64) << 16) | (no_size as u64)
}

/// Unpack a u64 orderbook representation back into its component values
#[inline(always)]
pub fn unpack_orderbook(packed: u64) -> (PriceCents, PriceCents, SizeCents, SizeCents) {
    let yes_ask = ((packed >> 48) & 0xFFFF) as PriceCents;
    let no_ask = ((packed >> 32) & 0xFFFF) as PriceCents;
    let yes_size = ((packed >> 16) & 0xFFFF) as SizeCents;
    let no_size = (packed & 0xFFFF) as SizeCents;
    (yes_ask, no_ask, yes_size, no_size)
}

/// Lock-free orderbook state for a single trading platform.
/// Uses atomic operations for thread-safe, zero-copy price updates.
#[repr(align(64))]
pub struct AtomicOrderbook {
    /// Packed orderbook state: [yes_ask:16][no_ask:16][yes_size:16][no_size:16]
    packed: AtomicU64,
}

impl AtomicOrderbook {
    pub const fn new() -> Self {
        Self { packed: AtomicU64::new(0) }
    }

    /// Load current state
    #[inline(always)]
    pub fn load(&self) -> (PriceCents, PriceCents, SizeCents, SizeCents) {
        unpack_orderbook(self.packed.load(Ordering::Acquire))
    }

    /// Store new state
    #[inline(always)]
    #[allow(dead_code)]
    pub fn store(&self, yes_ask: PriceCents, no_ask: PriceCents, yes_size: SizeCents, no_size: SizeCents) {
        self.packed.store(pack_orderbook(yes_ask, no_ask, yes_size, no_size), Ordering::Release);
    }

    /// Update YES side only
    #[inline(always)]
    pub fn update_yes(&self, yes_ask: PriceCents, yes_size: SizeCents) {
        let mut current = self.packed.load(Ordering::Acquire);
        loop {
            let (_, no_ask, _, no_size) = unpack_orderbook(current);
            let new = pack_orderbook(yes_ask, no_ask, yes_size, no_size);
            match self.packed.compare_exchange_weak(current, new, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => break,
                Err(c) => current = c,
            }
        }
    }

    /// Update NO side only
    #[inline(always)]
    pub fn update_no(&self, no_ask: PriceCents, no_size: SizeCents) {
        let mut current = self.packed.load(Ordering::Acquire);
        loop {
            let (yes_ask, _, yes_size, _) = unpack_orderbook(current);
            let new = pack_orderbook(yes_ask, no_ask, yes_size, no_size);
            match self.packed.compare_exchange_weak(current, new, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => break,
                Err(c) => current = c,
            }
        }
    }
}

impl Default for AtomicOrderbook {
    fn default() -> Self {
        Self::new()
    }
}

/// Complete market state for a single Polymarket market
pub struct AtomicMarketState {
    /// Polymarket platform orderbook state
    pub poly: AtomicOrderbook,
    /// Market metadata (immutable after discovery phase)
    pub pair: Option<Arc<MarketPair>>,
    /// Unique market identifier for O(1) lookups
    pub market_id: u16,
}

impl AtomicMarketState {
    pub fn new(market_id: u16) -> Self {
        Self {
            poly: AtomicOrderbook::new(),
            pair: None,
            market_id,
        }
    }

    #[inline(always)]
    pub fn check_arbs(&self, threshold_cents: PriceCents) -> u8 {
        let (p_yes, p_no, _, _) = self.poly.load();

        if p_yes == NO_PRICE || p_no == NO_PRICE {
            return 0;
        }

        // Poly-only arbitrage: YES + NO < 100 cents (no fees!)
        let cost = p_yes + p_no;

        if cost < threshold_cents {
            1  // PolyOnly arb detected
        } else {
            0
        }
    }
}

// Polymarket has zero trading fees - no fee calculation needed

/// Convert f64 price (0.01-0.99) to PriceCents (1-99)
#[inline(always)]
pub fn price_to_cents(price: f64) -> PriceCents {
    ((price * 100.0).round() as PriceCents).clamp(0, 99)
}

/// Convert PriceCents back to f64
#[inline(always)]
pub fn cents_to_price(cents: PriceCents) -> f64 {
    cents as f64 / 100.0
}

/// Parse price from string "0.XX" format (Polymarket)
/// Returns 0 if parsing fails
#[inline(always)]
pub fn parse_price(s: &str) -> PriceCents {
    let bytes = s.as_bytes();
    // Handle "0.XX" format (4 chars)
    if bytes.len() == 4 && bytes[0] == b'0' && bytes[1] == b'.' {
        let d1 = bytes[2].wrapping_sub(b'0');
        let d2 = bytes[3].wrapping_sub(b'0');
        if d1 < 10 && d2 < 10 {
            return (d1 as u16 * 10 + d2 as u16) as PriceCents;
        }
    }
    // Handle "0.X" format (3 chars) for prices like 0.5
    if bytes.len() == 3 && bytes[0] == b'0' && bytes[1] == b'.' {
        let d = bytes[2].wrapping_sub(b'0');
        if d < 10 {
            return (d as u16 * 10) as PriceCents;
        }
    }
    // Fallback to standard parse
    s.parse::<f64>()
        .map(|p| price_to_cents(p))
        .unwrap_or(0)
}

/// Arbitrage opportunity type - Polymarket-only trading
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArbType {
    /// Same-platform: Buy Polymarket YES + Buy Polymarket NO
    PolyOnly,
}

/// High-priority execution request for an arbitrage opportunity
#[derive(Debug, Clone, Copy)]
pub struct FastExecutionRequest {
    /// Market identifier (index into GlobalState.markets array)
    pub market_id: u16,
    /// YES outcome ask price in cents
    pub yes_price: PriceCents,
    /// NO outcome ask price in cents
    pub no_price: PriceCents,
    /// YES outcome available size in cents
    pub yes_size: SizeCents,
    /// NO outcome available size in cents
    pub no_size: SizeCents,
    /// Arbitrage type (determines execution strategy)
    pub arb_type: ArbType,
    /// Detection timestamp in nanoseconds since system start
    pub detected_ns: u64,
}

impl FastExecutionRequest {
    #[inline(always)]
    pub fn profit_cents(&self) -> i16 {
        100 - (self.yes_price as i16 + self.no_price as i16 + self.estimated_fee_cents() as i16)
    }

    #[inline(always)]
    pub fn estimated_fee_cents(&self) -> PriceCents {
        // Polymarket has zero trading fees
        0
    }
}

/// Global market state manager for all tracked Polymarket markets
pub struct GlobalState {
    /// Market states indexed by market_id for O(1) access
    pub markets: Vec<AtomicMarketState>,

    /// Next available market identifier (monotonically increasing)
    next_market_id: u16,

    /// O(1) lookup map: pre-hashed Polymarket YES token → market_id
    pub poly_yes_to_id: FxHashMap<u64, u16>,

    /// O(1) lookup map: pre-hashed Polymarket NO token → market_id
    pub poly_no_to_id: FxHashMap<u64, u16>,
}

impl GlobalState {
    pub fn new() -> Self {
        // Allocate market slots
        let markets: Vec<AtomicMarketState> = (0..MAX_MARKETS)
            .map(|i| AtomicMarketState::new(i as u16))
            .collect();

        Self {
            markets,
            next_market_id: 0,
            poly_yes_to_id: FxHashMap::default(),
            poly_no_to_id: FxHashMap::default(),
        }
    }

    /// Add a market pair, returns market_id
    pub fn add_pair(&mut self, pair: MarketPair) -> Option<u16> {
        if self.next_market_id as usize >= MAX_MARKETS {
            return None;
        }

        let market_id = self.next_market_id;
        self.next_market_id += 1;

        // Pre-compute hashes
        let poly_yes_hash = fxhash_str(&pair.poly_yes_token);
        let poly_no_hash = fxhash_str(&pair.poly_no_token);

        // Update lookup maps
        self.poly_yes_to_id.insert(poly_yes_hash, market_id);
        self.poly_no_to_id.insert(poly_no_hash, market_id);

        // Store pair
        self.markets[market_id as usize].pair = Some(Arc::new(pair));

        Some(market_id)
    }

    /// Get market by Poly YES token hash (O(1))
    #[inline(always)]
    #[allow(dead_code)]
    pub fn get_by_poly_yes_hash(&self, hash: u64) -> Option<&AtomicMarketState> {
        let id = *self.poly_yes_to_id.get(&hash)?;
        Some(&self.markets[id as usize])
    }

    /// Get market by Poly NO token hash (O(1))
    #[inline(always)]
    #[allow(dead_code)]
    pub fn get_by_poly_no_hash(&self, hash: u64) -> Option<&AtomicMarketState> {
        let id = *self.poly_no_to_id.get(&hash)?;
        Some(&self.markets[id as usize])
    }

    /// Get market_id by Poly YES token hash
    #[inline(always)]
    #[allow(dead_code)]
    pub fn id_by_poly_yes_hash(&self, hash: u64) -> Option<u16> {
        self.poly_yes_to_id.get(&hash).copied()
    }

    /// Get market_id by Poly NO token hash
    #[inline(always)]
    #[allow(dead_code)]
    pub fn id_by_poly_no_hash(&self, hash: u64) -> Option<u16> {
        self.poly_no_to_id.get(&hash).copied()
    }

    /// Get market by ID
    #[inline(always)]
    pub fn get_by_id(&self, id: u16) -> Option<&AtomicMarketState> {
        if (id as usize) < self.markets.len() {
            Some(&self.markets[id as usize])
        } else {
            None
        }
    }

    pub fn market_count(&self) -> usize {
        self.next_market_id as usize
    }
}

impl Default for GlobalState {
    fn default() -> Self {
        Self::new()
    }
}

/// Fast string hashing function using FxHash for O(1) lookups
#[inline(always)]
pub fn fxhash_str(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = rustc_hash::FxHasher::default();
    s.hash(&mut hasher);
    hasher.finish()
}

// === Platform Enum ===

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Platform {
    Kalshi,
    Polymarket,
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Platform::Kalshi => write!(f, "KALSHI"),
            Platform::Polymarket => write!(f, "POLYMARKET"),
        }
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    // =========================================================================
    // Pack/Unpack Tests - Verify bit manipulation correctness
    // =========================================================================

    #[test]
    fn test_pack_unpack_roundtrip() {
        // Test various values pack and unpack correctly
        let test_cases = vec![
            (50, 50, 1000, 1000),  // Common mid-price
            (1, 99, 100, 100),      // Edge prices
            (99, 1, 65535, 65535),  // Max sizes
            (0, 0, 0, 0),           // All zeros
            (NO_PRICE, NO_PRICE, 0, 0),  // No prices
        ];

        for (yes_ask, no_ask, yes_size, no_size) in test_cases {
            let packed = pack_orderbook(yes_ask, no_ask, yes_size, no_size);
            let (y, n, ys, ns) = unpack_orderbook(packed);
            assert_eq!((y, n, ys, ns), (yes_ask, no_ask, yes_size, no_size),
                "Roundtrip failed for ({}, {}, {}, {})", yes_ask, no_ask, yes_size, no_size);
        }
    }

    #[test]
    fn test_pack_bit_layout() {
        // Verify the exact bit layout: [yes_ask:16][no_ask:16][yes_size:16][no_size:16]
        let packed = pack_orderbook(0xABCD, 0x1234, 0x5678, 0x9ABC);

        assert_eq!((packed >> 48) & 0xFFFF, 0xABCD, "yes_ask should be in bits 48-63");
        assert_eq!((packed >> 32) & 0xFFFF, 0x1234, "no_ask should be in bits 32-47");
        assert_eq!((packed >> 16) & 0xFFFF, 0x5678, "yes_size should be in bits 16-31");
        assert_eq!(packed & 0xFFFF, 0x9ABC, "no_size should be in bits 0-15");
    }

    // =========================================================================
    // AtomicOrderbook Tests
    // =========================================================================

    #[test]
    fn test_atomic_orderbook_store_load() {
        let book = AtomicOrderbook::new();

        // Initially all zeros
        let (y, n, ys, ns) = book.load();
        assert_eq!((y, n, ys, ns), (0, 0, 0, 0));

        // Store and load
        book.store(45, 55, 500, 600);
        let (y, n, ys, ns) = book.load();
        assert_eq!((y, n, ys, ns), (45, 55, 500, 600));
    }

    #[test]
    fn test_atomic_orderbook_update_yes() {
        let book = AtomicOrderbook::new();

        // Set initial state
        book.store(40, 60, 100, 200);

        // Update only YES side
        book.update_yes(42, 150);

        let (y, n, ys, ns) = book.load();
        assert_eq!(y, 42, "YES ask should be updated");
        assert_eq!(ys, 150, "YES size should be updated");
        assert_eq!(n, 60, "NO ask should be unchanged");
        assert_eq!(ns, 200, "NO size should be unchanged");
    }

    #[test]
    fn test_atomic_orderbook_update_no() {
        let book = AtomicOrderbook::new();

        // Set initial state
        book.store(40, 60, 100, 200);

        // Update only NO side
        book.update_no(58, 250);

        let (y, n, ys, ns) = book.load();
        assert_eq!(y, 40, "YES ask should be unchanged");
        assert_eq!(ys, 100, "YES size should be unchanged");
        assert_eq!(n, 58, "NO ask should be updated");
        assert_eq!(ns, 250, "NO size should be updated");
    }

    #[test]
    fn test_atomic_orderbook_concurrent_updates() {
        // Verify correctness under concurrent access
        let book = Arc::new(AtomicOrderbook::new());
        book.store(50, 50, 1000, 1000);

        let handles: Vec<_> = (0..4).map(|i| {
            let book = book.clone();
            thread::spawn(move || {
                for _ in 0..1000 {
                    if i % 2 == 0 {
                        book.update_yes(45 + (i as u16), 500);
                    } else {
                        book.update_no(55 + (i as u16), 500);
                    }
                }
            })
        }).collect();

        for h in handles {
            h.join().unwrap();
        }

        // State should be consistent (not corrupted)
        let (y, n, ys, ns) = book.load();
        assert!(y > 0 && y < 100, "YES ask should be valid");
        assert!(n > 0 && n < 100, "NO ask should be valid");
        assert_eq!(ys, 500, "YES size should be consistent");
        assert_eq!(ns, 500, "NO size should be consistent");
    }

    // Polymarket has zero trading fees - no fee tests needed

    // =========================================================================
    // Price Conversion Tests
    // =========================================================================

    #[test]
    fn test_price_to_cents() {
        assert_eq!(price_to_cents(0.50), 50);
        assert_eq!(price_to_cents(0.01), 1);
        assert_eq!(price_to_cents(0.99), 99);
        assert_eq!(price_to_cents(0.0), 0);
        assert_eq!(price_to_cents(1.0), 99);  // Clamped to 99
        assert_eq!(price_to_cents(0.505), 51);  // Rounded
        assert_eq!(price_to_cents(0.504), 50);  // Rounded
    }

    #[test]
    fn test_cents_to_price() {
        assert!((cents_to_price(50) - 0.50).abs() < 0.001);
        assert!((cents_to_price(1) - 0.01).abs() < 0.001);
        assert!((cents_to_price(99) - 0.99).abs() < 0.001);
        assert!((cents_to_price(0) - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_parse_price() {
        // Standard "0.XX" format
        assert_eq!(parse_price("0.50"), 50);
        assert_eq!(parse_price("0.01"), 1);
        assert_eq!(parse_price("0.99"), 99);

        // "0.X" format
        assert_eq!(parse_price("0.5"), 50);

        // Fallback parsing
        assert_eq!(parse_price("0.505"), 51);

        // Invalid input
        assert_eq!(parse_price("invalid"), 0);
        assert_eq!(parse_price(""), 0);
    }

    // =========================================================================
    // check_arbs Tests
    // =========================================================================

    fn make_market_state(
        poly_yes: PriceCents,
        poly_no: PriceCents,
    ) -> AtomicMarketState {
        let state = AtomicMarketState::new(0);
        state.poly.store(poly_yes, poly_no, 1000, 1000);
        state
    }

    #[test]
    fn test_check_arbs_poly_only() {
        // Poly YES 48¢ + Poly NO 50¢ = 98¢ → ARB (no fees!)
        let state = make_market_state(48, 50);

        let mask = state.check_arbs(100);

        assert!(mask & 1 != 0, "Should detect Poly-only arb");
    }

    #[test]
    fn test_check_arbs_no_arbs() {
        // Prices efficient - no arb
        // Poly: 52 + 52 = 104 > 100
        let state = make_market_state(52, 52);

        let mask = state.check_arbs(100);

        assert_eq!(mask, 0, "Should detect no arbs in efficient market");
    }

    #[test]
    fn test_check_arbs_missing_prices() {
        // Missing price should return no arbs
        let state = AtomicMarketState::new(0);
        state.poly.store(50, NO_PRICE, 1000, 1000);

        let mask = state.check_arbs(100);

        assert_eq!(mask, 0, "Should return 0 when any price is missing");
    }

    // =========================================================================
    // GlobalState Tests
    // =========================================================================

    fn make_test_pair(id: &str) -> MarketPair {
        MarketPair {
            pair_id: id.into(),
            category: "crypto".into(),
            description: format!("Test Market {}", id).into(),
            poly_slug: format!("test-{}", id).into(),
            poly_yes_token: format!("yes_token_{}", id).into(),
            poly_no_token: format!("no_token_{}", id).into(),
            liquidity: 10000.0,
        }
    }

    #[test]
    fn test_global_state_add_pair() {
        let mut state = GlobalState::new();

        let pair = make_test_pair("001");
        let poly_yes = pair.poly_yes_token.clone();
        let poly_no = pair.poly_no_token.clone();

        let id = state.add_pair(pair).expect("Should add pair");

        assert_eq!(id, 0, "First market should have id 0");
        assert_eq!(state.market_count(), 1);

        // Verify lookups work
        let poly_yes_hash = fxhash_str(&poly_yes);
        let poly_no_hash = fxhash_str(&poly_no);

        assert!(state.poly_yes_to_id.contains_key(&poly_yes_hash));
        assert!(state.poly_no_to_id.contains_key(&poly_no_hash));
    }

    #[test]
    fn test_global_state_lookups() {
        let mut state = GlobalState::new();

        let pair = make_test_pair("002");
        let poly_yes = pair.poly_yes_token.clone();

        let id = state.add_pair(pair).unwrap();

        // Test get_by_id
        let market = state.get_by_id(id).expect("Should find by id");
        assert!(market.pair.is_some());

        // Test get_by_poly_yes_hash
        let market = state.get_by_poly_yes_hash(fxhash_str(&poly_yes))
            .expect("Should find by Poly YES hash");
        assert!(market.pair.is_some());

        // Test id lookups
        assert_eq!(state.id_by_poly_yes_hash(fxhash_str(&poly_yes)), Some(id));
    }

    #[test]
    fn test_global_state_multiple_markets() {
        let mut state = GlobalState::new();

        // Add multiple markets
        for i in 0..10 {
            let pair = make_test_pair(&format!("{:03}", i));
            let id = state.add_pair(pair).unwrap();
            assert_eq!(id, i as u16);
        }

        assert_eq!(state.market_count(), 10);

        // All should be findable
        for i in 0..10 {
            let market = state.get_by_id(i as u16);
            assert!(market.is_some(), "Market {} should exist", i);
        }
    }

    #[test]
    fn test_global_state_update_prices() {
        let mut state = GlobalState::new();

        let pair = make_test_pair("003");
        let id = state.add_pair(pair).unwrap();

        // Update Poly prices
        let market = state.get_by_id(id).unwrap();
        market.poly.store(44, 56, 700, 800);

        // Verify prices
        let (p_yes, p_no, p_yes_sz, p_no_sz) = market.poly.load();
        assert_eq!((p_yes, p_no, p_yes_sz, p_no_sz), (44, 56, 700, 800));
    }

    // =========================================================================
    // FastExecutionRequest Tests
    // =========================================================================

    #[test]
    fn test_execution_request_profit_cents_poly_only() {
        // Poly YES 40¢ + Poly NO 48¢ = 88¢
        // No fees on Polymarket
        // Profit = 100 - 88 - 0 = 12¢
        let req = FastExecutionRequest {
            market_id: 0,
            yes_price: 40,
            no_price: 48,
            yes_size: 1000,
            no_size: 1000,
            arb_type: ArbType::PolyOnly,
            detected_ns: 0,
        };

        assert_eq!(req.profit_cents(), 12);
        assert_eq!(req.estimated_fee_cents(), 0);
    }

    #[test]
    fn test_execution_request_negative_profit() {
        // Prices too high - no profit
        let req = FastExecutionRequest {
            market_id: 0,
            yes_price: 52,
            no_price: 52,
            yes_size: 1000,
            no_size: 1000,
            arb_type: ArbType::PolyOnly,
            detected_ns: 0,
        };

        assert!(req.profit_cents() < 0, "Should have negative profit");
    }

    // =========================================================================
    // fxhash_str Tests
    // =========================================================================

    #[test]
    fn test_fxhash_str_consistency() {
        let s = "KXEPLGAME-25DEC27CFCARS-CFC";

        // Same string should always produce same hash
        let h1 = fxhash_str(s);
        let h2 = fxhash_str(s);
        assert_eq!(h1, h2);

        // Different strings should (almost certainly) produce different hashes
        let h3 = fxhash_str("KXEPLGAME-25DEC27CFCARS-ARS");
        assert_ne!(h1, h3);
    }

    // =========================================================================
    // Integration: Full Arb Detection Flow
    // =========================================================================

    #[test]
    fn test_full_arb_flow() {
        // Simulate the full flow: add market, update prices, detect arb
        let mut state = GlobalState::new();

        // 1. Add market during discovery
        let pair = MarketPair {
            pair_id: "test-arb".into(),
            category: "crypto".into(),
            description: "Bitcoin > $100k by March".into(),
            poly_slug: "bitcoin-100k-march".into(),
            poly_yes_token: "yes_token_btc".into(),
            poly_no_token: "no_token_btc".into(),
            liquidity: 50000.0,
        };

        let poly_yes_token = pair.poly_yes_token.clone();

        let market_id = state.add_pair(pair).unwrap();

        // 2. Simulate WebSocket updates setting prices
        // Polymarket update
        let poly_hash = fxhash_str(&poly_yes_token);
        if let Some(id) = state.poly_yes_to_id.get(&poly_hash) {
            state.markets[*id as usize].poly.store(48, 50, 700, 800);
        }

        // 3. Check for arbs (threshold = 100 cents = $1.00)
        let market = state.get_by_id(market_id).unwrap();
        let arb_mask = market.check_arbs(100);

        // 4. Verify arb detected
        assert!(arb_mask & 1 != 0, "Should detect Poly-only arb");

        // 5. Build execution request
        let (p_yes, p_no, p_yes_sz, p_no_sz) = market.poly.load();

        let req = FastExecutionRequest {
            market_id,
            yes_price: p_yes,
            no_price: p_no,
            yes_size: p_yes_sz,
            no_size: p_no_sz,
            arb_type: ArbType::PolyOnly,
            detected_ns: 0,
        };

        assert!(req.profit_cents() > 0, "Should have positive profit");
    }

    #[test]
    fn test_price_update_race_condition() {
        // Simulate concurrent price updates from Polymarket WebSocket
        let state = Arc::new(GlobalState::default());

        // Pre-populate with a market
        let market = &state.markets[0];
        market.poly.store(50, 50, 1000, 1000);

        let handles: Vec<_> = (0..4).map(|i| {
            let state = state.clone();
            thread::spawn(move || {
                for j in 0..1000 {
                    let market = &state.markets[0];
                    if i % 2 == 0 {
                        // Simulate YES updates
                        market.poly.update_yes(40 + ((j % 10) as u16), 500 + j as u16);
                    } else {
                        // Simulate NO updates
                        market.poly.update_no(50 + ((j % 10) as u16), 600 + j as u16);
                    }

                    // Check arbs (should never panic) - threshold = 100 cents
                    let _ = market.check_arbs(100);
                }
            })
        }).collect();

        for h in handles {
            h.join().unwrap();
        }

        // Final state should be valid
        let market = &state.markets[0];
        let (p_yes, p_no, _, _) = market.poly.load();

        assert!(p_yes > 0 && p_yes < 100);
        assert!(p_no > 0 && p_no < 100);
    }
}

// === Polymarket/Gamma API Types ===

/// Helper to deserialize f64 from either number or string
fn deserialize_f64_flexible<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrNumber {
        String(String),
        Number(f64),
    }

    match Option::<StringOrNumber>::deserialize(deserializer)? {
        Some(StringOrNumber::String(s)) => s.parse::<f64>()
            .map(Some)
            .map_err(|e| D::Error::custom(format!("Failed to parse float: {}", e))),
        Some(StringOrNumber::Number(n)) => Ok(Some(n)),
        None => Ok(None),
    }
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct GammaMarket {
    pub slug: Option<String>,
    pub question: Option<String>,
    #[serde(rename = "clobTokenIds")]
    pub clob_token_ids: Option<String>,
    pub outcomes: Option<String>,
    #[serde(rename = "outcomePrices")]
    pub outcome_prices: Option<String>,
    pub active: Option<bool>,
    pub closed: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_f64_flexible")]
    pub liquidity: Option<f64>,      // Total liquidity (USD, can be string or number)
    #[serde(default, deserialize_with = "deserialize_f64_flexible")]
    pub volume: Option<f64>,         // Total volume (USD, can be string or number)
    #[serde(rename = "volume24hr", default, deserialize_with = "deserialize_f64_flexible")]
    pub volume_24hr: Option<f64>,    // 24h volume (USD, can be string or number)
}

// === Discovery Result ===

#[derive(Debug, Default)]
pub struct DiscoveryResult {
    pub pairs: Vec<MarketPair>,
    pub total_found: usize,
    pub errors: Vec<String>,
}