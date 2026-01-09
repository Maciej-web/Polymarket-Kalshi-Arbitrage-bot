//! Polymarket-Only Multi-Market Trading System
//!
//! A high-performance, production-ready trading system for Polymarket prediction markets
//! with real-time price monitoring and execution across multiple categories.

// pub mod cache;  // Not used in Poly-only mode
pub mod circuit_breaker;
pub mod config;
pub mod discovery;
pub mod execution;
// pub mod kalshi;  // Not used in Poly-only mode
pub mod polymarket;
pub mod polymarket_clob;
pub mod position_tracker;
pub mod types;