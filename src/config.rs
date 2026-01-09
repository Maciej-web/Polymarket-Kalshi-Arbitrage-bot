//! System configuration and market category definitions.
//!
//! This module contains all configuration constants and category mappings
//! for the Polymarket-only trading system.

/// Polymarket WebSocket URL
pub const POLYMARKET_WS_URL: &str = "wss://ws-subscriptions-clob.polymarket.com/ws/market";

/// Gamma API base URL (Polymarket market data)
pub const GAMMA_API_BASE: &str = "https://gamma-api.polymarket.com";

/// Arb threshold: alert when total cost < this (e.g., 0.995 = 0.5% profit)
pub const ARB_THRESHOLD: f64 = 0.995;

/// Polymarket ping interval (seconds) - keep connection alive
pub const POLY_PING_INTERVAL_SECS: u64 = 30;

/// WebSocket reconnect delay (seconds)
pub const WS_RECONNECT_DELAY_SECS: u64 = 5;

/// Market discovery refresh interval (seconds) - check for new markets every 5 minutes
pub const DISCOVERY_REFRESH_INTERVAL_SECS: u64 = 300;

/// Top N markets by liquidity to track
pub const TOP_N_MARKETS_BY_LIQUIDITY: usize = 100;

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

    pub fn all() -> &'static [MarketCategory] {
        &[
            MarketCategory::Crypto,
            MarketCategory::Sports,
            MarketCategory::Politics,
            MarketCategory::Business,
        ]
    }
}

impl std::fmt::Display for MarketCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Get enabled market categories from environment variable
/// Format: ENABLED_CATEGORIES="crypto,sports,politics"
/// If not set, returns all categories
pub fn get_enabled_categories() -> Vec<MarketCategory> {
    if let Ok(categories_str) = std::env::var("ENABLED_CATEGORIES") {
        if categories_str.is_empty() {
            return MarketCategory::all().to_vec();
        }

        categories_str
            .split(',')
            .filter_map(|s| MarketCategory::from_str(s.trim()))
            .collect()
    } else {
        MarketCategory::all().to_vec()
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
