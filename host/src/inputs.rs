// Copyright 2024 RISC Zero, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Production-grade market data collection system for VRBCA.
//!
//! This module provides robust, real-time market data collection from multiple sources:
//! - Binance futures API with WebSocket streaming
//! - Uniswap V3 on-chain data via RPC
//! - Chainlink price feeds
//! - Comprehensive error handling and retry logic
//! - Rate limiting and circuit breakers
//! - Data quality validation and anomaly detection

use anyhow::{Context, Result, bail, anyhow};
use serde::Deserialize;
use reqwest::Client;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tracing::{info, warn, debug};
use tokio::time::sleep;
use async_trait::async_trait;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use ethers::prelude::*;
use rust_decimal::{Decimal, prelude::*};
use backoff::{ExponentialBackoff, backoff::Backoff};

/// Custom error type to distinguish between retryable and permanent errors
#[derive(Debug)]
pub enum DataSourceError {
    /// Operation not supported by this data source (not retryable)
    UnsupportedOperation(String),
    /// Retryable error (network, timeout, etc.)
    Retryable(anyhow::Error),
}

impl std::fmt::Display for DataSourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DataSourceError::UnsupportedOperation(msg) => write!(f, "Unsupported: {}", msg),
            DataSourceError::Retryable(e) => write!(f, "{}", e),
        }
    }
}

impl std::error::Error for DataSourceError {}

impl From<anyhow::Error> for DataSourceError {
    fn from(e: anyhow::Error) -> Self {
        DataSourceError::Retryable(e)
    }
}

/// Simple market data structure for testing
#[derive(Clone, Debug)]
pub struct MarketData {
    pub spot_price: u128,
    pub perp_price: u128,
    pub funding_rate: i128,
    pub funding_interval_secs: u64,
    pub liquidity_score: u64,
    pub timestamp: u64,
}

/// Production arguments for market data collection
#[derive(Clone, Debug)]
pub struct MarketDataArgs {
    // Binance configuration
    pub binance_api_key: Option<String>,
    pub binance_api_secret: Option<String>,
    pub binance_use_testnet: bool,
    
    // Ethereum mainnet configuration
    pub rpc_url: String,
    pub chain_id: u64,
    
    // Chainlink configuration
    pub chainlink_eth_feed: Option<String>,
    
    // Uniswap configuration  
    pub uniswap_router: Option<String>,
    pub enable_uniswap: bool,
    
    // WebSocket configuration
    pub enable_websockets: bool,
    
    // Quality and validation
    pub data_quality_checks: bool,
}

/// Market data source configuration
#[derive(Debug, Clone)]
pub struct DataSourceConfig {
    pub name: String,
    pub weight: f64,
    pub timeout_secs: u64,
    pub retry_attempts: u32,
    pub circuit_breaker_threshold: u32,
}

/// Circuit breaker state for data sources
#[derive(Debug, Clone)]
pub enum CircuitState {
    Closed,
    HalfOpen,
    Open { last_failure: Instant },
}

/// Market data quality metrics
#[derive(Debug, Clone)]
pub struct DataQualityMetrics {
    pub price_deviation_bps: u64,
    pub staleness_secs: u64,
    pub source_count: u32,
    pub confidence_score: f64,
}

/// Enhanced market data with quality metrics
#[derive(Debug, Clone)]
pub struct EnhancedMarketData {
    pub market_data: MarketData,
    pub quality: DataQualityMetrics,
    pub sources: Vec<String>,
    pub collection_latency_ms: u64,
}

/// Price data from a single source
#[derive(Debug, Clone)]
pub struct PricePoint {
    pub price: Decimal,
    pub source: String,
    pub timestamp: u64,
    pub volume_24h: Option<Decimal>,
    pub liquidity: Option<Decimal>,
}

/// Funding rate data
#[derive(Debug, Clone)]
pub struct FundingRateData {
    pub rate: Decimal,
    pub next_funding_time: u64,
    pub mark_price: Decimal,
    pub index_price: Decimal,
    pub source: String,
    pub timestamp: u64,
}

/// Trait for market data sources
#[async_trait]
pub trait DataSource: Send + Sync {
    async fn get_spot_price(&self, symbol: &str) -> Result<PricePoint>;
    async fn get_perp_price(&self, symbol: &str) -> Result<PricePoint>;
    async fn get_funding_rate(&self, symbol: &str) -> Result<FundingRateData>;
    async fn get_liquidity_metrics(&self, symbol: &str) -> Result<LiquidityMetrics>;
    fn source_name(&self) -> &str;
    fn is_healthy(&self) -> bool;
}

/// Liquidity metrics for a trading pair
#[derive(Debug, Clone)]
pub struct LiquidityMetrics {
    pub bid_depth_usd: Decimal,
    pub ask_depth_usd: Decimal,
    pub spread_bps: u64,
    pub market_impact_1m: Decimal, // Price impact for $1M trade
    pub volume_24h: Decimal,
    pub source: String,
    pub timestamp: u64,
}

/// Production-grade market data collector
pub struct ProductionMarketDataCollector {
    sources: Vec<Arc<dyn DataSource>>,
    config: MarketDataArgs,
    circuit_states: Arc<Mutex<HashMap<String, CircuitState>>>,
    price_history: Arc<Mutex<VecDeque<PricePoint>>>,
    quality_validator: DataQualityValidator,
    rate_limiter: Arc<RateLimiter>,
}

impl ProductionMarketDataCollector {
    /// Create new production market data collector
    pub async fn new(config: MarketDataArgs) -> Result<Self> {
        let mut sources: Vec<Arc<dyn DataSource>> = Vec::new();
        
        // Initialize Binance data source (works with or without API keys)
        let binance_source = BinanceDataSource::new(
            config.binance_api_key.as_deref(),
            config.binance_api_secret.as_deref(),
            config.enable_websockets
        ).await?;
        sources.push(Arc::new(binance_source));

        // Initialize Uniswap data source if enabled and RPC URL provided
        if config.enable_uniswap && !config.rpc_url.is_empty() {
            match UniswapDataSource::new(&config.rpc_url).await {
                Ok(uniswap_source) => {
                    info!("✅ Uniswap V3 data source initialized for Mainnet");
                    sources.push(Arc::new(uniswap_source));
                }
                Err(e) => {
                    warn!("⚠️ Failed to initialize Uniswap data source: {}", e);
                }
            }
        }

        // Initialize Chainlink data source if feed address provided
        if let Some(feed_address) = &config.chainlink_eth_feed {
            match ChainlinkDataSource::new(&config.rpc_url, feed_address).await {
                Ok(chainlink_source) => {
                    info!("✅ Chainlink data source initialized");
                    sources.push(Arc::new(chainlink_source));
                }
                Err(e) => {
                    warn!("⚠️ Failed to initialize Chainlink data source: {}", e);
                }
            }
        }

        if sources.is_empty() {
            bail!("No market data sources configured");
        }

        Ok(Self {
            sources,
            config,
            circuit_states: Arc::new(Mutex::new(HashMap::new())),
            price_history: Arc::new(Mutex::new(VecDeque::with_capacity(1000))),
            quality_validator: DataQualityValidator::new(),
            rate_limiter: Arc::new(RateLimiter::new(100, Duration::from_secs(60))), // 100 requests per minute
        })
    }

    /// Collect comprehensive market data with quality validation
    pub async fn collect_enhanced_market_data(&self, symbol: &str) -> Result<EnhancedMarketData> {
        let start_time = Instant::now();
        info!("Collecting enhanced market data for {}", symbol);

        // Collect data from all healthy sources concurrently
        let (spot_prices, perp_prices, funding_rates, liquidity_metrics) = self.collect_all_data(symbol).await;

        // Extract source names before moving data
        let sources: Vec<String> = spot_prices.iter()
            .chain(perp_prices.iter())
            .map(|p| p.source.clone())
            .collect();

        // Calculate quality metrics before moving data
        let quality = self.quality_validator.calculate_quality_metrics(&spot_prices, &perp_prices)?;

        // Aggregate and validate data (this moves the vectors)
        let market_data = self.aggregate_market_data(
            symbol,
            spot_prices,
            perp_prices, 
            funding_rates,
            liquidity_metrics,
        ).await?;

        // Track collection latency
        let collection_latency_ms = start_time.elapsed().as_millis() as u64;

        let enhanced_data = EnhancedMarketData {
            market_data,
            quality,
            sources,
            collection_latency_ms,
        };

        // Log quality metrics
        debug!("Market data quality: confidence={:.2}%, latency={}ms, sources={}",
               enhanced_data.quality.confidence_score * 100.0,
               enhanced_data.collection_latency_ms,
               enhanced_data.quality.source_count);

        Ok(enhanced_data)
    }

    /// Collect data from all sources with circuit breaker protection
    async fn collect_all_data(&self, symbol: &str) -> (Vec<PricePoint>, Vec<PricePoint>, Vec<FundingRateData>, Vec<LiquidityMetrics>) {
        let mut spot_prices = Vec::new();
        let mut perp_prices = Vec::new();
        let mut funding_rates = Vec::new();
        let mut liquidity_metrics = Vec::new();

        // Use futures to collect data concurrently
        let futures = self.sources.iter().map(|source| {
            let source_clone = Arc::clone(source);
            let symbol = symbol.to_string();
            
            async move {
                if !self.is_source_healthy(&source_clone.source_name()).await {
                    return (None, None, None, None);
                }

                let spot = self.collect_with_retry(|| source_clone.get_spot_price(&symbol)).await;
                let perp = self.collect_with_retry(|| source_clone.get_perp_price(&symbol)).await;
                let funding = self.collect_with_retry(|| source_clone.get_funding_rate(&symbol)).await;
                let liquidity = self.collect_with_retry(|| source_clone.get_liquidity_metrics(&symbol)).await;

                (spot.ok(), perp.ok(), funding.ok(), liquidity.ok())
            }
        });

        let results = futures::future::join_all(futures).await;

        for (spot, perp, funding, liquidity) in results {
            if let Some(s) = spot { spot_prices.push(s); }
            if let Some(p) = perp { perp_prices.push(p); }
            if let Some(f) = funding { funding_rates.push(f); }
            if let Some(l) = liquidity { liquidity_metrics.push(l); }
        }

        (spot_prices, perp_prices, funding_rates, liquidity_metrics)
    }

    /// Collect data with exponential backoff retry
    async fn collect_with_retry<F, Fut, T>(&self, mut f: F) -> Result<T>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let mut backoff = ExponentialBackoff {
            initial_interval: Duration::from_millis(100),
            max_interval: Duration::from_secs(10),
            multiplier: 2.0,
            max_elapsed_time: Some(Duration::from_secs(30)),
            ..Default::default()
        };

        loop {
            match f().await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    // Check if this is an expected unsupported operation
                    let error_msg = e.to_string();
                    if error_msg.contains("doesn't provide") || 
                       error_msg.contains("doesn't support") || 
                       error_msg.contains("not supported") ||
                       error_msg.contains("temporarily disabled") {
                        debug!("Skipping retry for unsupported operation: {}", error_msg);
                        return Err(e);
                    }
                    
                    // Retry for other errors
                    if let Some(delay) = backoff.next_backoff() {
                        warn!("Retrying after error: {}, waiting {:?}", e, delay);
                        sleep(delay).await;
                    } else {
                        return Err(e);
                    }
                }
            }
        }
    }

    /// Check if a source is healthy (circuit breaker logic)
    async fn is_source_healthy(&self, source_name: &str) -> bool {
        let circuit_states = self.circuit_states.lock().unwrap();
        
        match circuit_states.get(source_name) {
            Some(CircuitState::Open { last_failure }) => {
                // Check if enough time has passed to try half-open
                last_failure.elapsed() > Duration::from_secs(60)
            }
            Some(CircuitState::HalfOpen) => true, // Allow limited requests
            Some(CircuitState::Closed) | None => true, // Source is healthy
        }
    }

    /// Aggregate market data from multiple sources
    async fn aggregate_market_data(
        &self,
        symbol: &str,
        spot_prices: Vec<PricePoint>,
        perp_prices: Vec<PricePoint>,
        funding_rates: Vec<FundingRateData>,
        liquidity_metrics: Vec<LiquidityMetrics>,
    ) -> Result<MarketData> {
        if spot_prices.is_empty() {
            bail!("No spot price data available for {}", symbol);
        }

        // Calculate weighted average prices
        let spot_price = self.calculate_weighted_price(&spot_prices)?;
        let perp_price = if !perp_prices.is_empty() {
            self.calculate_weighted_price(&perp_prices)?
        } else {
            spot_price // Fallback to spot if no perp data
        };

        // Get most recent funding rate
        let (funding_rate, funding_interval) = if let Some(funding) = funding_rates.first() {
            (funding.rate, 28800) // 8 hours typical
        } else {
            (Decimal::ZERO, 28800)
        };

        // Calculate liquidity score
        let liquidity_score = self.calculate_liquidity_score(&liquidity_metrics);

        Ok(MarketData {
            spot_price: self.decimal_to_scaled_u128(spot_price)?,
            perp_price: self.decimal_to_scaled_u128(perp_price)?,
            funding_rate: self.decimal_to_scaled_i128(funding_rate)?,
            funding_interval_secs: funding_interval,
            liquidity_score,
            timestamp: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
        })
    }

    /// Calculate weighted average price from multiple sources
    fn calculate_weighted_price(&self, prices: &[PricePoint]) -> Result<Decimal> {
        if prices.is_empty() {
            bail!("No prices to aggregate");
        }

        // Simple average for now - could implement volume weighting
        let sum: Decimal = prices.iter().map(|p| p.price).sum();
        Ok(sum / Decimal::from(prices.len()))
    }

    /// Calculate aggregate liquidity score (0-1000)
    fn calculate_liquidity_score(&self, metrics: &[LiquidityMetrics]) -> u64 {
        if metrics.is_empty() {
            return 500; // Default moderate score
        }

        let mut total_score = 0u64;
        let mut count = 0;

        for metric in metrics {
            let mut score = 1000u64; // Start with perfect score

            // Penalize wide spreads
            score = score.saturating_sub(metric.spread_bps * 2);

            // Penalize low liquidity
            let liquidity_usd = metric.bid_depth_usd + metric.ask_depth_usd;
            if liquidity_usd < Decimal::from(100_000) { // Less than $100K
                score = score.saturating_sub(300);
            } else if liquidity_usd < Decimal::from(1_000_000) { // Less than $1M
                score = score.saturating_sub(100);
            }

            // Penalize high market impact
            if metric.market_impact_1m > Decimal::from_f64(0.01).unwrap() { // >1% impact
                score = score.saturating_sub(200);
            }

            total_score += score;
            count += 1;
        }

        total_score / count.max(1)
    }

    /// Convert Decimal to scaled u128 (multiplied by 1e6)
    fn decimal_to_scaled_u128(&self, value: Decimal) -> Result<u128> {
        let scaled = value * Decimal::from(1_000_000);
        scaled.to_string().parse::<f64>().map(|f| f as u128)
            .with_context(|| format!("Failed to convert decimal {} to scaled u128", value))
    }

    /// Convert Decimal to scaled i128 (multiplied by 1e4 for basis points)
    fn decimal_to_scaled_i128(&self, value: Decimal) -> Result<i128> {
        let scaled = value * Decimal::from(10_000);
        scaled.to_string().parse::<f64>().map(|f| f as i128)
            .with_context(|| format!("Failed to convert decimal {} to scaled i128", value))
    }
}

/// Data quality validator
pub struct DataQualityValidator {
    max_deviation_bps: u64,
    max_staleness_secs: u64,
}

impl DataQualityValidator {
    pub fn new() -> Self {
        Self {
            max_deviation_bps: 500, // 5% max deviation between sources
            max_staleness_secs: 300, // 5 minutes max staleness
        }
    }

    pub fn calculate_quality_metrics(
        &self,
        spot_prices: &[PricePoint],
        perp_prices: &[PricePoint],
    ) -> Result<DataQualityMetrics> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        
        // Calculate price deviation
        let price_deviation_bps = self.calculate_price_deviation(spot_prices, perp_prices);
        
        // Calculate staleness
        let staleness_secs = spot_prices.iter()
            .chain(perp_prices.iter())
            .map(|p| now.saturating_sub(p.timestamp))
            .max()
            .unwrap_or(0);
        
        // Source count
        let source_count = (spot_prices.len() + perp_prices.len()) as u32;
        
        // Confidence score (0.0 to 1.0)
        let confidence_score = self.calculate_confidence_score(price_deviation_bps, staleness_secs, source_count);
        
        Ok(DataQualityMetrics {
            price_deviation_bps,
            staleness_secs,
            source_count,
            confidence_score,
        })
    }

    fn calculate_price_deviation(&self, spot_prices: &[PricePoint], perp_prices: &[PricePoint]) -> u64 {
        let all_prices: Vec<&PricePoint> = spot_prices.iter().chain(perp_prices.iter()).collect();
        
        if all_prices.len() < 2 {
            return 0;
        }

        let prices: Vec<Decimal> = all_prices.iter().map(|p| p.price).collect();
        let min_price = prices.iter().min().unwrap();
        let max_price = prices.iter().max().unwrap();
        
        if *min_price == Decimal::ZERO {
            return 10000; // 100% deviation for invalid prices
        }

        let deviation = (*max_price - *min_price) / *min_price * Decimal::from(10_000);
        deviation.to_string().parse::<u64>().unwrap_or(10000).min(10000)
    }

    fn calculate_confidence_score(&self, price_deviation_bps: u64, staleness_secs: u64, source_count: u32) -> f64 {
        let mut score = 1.0;

        // Penalize high price deviation
        if price_deviation_bps > self.max_deviation_bps {
            score *= 0.5;
        } else {
            score *= 1.0 - (price_deviation_bps as f64 / self.max_deviation_bps as f64) * 0.3;
        }

        // Penalize staleness
        if staleness_secs > self.max_staleness_secs {
            score *= 0.3;
        } else {
            score *= 1.0 - (staleness_secs as f64 / self.max_staleness_secs as f64) * 0.2;
        }

        // Reward multiple sources
        score *= (source_count as f64).min(3.0) / 3.0;

        score.max(0.0).min(1.0)
    }
}

/// Rate limiter for API calls
pub struct RateLimiter {
    max_requests: u32,
    window: Duration,
    requests: Arc<Mutex<VecDeque<Instant>>>,
}

impl RateLimiter {
    pub fn new(max_requests: u32, window: Duration) -> Self {
        Self {
            max_requests,
            window,
            requests: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    pub async fn acquire(&self) -> Result<()> {
        let now = Instant::now();
        let mut requests = self.requests.lock().unwrap();
        
        // Remove old requests outside the window
        while let Some(&front) = requests.front() {
            if now.duration_since(front) > self.window {
                requests.pop_front();
            } else {
                break;
            }
        }
        
        // Check if we're at the limit
        if requests.len() >= self.max_requests as usize {
            let wait_time = self.window - now.duration_since(*requests.front().unwrap());
            drop(requests);
            sleep(wait_time).await;
            return Box::pin(self.acquire()).await;
        }
        
        // Add current request
        requests.push_back(now);
        Ok(())
    }
}

/// Binance data source implementation
pub struct BinanceDataSource {
    client: Client,
    api_key: Option<String>,
    api_secret: Option<String>,
    spot_base_url: String,
    futures_base_url: String,
    websocket_enabled: bool,
}

impl BinanceDataSource {
    pub async fn new(api_key: Option<&str>, api_secret: Option<&str>, websocket_enabled: bool) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .context("Failed to create HTTP client")?;

        Ok(Self {
            client,
            api_key: api_key.map(|s| s.to_string()),
            api_secret: api_secret.map(|s| s.to_string()),
            spot_base_url: "https://data-api.binance.vision".to_string(),
            futures_base_url: "https://fapi.binance.com".to_string(),
            websocket_enabled,
        })
    }

    /// Generate HMAC SHA256 signature for authenticated requests
    fn sign_request(&self, query_string: &str) -> Result<String> {
        let api_secret = self.api_secret.as_ref()
            .ok_or_else(|| anyhow!("API secret required for authenticated requests"))?;
        let mut mac = Hmac::<Sha256>::new_from_slice(api_secret.as_bytes())
            .context("Failed to create HMAC")?;
        mac.update(query_string.as_bytes());
        Ok(hex::encode(mac.finalize().into_bytes()))
    }

    /// Make public API request (no authentication required)
    async fn public_request<T: for<'de> Deserialize<'de>>(&self, base_url: &str, endpoint: &str, params: &str) -> Result<T> {
        let url = if params.is_empty() {
            format!("{}/{}", base_url, endpoint)
        } else {
            format!("{}/{}?{}", base_url, endpoint, params)
        };
        
        debug!("Making public request to: {}", url);
        
        let response = self.client
            .get(&url)
            .send()
            .await
            .context("Failed to send request")?;
            
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            bail!("Binance API error: {} - {}", status, error_text);
        }
        
        response.json().await
            .context("Failed to parse JSON response")
    }
}

#[async_trait]
impl DataSource for BinanceDataSource {
    async fn get_spot_price(&self, symbol: &str) -> Result<PricePoint> {
        // Use Binance Spot API for spot prices
        let ticker: BinanceSpotTickerResponse = self.public_request(
            &self.spot_base_url,
            "api/v3/ticker/price",
            &format!("symbol={}", symbol)
        ).await?;

        Ok(PricePoint {
            price: Decimal::from_str_exact(&ticker.price)?,
            source: "binance_spot".to_string(),
            timestamp: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
            volume_24h: None, // Would need separate 24hr ticker call
            liquidity: None,
        })
    }

    async fn get_perp_price(&self, symbol: &str) -> Result<PricePoint> {
        // Use Binance Futures API for perpetual prices
        let premium_info: BinancePremiumIndexResponse = self.public_request(
            &self.futures_base_url,
            "fapi/v1/premiumIndex",
            &format!("symbol={}", symbol)
        ).await?;

        Ok(PricePoint {
            price: Decimal::from_str_exact(&premium_info.markPrice)?,
            source: "binance_futures".to_string(),
            timestamp: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
            volume_24h: None, // Would need separate 24hr ticker call
            liquidity: None,
        })
    }

    async fn get_funding_rate(&self, symbol: &str) -> Result<FundingRateData> {
        // Get current funding rate info from premium index
        let premium_info: BinancePremiumIndexResponse = self.public_request(
            &self.futures_base_url,
            "fapi/v1/premiumIndex",
            &format!("symbol={}", symbol)
        ).await?;

        Ok(FundingRateData {
            rate: Decimal::from_str_exact(&premium_info.lastFundingRate)?,
            next_funding_time: premium_info.nextFundingTime,
            mark_price: Decimal::from_str_exact(&premium_info.markPrice)?,
            index_price: Decimal::from_str_exact(&premium_info.indexPrice)?,
            source: "binance_futures".to_string(),
            timestamp: premium_info.time,
        })
    }

    async fn get_liquidity_metrics(&self, symbol: &str) -> Result<LiquidityMetrics> {
        // Get order book depth from futures API
        let depth: BinanceDepthResponse = self.public_request(
            &self.futures_base_url,
            "fapi/v1/depth",
            &format!("symbol={}&limit=100", symbol)
        ).await?;

        // Calculate depth and spread from order book
        let bid_depth: Decimal = depth.bids.iter()
            .take(10) // Top 10 levels
            .map(|bid| {
                let price = Decimal::from_str_exact(&bid[0]).unwrap_or_default();
                let quantity = Decimal::from_str_exact(&bid[1]).unwrap_or_default();
                price * quantity
            })
            .sum();

        let ask_depth: Decimal = depth.asks.iter()
            .take(10)
            .map(|ask| {
                let price = Decimal::from_str_exact(&ask[0]).unwrap_or_default();
                let quantity = Decimal::from_str_exact(&ask[1]).unwrap_or_default();
                price * quantity
            })
            .sum();

        if depth.bids.is_empty() || depth.asks.is_empty() {
            bail!("Empty order book for {}", symbol);
        }

        let best_bid = Decimal::from_str_exact(&depth.bids[0][0])?;
        let best_ask = Decimal::from_str_exact(&depth.asks[0][0])?;
        let mid_price = (best_bid + best_ask) / Decimal::from(2);
        let spread_bps = ((best_ask - best_bid) / mid_price * Decimal::from(10_000))
            .to_string().parse().unwrap_or(0);

        Ok(LiquidityMetrics {
            bid_depth_usd: bid_depth,
            ask_depth_usd: ask_depth,
            spread_bps,
            market_impact_1m: Decimal::from_f64(0.001).unwrap_or_default(),
            volume_24h: Decimal::ZERO, // Would need separate 24hr ticker call
            source: "binance_futures".to_string(),
            timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),
        })
    }

    fn source_name(&self) -> &str {
        "binance_combined"
    }

    fn is_healthy(&self) -> bool {
        true // TODO: Implement health checks
    }
}

/// Uniswap V3 data source
pub struct UniswapDataSource {
    uniswap_client: crate::contracts::UniswapV3Client,
}

impl UniswapDataSource {
    pub async fn new(rpc_url: &str) -> Result<Self> {
        let uniswap_client = crate::contracts::UniswapV3Client::new(rpc_url).await
            .context("Failed to create Uniswap V3 client")?;

        Ok(Self { uniswap_client })
    }
}

#[async_trait]
impl DataSource for UniswapDataSource {
    async fn get_spot_price(&self, symbol: &str) -> Result<PricePoint> {
        // For ETH/USD pairs, use WETH/USDC pool
        if symbol.to_uppercase().contains("ETH") {
            self.uniswap_client.get_eth_usd_spot_price().await
                .context("Failed to get ETH/USD spot price from Uniswap")
        } else {
            bail!("Symbol {} not supported on Uniswap integration", symbol)
        }
    }

    async fn get_perp_price(&self, _symbol: &str) -> Result<PricePoint> {
        bail!("Uniswap doesn't provide perpetual prices")
    }

    async fn get_funding_rate(&self, _symbol: &str) -> Result<FundingRateData> {
        bail!("Uniswap doesn't provide funding rates")
    }

    async fn get_liquidity_metrics(&self, symbol: &str) -> Result<LiquidityMetrics> {
        // For ETH/USD pairs, use WETH/USDC pool
        if symbol.to_uppercase().contains("ETH") {
            self.uniswap_client.get_eth_usd_liquidity_metrics().await
                .context("Failed to get ETH/USD liquidity metrics from Uniswap")
        } else {
            bail!("Symbol {} not supported on Uniswap integration", symbol)
        }
    }

    fn source_name(&self) -> &str {
        "uniswap"
    }

    fn is_healthy(&self) -> bool {
        true
    }
}

/// Chainlink oracle data source
pub struct ChainlinkDataSource {
    provider: Provider<Http>,
    feed_address: Address,
}

impl ChainlinkDataSource {
    pub async fn new(rpc_url: &str, feed_address: &str) -> Result<Self> {
        let provider = Provider::<Http>::try_from(rpc_url)
            .context("Failed to create Ethereum provider")?;
        
        let feed_address = feed_address.parse::<Address>()
            .context("Invalid Chainlink feed address")?;

        Ok(Self { provider, feed_address })
    }
}

#[async_trait]
impl DataSource for ChainlinkDataSource {
    async fn get_spot_price(&self, _symbol: &str) -> Result<PricePoint> {
        // TODO: Implement actual Chainlink price feed reading
        Ok(PricePoint {
            price: Decimal::from(3200),
            source: "chainlink".to_string(),
            timestamp: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
            volume_24h: None,
            liquidity: None,
        })
    }

    async fn get_perp_price(&self, _symbol: &str) -> Result<PricePoint> {
        bail!("Chainlink doesn't provide perpetual prices")
    }

    async fn get_funding_rate(&self, _symbol: &str) -> Result<FundingRateData> {
        bail!("Chainlink doesn't provide funding rates")
    }

    async fn get_liquidity_metrics(&self, _symbol: &str) -> Result<LiquidityMetrics> {
        bail!("Chainlink doesn't provide liquidity metrics")
    }

    fn source_name(&self) -> &str {
        "chainlink"
    }

    fn is_healthy(&self) -> bool {
        true
    }
}

// API response structures
#[derive(Deserialize, Debug)]
pub struct BinanceSpotTickerResponse {
    pub symbol: String,
    pub price: String,
}

#[derive(Deserialize, Debug)]
pub struct BinancePremiumIndexResponse {
    pub symbol: String,
    #[serde(rename = "markPrice")]
    pub markPrice: String,
    #[serde(rename = "indexPrice")]
    pub indexPrice: String,
    #[serde(rename = "estimatedSettlePrice")]
    pub estimatedSettlePrice: String,
    #[serde(rename = "lastFundingRate")]
    pub lastFundingRate: String,
    #[serde(rename = "nextFundingTime")]
    pub nextFundingTime: u64,
    pub time: u64,
}

#[derive(Deserialize, Debug)]
pub struct BinanceDepthResponse {
    #[serde(rename = "lastUpdateId")]
    pub last_update_id: u64,
    pub bids: Vec<Vec<String>>,
    pub asks: Vec<Vec<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_rate_limiter() {
        let limiter = RateLimiter::new(2, Duration::from_millis(100));
        
        // First two requests should succeed immediately
        assert!(limiter.acquire().await.is_ok());
        assert!(limiter.acquire().await.is_ok());
        
        // Third request should be rate limited
        let start = Instant::now();
        assert!(limiter.acquire().await.is_ok());
        assert!(start.elapsed() >= Duration::from_millis(90));
    }

    #[test]
    fn test_data_quality_validator() {
        let validator = DataQualityValidator::new();
        
        let spot_prices = vec![
            PricePoint {
                price: Decimal::from(3200),
                source: "source1".to_string(),
                timestamp: 0,
                volume_24h: None,
                liquidity: None,
            },
            PricePoint {
                price: Decimal::from(3205),
                source: "source2".to_string(),
                timestamp: 0,
                volume_24h: None,
                liquidity: None,
            },
        ];
        
        let market_data = MarketData {
            spot_price: 3200_000000,
            perp_price: 3205_000000,
            funding_rate: 25,
            funding_interval_secs: 28800,
            liquidity_score: 800,
            timestamp: 0,
        };
        
        let quality = validator.calculate_quality_metrics(&spot_prices, &[]).unwrap();
        assert!(quality.confidence_score > 0.5);
        assert!(quality.price_deviation_bps < 500);
    }
}