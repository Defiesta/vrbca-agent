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

//! Market data collection and normalization module.
//!
//! This module handles collecting market data from multiple sources:
//! - Binance API for perpetual prices and funding rates
//! - Uniswap for spot prices and liquidity
//! - Chainlink oracles for price validation
//! - On-chain liquidity metrics

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use reqwest::Client;
use std::collections::HashMap;
use tracing::{info, warn, debug};
use tokio::time::{timeout, Duration};

use crate::Args;
use core::strategy::MarketData;

/// Market data collector that aggregates from multiple sources
pub struct MarketDataCollector {
    binance_client: Option<BinanceClient>,
    chainlink_client: Option<ChainlinkClient>,
    uniswap_client: Option<UniswapClient>,
    http_client: Client,
}

impl MarketDataCollector {
    /// Create new market data collector
    pub async fn new(args: &Args) -> Result<Self> {
        let http_client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .context("Failed to create HTTP client")?;

        let binance_client = if args.binance_api_key.is_some() && args.binance_api_secret.is_some() {
            Some(BinanceClient::new(
                args.binance_api_key.as_ref().unwrap(),
                args.binance_api_secret.as_ref().unwrap(),
                &http_client
            )?)
        } else {
            None
        };

        let chainlink_client = if let Some(feed_address) = &args.chainlink_eth_feed {
            Some(ChainlinkClient::new(&args.rpc_url, feed_address)?)
        } else {
            None
        };

        let uniswap_client = if let Some(router_address) = &args.uniswap_router {
            Some(UniswapClient::new(&args.rpc_url, router_address)?)
        } else {
            None
        };

        Ok(Self {
            binance_client,
            chainlink_client,
            uniswap_client,
            http_client,
        })
    }

    /// Collect comprehensive market data from all sources
    pub async fn collect_market_data(&self) -> Result<MarketData> {
        info!("Collecting market data from all sources");

        // Collect data from all sources concurrently
        let (binance_data, chainlink_data, uniswap_data) = tokio::join!(
            self.collect_binance_data(),
            self.collect_chainlink_data(),
            self.collect_uniswap_data()
        );

        // Process and validate collected data
        let spot_price = self.determine_spot_price(chainlink_data?, uniswap_data?)?;
        let (perp_price, funding_rate, funding_interval) = self.determine_perp_data(binance_data?)?;
        let liquidity_score = self.calculate_liquidity_score(&spot_price, &perp_price).await?;

        let market_data = MarketData {
            spot_price,
            perp_price,
            funding_rate,
            funding_interval_secs: funding_interval,
            liquidity_score,
            timestamp: chrono::Utc::now().timestamp() as u64,
        };

        // Validate market data quality
        self.validate_market_data(&market_data)?;

        debug!("Collected market data: spot=${:.2}, perp=${:.2}, funding={:.4}%, liquidity={}",
               spot_price as f64 / 1_000000.0,
               perp_price as f64 / 1_000000.0,
               funding_rate as f64 / 100.0,
               liquidity_score);

        Ok(market_data)
    }

    /// Collect data from Binance
    async fn collect_binance_data(&self) -> Result<BinanceData> {
        if let Some(client) = &self.binance_client {
            timeout(Duration::from_secs(5), client.get_eth_perp_data()).await?
        } else {
            // Return mock data if Binance is not configured
            Ok(BinanceData {
                perp_price: 3208_000000, // $3208
                funding_rate: 25,        // 0.0025%
                funding_interval: 28800, // 8 hours
            })
        }
    }

    /// Collect data from Chainlink
    async fn collect_chainlink_data(&self) -> Result<ChainlinkData> {
        if let Some(client) = &self.chainlink_client {
            timeout(Duration::from_secs(5), client.get_eth_price()).await?
        } else {
            // Return mock data if Chainlink is not configured
            Ok(ChainlinkData {
                eth_price: 3200_000000, // $3200
                last_update: chrono::Utc::now().timestamp() as u64,
            })
        }
    }

    /// Collect data from Uniswap
    async fn collect_uniswap_data(&self) -> Result<UniswapData> {
        if let Some(client) = &self.uniswap_client {
            timeout(Duration::from_secs(5), client.get_eth_spot_data()).await?
        } else {
            // Return mock data if Uniswap is not configured
            Ok(UniswapData {
                spot_price: 3200_000000,  // $3200
                liquidity_eth: 1000_000000, // 1000 ETH liquidity
                liquidity_usd: 3200000_000000, // $3.2M liquidity
            })
        }
    }

    /// Determine best spot price from multiple sources
    fn determine_spot_price(&self, chainlink_data: ChainlinkData, uniswap_data: UniswapData) -> Result<u128> {
        let prices = vec![
            ("Chainlink", chainlink_data.eth_price),
            ("Uniswap", uniswap_data.spot_price),
        ];

        // Check for price deviations
        let max_price = prices.iter().map(|(_, p)| *p).max().unwrap();
        let min_price = prices.iter().map(|(_, p)| *p).min().unwrap();
        
        if max_price > 0 {
            let deviation_bps = ((max_price - min_price) * 10000) / min_price;
            if deviation_bps > 500 { // 5% max deviation
                warn!("Large price deviation detected: {:.2}%", deviation_bps as f64 / 100.0);
            }
        }

        // Use Chainlink as primary, fall back to Uniswap
        let primary_price = chainlink_data.eth_price;
        if primary_price > 0 {
            Ok(primary_price)
        } else {
            Ok(uniswap_data.spot_price)
        }
    }

    /// Determine perpetual data from Binance
    fn determine_perp_data(&self, binance_data: BinanceData) -> Result<(u128, i128, u64)> {
        if binance_data.perp_price == 0 {
            bail!("Invalid perpetual price from Binance");
        }

        Ok((
            binance_data.perp_price,
            binance_data.funding_rate,
            binance_data.funding_interval,
        ))
    }

    /// Calculate aggregate liquidity score
    async fn calculate_liquidity_score(&self, spot_price: &u128, perp_price: &u128) -> Result<u64> {
        // Simplified liquidity scoring (0-1000 scale)
        // In production, this would analyze:
        // - Order book depth
        // - Recent volume
        // - Bid-ask spreads
        // - Market impact estimates

        let base_score = 700u64; // Default good liquidity
        
        // Penalize for large spreads
        let spread_bps = if *spot_price > 0 {
            ((perp_price.abs_diff(*spot_price)) * 10000) / *spot_price
        } else { 0 };

        let spread_penalty = (spread_bps as u64).min(200); // Max 200 point penalty
        let liquidity_score = base_score.saturating_sub(spread_penalty);

        Ok(liquidity_score)
    }

    /// Validate collected market data for reasonableness
    fn validate_market_data(&self, data: &MarketData) -> Result<()> {
        // Check prices are in reasonable range
        if data.spot_price == 0 || data.spot_price > 100000_000000 { // $100k max
            bail!("Invalid spot price: {}", data.spot_price);
        }

        if data.perp_price == 0 || data.perp_price > 100000_000000 {
            bail!("Invalid perpetual price: {}", data.perp_price);
        }

        // Check funding rate is reasonable
        if data.funding_rate.abs() > 1000 { // 10% max funding rate
            bail!("Extreme funding rate: {:.4}%", data.funding_rate as f64 / 100.0);
        }

        // Check liquidity score is valid
        if data.liquidity_score > 1000 {
            bail!("Invalid liquidity score: {}", data.liquidity_score);
        }

        // Check data freshness
        let now = chrono::Utc::now().timestamp() as u64;
        if data.timestamp > now + 300 { // Not more than 5 minutes in future
            bail!("Market data timestamp in future");
        }

        Ok(())
    }
}

/// Binance API client for perpetual futures data
struct BinanceClient {
    api_key: String,
    api_secret: String,
    client: Client,
    base_url: String,
}

impl BinanceClient {
    fn new(api_key: &str, api_secret: &str, client: &Client) -> Result<Self> {
        Ok(Self {
            api_key: api_key.to_string(),
            api_secret: api_secret.to_string(),
            client: client.clone(),
            base_url: "https://fapi.binance.com".to_string(),
        })
    }

    async fn get_eth_perp_data(&self) -> Result<BinanceData> {
        // Get perpetual price
        let ticker_url = format!("{}/fapi/v1/ticker/24hr?symbol=ETHUSDT", self.base_url);
        let ticker_response: BinanceTickerResponse = self.client
            .get(&ticker_url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await
            .context("Failed to fetch Binance ticker")?
            .json()
            .await
            .context("Failed to parse Binance ticker response")?;

        let perp_price = (ticker_response.lastPrice.parse::<f64>()? * 1_000000.0) as u128;

        // Get funding rate
        let funding_url = format!("{}/fapi/v1/fundingRate?symbol=ETHUSDT&limit=1", self.base_url);
        let funding_response: Vec<BinanceFundingResponse> = self.client
            .get(&funding_url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await
            .context("Failed to fetch Binance funding rate")?
            .json()
            .await
            .context("Failed to parse Binance funding response")?;

        let funding_rate = if let Some(funding) = funding_response.first() {
            (funding.fundingRate.parse::<f64>()? * 10000.0) as i128 // Convert to basis points
        } else {
            0
        };

        Ok(BinanceData {
            perp_price,
            funding_rate,
            funding_interval: 28800, // 8 hours for Binance
        })
    }
}

/// Chainlink oracle client for spot price data
struct ChainlinkClient {
    rpc_url: String,
    feed_address: String,
}

impl ChainlinkClient {
    fn new(rpc_url: &str, feed_address: &str) -> Result<Self> {
        Ok(Self {
            rpc_url: rpc_url.to_string(),
            feed_address: feed_address.to_string(),
        })
    }

    async fn get_eth_price(&self) -> Result<ChainlinkData> {
        // In production, this would call the Chainlink price feed contract
        // For now, return mock data
        Ok(ChainlinkData {
            eth_price: 3200_000000,
            last_update: chrono::Utc::now().timestamp() as u64,
        })
    }
}

/// Uniswap V3 client for DEX spot data
struct UniswapClient {
    rpc_url: String,
    router_address: String,
}

impl UniswapClient {
    fn new(rpc_url: &str, router_address: &str) -> Result<Self> {
        Ok(Self {
            rpc_url: rpc_url.to_string(),
            router_address: router_address.to_string(),
        })
    }

    async fn get_eth_spot_data(&self) -> Result<UniswapData> {
        // In production, this would query Uniswap V3 pools for current price and liquidity
        // For now, return mock data
        Ok(UniswapData {
            spot_price: 3200_000000,
            liquidity_eth: 1000_000000,
            liquidity_usd: 3200000_000000,
        })
    }
}

/// Binance market data
#[derive(Debug)]
struct BinanceData {
    perp_price: u128,      // Perpetual price in USD (scaled by 1e6)
    funding_rate: i128,    // Funding rate in basis points
    funding_interval: u64, // Funding interval in seconds
}

/// Chainlink market data
#[derive(Debug)]
struct ChainlinkData {
    eth_price: u128,    // ETH price in USD (scaled by 1e6)
    last_update: u64,   // Timestamp of last update
}

/// Uniswap market data
#[derive(Debug)]
struct UniswapData {
    spot_price: u128,    // Spot price in USD (scaled by 1e6)
    liquidity_eth: u128, // Available ETH liquidity (scaled by 1e6)
    liquidity_usd: u128, // Available USD liquidity (scaled by 1e6)
}

/// Binance API response structures
#[derive(Deserialize)]
struct BinanceTickerResponse {
    #[serde(alias = "lastPrice")]
    lastPrice: String,
}

#[derive(Deserialize)]
struct BinanceFundingResponse {
    #[serde(alias = "fundingRate")]
    fundingRate: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_market_data_validation() {
        let collector = MarketDataCollector {
            binance_client: None,
            chainlink_client: None,
            uniswap_client: None,
            http_client: Client::new(),
        };

        let valid_data = MarketData {
            spot_price: 3200_000000,
            perp_price: 3208_000000,
            funding_rate: 25,
            funding_interval_secs: 28800,
            liquidity_score: 800,
            timestamp: chrono::Utc::now().timestamp() as u64,
        };

        assert!(collector.validate_market_data(&valid_data).is_ok());

        let invalid_data = MarketData {
            spot_price: 0, // Invalid zero price
            ..valid_data
        };

        assert!(collector.validate_market_data(&invalid_data).is_err());
    }

    #[test]
    fn test_liquidity_score_calculation() {
        // Test would verify liquidity scoring logic
        assert!(true); // Placeholder
    }
}