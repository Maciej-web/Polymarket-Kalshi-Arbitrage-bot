//! Polymarket-only market discovery system.
//!
//! This module discovers Polymarket markets across multiple categories,
//! filters by liquidity, and returns the top N most liquid markets.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{info, warn};

use crate::config::{GAMMA_API_BASE, MARKETS_PER_CATEGORY, MIN_LIQUIDITY_USD, MIN_VOLUME_24H_USD, MarketCategory};
use crate::types::{MarketPair, DiscoveryResult, GammaMarket};

/// Discovery cache file path
const DISCOVERY_CACHE_PATH: &str = ".discovery_cache.json";

/// Cache TTL in seconds (2 hours)
const CACHE_TTL_SECS: u64 = 2 * 60 * 60;

/// Persistent cache for discovered markets
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DiscoveryCache {
    /// Unix timestamp when cache was created
    timestamp_secs: u64,
    /// Cached markets
    markets: Vec<MarketPair>,
}

impl DiscoveryCache {
    fn new(markets: Vec<MarketPair>) -> Self {
        Self {
            timestamp_secs: current_unix_secs(),
            markets,
        }
    }

    fn is_expired(&self) -> bool {
        let now = current_unix_secs();
        now.saturating_sub(self.timestamp_secs) > CACHE_TTL_SECS
    }

    fn age_secs(&self) -> u64 {
        current_unix_secs().saturating_sub(self.timestamp_secs)
    }
}

fn current_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Polymarket-only discovery client
pub struct DiscoveryClient {
    http: reqwest::Client,
}

impl DiscoveryClient {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("Failed to build HTTP client"),
        }
    }

    /// Load cache from disk
    async fn load_cache() -> Option<DiscoveryCache> {
        let data = tokio::fs::read_to_string(DISCOVERY_CACHE_PATH).await.ok()?;
        serde_json::from_str(&data).ok()
    }

    /// Save cache to disk
    async fn save_cache(cache: &DiscoveryCache) -> Result<()> {
        let data = serde_json::to_string_pretty(cache)?;
        tokio::fs::write(DISCOVERY_CACHE_PATH, data).await?;
        Ok(())
    }

    /// Discover markets with caching
    pub async fn discover(&self, categories: &[MarketCategory]) -> DiscoveryResult {
        // Try loading cache
        let cached = Self::load_cache().await;

        match cached {
            Some(cache) if !cache.is_expired() && !cache.markets.is_empty() => {
                let count = cache.markets.len();
                info!("📂 Loaded {} markets from cache (age: {}s)", count, cache.age_secs());
                return DiscoveryResult {
                    pairs: cache.markets,
                    total_found: count,
                    errors: vec![],
                };
            }
            Some(cache) if cache.markets.is_empty() => {
                warn!("📂 Cache is empty (age: {}s), refreshing...", cache.age_secs());
            }
            Some(cache) => {
                info!("📂 Cache expired (age: {}s), refreshing...", cache.age_secs());
            }
            None => {
                info!("📂 No cache found, doing full discovery...");
            }
        }

        // Do full discovery
        let result = self.discover_full(categories).await;

        // Save to cache
        if !result.pairs.is_empty() {
            let cache = DiscoveryCache::new(result.pairs.clone());
            if let Err(e) = Self::save_cache(&cache).await {
                warn!("Failed to save discovery cache: {}", e);
            } else {
                info!("💾 Saved {} markets to cache", result.pairs.len());
            }
        }

        result
    }

    /// Full discovery - fetch all markets and filter
    async fn discover_full(&self, categories: &[MarketCategory]) -> DiscoveryResult {
        info!("🔍 Fetching Polymarket markets from {}/markets", GAMMA_API_BASE);

        // Fetch all markets from Gamma API with high limit
        let url = format!("{}/markets?limit=10000&closed=false", GAMMA_API_BASE);

        let resp = match self.http.get(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                warn!("❌ API request failed: {}", e);
                return DiscoveryResult {
                    pairs: vec![],
                    total_found: 0,
                    errors: vec![format!("Failed to fetch markets: {}", e)],
                };
            }
        };

        let gamma_markets: Vec<GammaMarket> = match resp.json().await {
            Ok(m) => m,
            Err(e) => {
                warn!("❌ Failed to parse JSON response: {}", e);
                return DiscoveryResult {
                    pairs: vec![],
                    total_found: 0,
                    errors: vec![format!("Failed to parse markets: {}", e)],
                };
            }
        };

        info!("📊 Fetched {} markets, filtering by categories: {:?}",
            gamma_markets.len(),
            categories.iter().map(|c| c.as_str()).collect::<Vec<_>>());

        // Convert to MarketPair and filter
        let markets: Vec<MarketPair> = gamma_markets
            .into_iter()
            .filter_map(|gm| self.convert_gamma_market(gm, categories))
            .collect();

        if markets.is_empty() {
            warn!("⚠️  No markets matched enabled categories. Check ENABLED_CATEGORIES config.");
            return DiscoveryResult {
                pairs: vec![],
                total_found: 0,
                errors: vec!["No markets matched category filters".to_string()],
            };
        }

        info!("✅ {} markets matched category filters", markets.len());

        // Group markets by category and take top N per category for balanced distribution
        use rustc_hash::FxHashMap;
        let mut by_category: FxHashMap<String, Vec<MarketPair>> = FxHashMap::default();

        for market in markets {
            by_category.entry(market.category.to_string())
                .or_insert_with(Vec::new)
                .push(market);
        }

        // Sort each category by liquidity and take top N per category
        let mut top_markets: Vec<MarketPair> = Vec::new();
        for (category, mut category_markets) in by_category {
            category_markets.sort_by(|a, b| b.liquidity.partial_cmp(&a.liquidity).unwrap_or(std::cmp::Ordering::Equal));
            let count = category_markets.len().min(MARKETS_PER_CATEGORY);
            info!("   📂 {}: taking top {} of {} markets", category, count, category_markets.len());
            top_markets.extend(category_markets.into_iter().take(MARKETS_PER_CATEGORY));
        }

        info!("🏆 Selected {} total markets ({} per category)", top_markets.len(), MARKETS_PER_CATEGORY);

        DiscoveryResult {
            pairs: top_markets.clone(),
            total_found: top_markets.len(),
            errors: vec![],
        }
    }

    /// Convert GammaMarket to MarketPair
    fn convert_gamma_market(&self, market: GammaMarket, categories: &[MarketCategory]) -> Option<MarketPair> {
        // Only filter out explicitly closed markets
        // If active is None or true, we consider it active
        if market.closed == Some(true) {
            return None;
        }

        let slug = market.slug.as_ref()?.clone();
        let question = market.question.as_ref()?.clone();

        // Parse token IDs - MUST be binary market (exactly 2 outcomes: YES/NO)
        let token_ids: Vec<String> = market.clob_token_ids
            .as_ref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();

        if token_ids.len() != 2 {
            return None;  // Only binary markets!
        }

        // Parse liquidity (USD)
        let liquidity = market.liquidity
            .as_ref()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);

        // Filter: minimum liquidity requirement
        if liquidity < MIN_LIQUIDITY_USD {
            return None;
        }

        // Parse 24h volume (USD)
        let volume_24h = market.volume_24hr
            .as_ref()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);

        // Filter: minimum volume requirement (indicates active market)
        if volume_24h < MIN_VOLUME_24H_USD {
            return None;
        }

        // Determine category from question/slug keywords
        let category = self.determine_category(&slug, &question, categories)?;

        Some(MarketPair {
            pair_id: slug.clone().into(),
            category: category.as_str().into(),
            description: question.into(),
            poly_slug: slug.into(),
            poly_yes_token: token_ids[0].clone().into(),
            poly_no_token: token_ids[1].clone().into(),
            liquidity,
        })
    }

    /// Determine market category from slug and question text
    fn determine_category(&self, slug: &str, question: &str, enabled_categories: &[MarketCategory]) -> Option<MarketCategory> {
        let text = format!("{} {}", slug.to_lowercase(), question.to_lowercase());

        for category in enabled_categories {
            let matched = match category {
                MarketCategory::Crypto => {
                    // Expanded crypto keywords
                    text.contains("bitcoin") || text.contains("btc") ||
                    text.contains("ethereum") || text.contains("eth") ||
                    text.contains("crypto") || text.contains("solana") || text.contains("sol") ||
                    text.contains("coin") || text.contains("blockchain") ||
                    text.contains("cardano") || text.contains("ada") ||
                    text.contains("polygon") || text.contains("matic") ||
                    text.contains("avalanche") || text.contains("avax") ||
                    text.contains("polkadot") || text.contains("dot") ||
                    text.contains("chainlink") || text.contains("link") ||
                    text.contains("ripple") || text.contains("xrp") ||
                    text.contains("dogecoin") || text.contains("doge") ||
                    text.contains("shib") || text.contains("shiba") ||
                    text.contains("defi") || text.contains("nft") ||
                    text.contains("memecoin") || text.contains("altcoin") ||
                    text.contains("binance") || text.contains("bnb") ||
                    text.contains("uniswap") || text.contains("uni") ||
                    text.contains("pepe") || text.contains("web3")
                }
                MarketCategory::Sports => {
                    text.contains("nfl") || text.contains("nba") || text.contains("mlb") ||
                    text.contains("nhl") || text.contains("epl") || text.contains("soccer") ||
                    text.contains("football") || text.contains("basketball") || text.contains("game") ||
                    text.contains("match") || text.contains("team") || text.contains("super bowl") ||
                    text.contains("champions") || text.contains("playoff") ||
                    text.contains("formula") || text.contains("f1") ||
                    text.contains("tennis") || text.contains("golf") ||
                    text.contains("boxing") || text.contains("ufc") || text.contains("mma") ||
                    text.contains("olympics") || text.contains("world cup")
                }
                MarketCategory::Politics => {
                    text.contains("election") || text.contains("president") ||
                    text.contains("congress") || text.contains("senate") ||
                    text.contains("trump") || text.contains("biden") ||
                    text.contains("democrat") || text.contains("republican") ||
                    text.contains("vote") || text.contains("poll") || text.contains("political") ||
                    text.contains("governor") || text.contains("supreme court") ||
                    text.contains("impeach") || text.contains("cabinet")
                }
                MarketCategory::Business => {
                    // Expanded business/finance keywords
                    text.contains("stock") || text.contains("share") ||
                    text.contains("economy") || text.contains("economic") ||
                    text.contains("fed") || text.contains("federal reserve") ||
                    text.contains("rate") || text.contains("interest") ||
                    text.contains("gdp") || text.contains("inflation") ||
                    text.contains("recession") || text.contains("unemployment") ||
                    text.contains("company") || text.contains("ipo") ||
                    text.contains("tesla") || text.contains("apple") ||
                    text.contains("amazon") || text.contains("google") ||
                    text.contains("microsoft") || text.contains("meta") ||
                    text.contains("nvidia") || text.contains("amd") ||
                    text.contains("s&p") || text.contains("dow") || text.contains("nasdaq") ||
                    text.contains("earnings") || text.contains("revenue") ||
                    text.contains("profit") || text.contains("ceo") ||
                    text.contains("merger") || text.contains("acquisition") ||
                    text.contains("bank") || text.contains("finance") ||
                    text.contains("dollar") || text.contains("euro") ||
                    text.contains("oil") || text.contains("gold") ||
                    text.contains("commodit") || text.contains("bond")
                }
            };

            if matched {
                return Some(*category);
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_determine_category_crypto() {
        let client = DiscoveryClient::new();
        let categories = MarketCategory::all();

        let cat = client.determine_category("bitcoin-100k", "Will Bitcoin reach $100k?", categories);
        assert_eq!(cat, Some(MarketCategory::Crypto));
    }

    #[test]
    fn test_determine_category_sports() {
        let client = DiscoveryClient::new();
        let categories = MarketCategory::all();

        let cat = client.determine_category("super-bowl-2025", "Who will win Super Bowl?", categories);
        assert_eq!(cat, Some(MarketCategory::Sports));
    }

    #[test]
    fn test_determine_category_politics() {
        let client = DiscoveryClient::new();
        let categories = MarketCategory::all();

        let cat = client.determine_category("2024-election", "Will Trump win?", categories);
        assert_eq!(cat, Some(MarketCategory::Politics));
    }
}
