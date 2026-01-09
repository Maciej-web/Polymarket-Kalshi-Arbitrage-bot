//! Standalone discovery test - shows detailed logs without running full bot

use prediction_market_arbitrage::discovery::DiscoveryClient;
use prediction_market_arbitrage::config::MarketCategory;

#[tokio::main]
async fn main() {
    // Initialize logging with more verbose output
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("=== POLYMARKET MARKET DISCOVERY TEST ===\n");

    // Test with all categories
    let categories = MarketCategory::all();
    println!("Testing with categories: {:?}\n", categories.iter().map(|c| c.as_str()).collect::<Vec<_>>());

    let client = DiscoveryClient::new();
    let result = client.discover(categories).await;

    println!("\n=== DISCOVERY RESULT ===");
    println!("Total markets found: {}", result.total_found);
    println!("Errors: {:?}", result.errors);

    if !result.pairs.is_empty() {
        println!("\nFirst 5 markets:");
        for (i, pair) in result.pairs.iter().take(5).enumerate() {
            println!("  {}. {} | {}", i + 1, pair.category, pair.description);
        }
    } else {
        println!("\n⚠️  NO MARKETS FOUND!");
        println!("Check the logs above for detailed filtering statistics.");
    }
}
