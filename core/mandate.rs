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

//! Mandate module for immutable trading mandate definitions.
//!
//! This module defines the immutable constraints and objectives that govern
//! the agent's behavior. Once registered on-chain, these mandates cannot be
//! changed, ensuring that the agent operates within its original parameters.

use crate::risk::RiskConfig;
use crate::strategy::BasisCaptureStrategy;

/// Unique identifier for an agent
pub type AgentId = [u8; 32];

/// Unique identifier for a mandate
pub type MandateId = [u8; 32];

/// Hash of the strategy code that must be proven in the zkVM
pub type StrategyCodeHash = [u8; 32];

/// Complete trading mandate that defines agent behavior
#[derive(Clone, Debug)]
pub struct TradingMandate {
    /// Unique mandate identifier
    pub id: MandateId,
    /// Version of this mandate (for upgrades)
    pub version: u32,
    /// Strategy configuration
    pub strategy: BasisCaptureStrategy,
    /// Risk management configuration
    pub risk_config: RiskConfig,
    /// Approved trading venues
    pub approved_venues: Vec<TradingVenue>,
    /// Approved assets for trading
    pub approved_assets: Vec<AssetConfig>,
    /// Operational parameters
    pub operational_params: OperationalParams,
    /// Emergency conditions that trigger halt
    pub emergency_conditions: EmergencyConditions,
    /// Code hash of the strategy implementation
    pub strategy_code_hash: StrategyCodeHash,
}

/// Configuration for a trading venue
#[derive(Clone, Debug)]
pub struct TradingVenue {
    /// Venue identifier (e.g., "BINANCE", "UNISWAP")
    pub name: [u8; 16],
    /// Venue type (CEX, DEX, etc.)
    pub venue_type: VenueType,
    /// Maximum position size on this venue (basis points of total capital)
    pub max_position_size_bps: u64,
    /// Venue-specific risk parameters
    pub risk_params: VenueRiskParams,
    /// Whether this venue is currently active
    pub is_active: bool,
}

/// Types of trading venues
#[derive(Clone, Debug)]
pub enum VenueType {
    /// Centralized exchange (Binance, etc.)
    CentralizedExchange,
    /// Decentralized exchange (Uniswap, etc.)
    DecentralizedExchange,
    /// Options market
    OptionsMarket,
    /// Lending protocol (for funding)
    LendingProtocol,
}

/// Risk parameters specific to a venue
#[derive(Clone, Debug)]
pub struct VenueRiskParams {
    /// Maximum leverage allowed on this venue
    pub max_leverage_bps: u64,
    /// Counterparty risk limit
    pub counterparty_limit: u128,
    /// Minimum liquidity required (USD)
    pub min_liquidity: u128,
    /// Maximum slippage tolerance (basis points)
    pub max_slippage_bps: u64,
}

/// Configuration for a tradeable asset
#[derive(Clone, Debug)]
pub struct AssetConfig {
    /// Asset symbol (e.g., "ETH", "BTC")
    pub symbol: [u8; 4],
    /// Asset type (spot, perpetual, option, etc.)
    pub asset_type: AssetType,
    /// Maximum position size in this asset (basis points of portfolio)
    pub max_position_bps: u64,
    /// Minimum liquidity required for trading
    pub min_liquidity_usd: u128,
    /// Oracle configuration for pricing
    pub oracle_config: OracleConfig,
    /// Asset-specific risk parameters
    pub risk_multiplier: u64, // Additional risk weighting (10000 = 1.0x)
}

/// Types of tradeable assets
#[derive(Clone, Debug)]
pub enum AssetType {
    /// Spot asset (ETH, BTC, etc.)
    Spot,
    /// Perpetual futures
    Perpetual,
    /// Options contract
    Option { expiry: u64, strike: u128 },
    /// Interest rate derivative
    InterestRate,
}

/// Oracle configuration for asset pricing
#[derive(Clone, Debug)]
pub struct OracleConfig {
    /// Primary oracle source
    pub primary_oracle: OracleSource,
    /// Fallback oracle sources
    pub fallback_oracles: Vec<OracleSource>,
    /// Maximum deviation between oracles (basis points)
    pub max_deviation_bps: u64,
    /// Maximum staleness allowed (seconds)
    pub max_staleness_secs: u64,
}

/// Oracle source for price feeds
#[derive(Clone, Debug)]
pub enum OracleSource {
    /// Chainlink price feed
    Chainlink { feed_address: [u8; 20] },
    /// Uniswap V3 TWAP
    UniswapTWAP { pool_address: [u8; 20], period: u32 },
    /// Binance API
    BinanceAPI { symbol: [u8; 16] },
    /// Custom oracle contract
    Custom { contract_address: [u8; 20] },
}

/// Operational parameters for the agent
#[derive(Clone, Debug)]
pub struct OperationalParams {
    /// Minimum time between strategy executions (seconds)
    pub min_execution_interval: u64,
    /// Maximum time agent can run without generating proof (seconds)
    pub max_execution_timeout: u64,
    /// Gas price limits for on-chain operations
    pub gas_limits: GasLimits,
    /// Profit-taking parameters
    pub profit_taking: ProfitTakingParams,
    /// Rebalancing parameters
    pub rebalancing: RebalancingParams,
}

/// Gas limit configuration
#[derive(Clone, Debug)]
pub struct GasLimits {
    /// Maximum gas price for urgent operations (wei)
    pub max_gas_price_urgent: u128,
    /// Maximum gas price for normal operations (wei)
    pub max_gas_price_normal: u128,
    /// Maximum total gas budget per epoch (wei)
    pub max_gas_budget_per_epoch: u128,
}

/// Profit-taking configuration
#[derive(Clone, Debug)]
pub struct ProfitTakingParams {
    /// Target profit threshold (basis points)
    pub target_profit_bps: u64,
    /// Stop-loss threshold (basis points)
    pub stop_loss_bps: u64,
    /// Trailing stop configuration
    pub trailing_stop_bps: u64,
    /// Profit-taking frequency (minimum time between takes)
    pub min_take_interval_secs: u64,
}

/// Rebalancing configuration
#[derive(Clone, Debug)]
pub struct RebalancingParams {
    /// Threshold for triggering rebalance (basis points deviation)
    pub rebalance_threshold_bps: u64,
    /// Minimum time between rebalances (seconds)
    pub min_rebalance_interval: u64,
    /// Maximum trade size for rebalancing (basis points of position)
    pub max_rebalance_size_bps: u64,
}

/// Emergency conditions that trigger agent halt
#[derive(Clone, Debug)]
pub struct EmergencyConditions {
    /// Maximum portfolio drawdown before halt (basis points)
    pub max_portfolio_drawdown_bps: u64,
    /// Maximum individual position loss before halt (basis points)
    pub max_position_loss_bps: u64,
    /// Minimum liquidity before halt (USD)
    pub min_market_liquidity: u128,
    /// Maximum funding rate before halt (basis points)
    pub max_funding_rate_bps: u64,
    /// Oracle failure conditions
    pub oracle_failure_conditions: Vec<OracleFailureCondition>,
    /// Venue-specific halt conditions
    pub venue_halt_conditions: Vec<VenueHaltCondition>,
}

/// Conditions that indicate oracle failure
#[derive(Clone, Debug)]
pub enum OracleFailureCondition {
    /// Price deviation exceeds threshold
    PriceDeviation { max_deviation_bps: u64 },
    /// Oracle is stale beyond threshold
    Staleness { max_staleness_secs: u64 },
    /// Oracle is completely offline
    OracleOffline,
    /// Insufficient oracle diversity
    InsufficientDiversity { min_sources: u8 },
}

/// Conditions that trigger venue-specific halts
#[derive(Clone, Debug)]
pub struct VenueHaltCondition {
    /// Venue this condition applies to
    pub venue: [u8; 16],
    /// Condition that triggers halt
    pub condition: HaltCondition,
}

/// Types of halt conditions
#[derive(Clone, Debug)]
pub enum HaltCondition {
    /// Trading suspended on venue
    TradingSuspended,
    /// Venue API unavailable
    APIUnavailable { max_downtime_secs: u64 },
    /// Unusual market conditions (e.g., extreme volatility)
    MarketAbnormal { volatility_threshold_bps: u64 },
    /// Liquidity below threshold
    LiquidityDried { min_liquidity: u128 },
}

impl TradingMandate {
    /// Create a default VRBCA mandate
    pub fn default_vrbca() -> Self {
        Self {
            id: [0u8; 32], // Will be set when registered
            version: 1,
            strategy: BasisCaptureStrategy::default(),
            risk_config: RiskConfig::default_vrbca(),
            approved_venues: vec![
                TradingVenue::binance_default(),
                TradingVenue::uniswap_default(),
            ],
            approved_assets: vec![
                AssetConfig::eth_spot(),
                AssetConfig::eth_perp(),
            ],
            operational_params: OperationalParams::default(),
            emergency_conditions: EmergencyConditions::default_vrbca(),
            strategy_code_hash: [0u8; 32], // Will be set from guest binary
        }
    }

    /// Calculate mandate hash for on-chain verification
    pub fn calculate_hash(&self) -> [u8; 32] {
        // In a real implementation, this would use a standardized serialization
        // and cryptographic hash function (e.g., SHA-256)
        let mut hasher_input = Vec::new();
        
        // Add mandate version
        hasher_input.extend_from_slice(&self.version.to_be_bytes());
        
        // Add strategy code hash (most critical)
        hasher_input.extend_from_slice(&self.strategy_code_hash);
        
        // Add key risk parameters
        hasher_input.extend_from_slice(&self.risk_config.max_leverage_bps.to_be_bytes());
        hasher_input.extend_from_slice(&self.risk_config.max_net_delta_bps.to_be_bytes());
        
        // Add emergency conditions
        hasher_input.extend_from_slice(&self.emergency_conditions.max_portfolio_drawdown_bps.to_be_bytes());
        
        // Use a simple hash for demonstration (use SHA-256 in production)
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let mut hasher = DefaultHasher::new();
        hasher_input.hash(&mut hasher);
        let hash_u64 = hasher.finish();
        
        let mut result = [0u8; 32];
        result[0..8].copy_from_slice(&hash_u64.to_be_bytes());
        result
    }

    /// Validate that a mandate is well-formed
    pub fn validate(&self) -> Result<(), MandateValidationError> {
        // Check that we have at least one approved venue
        if self.approved_venues.is_empty() {
            return Err(MandateValidationError::NoApprovedVenues);
        }

        // Check that we have at least one approved asset
        if self.approved_assets.is_empty() {
            return Err(MandateValidationError::NoApprovedAssets);
        }

        // Validate risk parameters are reasonable
        if self.risk_config.max_leverage_bps == 0 {
            return Err(MandateValidationError::InvalidRiskConfig("Zero leverage not allowed".to_string()));
        }

        if self.risk_config.max_leverage_bps > 50000 { // 5x max
            return Err(MandateValidationError::InvalidRiskConfig("Leverage too high".to_string()));
        }

        // Validate strategy code hash is set
        if self.strategy_code_hash == [0u8; 32] {
            return Err(MandateValidationError::MissingCodeHash);
        }

        Ok(())
    }
}

impl TradingVenue {
    /// Create default Binance venue configuration
    pub fn binance_default() -> Self {
        Self {
            name: *b"BINANCE\0\0\0\0\0\0\0\0\0",
            venue_type: VenueType::CentralizedExchange,
            max_position_size_bps: 5000, // 50% max
            risk_params: VenueRiskParams {
                max_leverage_bps: 20000, // 2x
                counterparty_limit: 1000000_000000, // $1M
                min_liquidity: 100000_000000, // $100K
                max_slippage_bps: 100, // 1%
            },
            is_active: true,
        }
    }

    /// Create default Uniswap venue configuration
    pub fn uniswap_default() -> Self {
        Self {
            name: *b"UNISWAP\0\0\0\0\0\0\0\0\0",
            venue_type: VenueType::DecentralizedExchange,
            max_position_size_bps: 3000, // 30% max
            risk_params: VenueRiskParams {
                max_leverage_bps: 10000, // 1x (no leverage on spot DEX)
                counterparty_limit: u128::MAX, // No counterparty risk
                min_liquidity: 50000_000000, // $50K
                max_slippage_bps: 200, // 2%
            },
            is_active: true,
        }
    }
}

impl AssetConfig {
    /// Create ETH spot configuration
    pub fn eth_spot() -> Self {
        Self {
            symbol: *b"ETH\0",
            asset_type: AssetType::Spot,
            max_position_bps: 4000, // 40% max
            min_liquidity_usd: 100000_000000, // $100K
            oracle_config: OracleConfig {
                primary_oracle: OracleSource::Chainlink { 
                    feed_address: [0u8; 20] // ETH/USD feed
                },
                fallback_oracles: vec![
                    OracleSource::UniswapTWAP { 
                        pool_address: [0u8; 20], 
                        period: 3600 // 1 hour TWAP
                    }
                ],
                max_deviation_bps: 300, // 3%
                max_staleness_secs: 3600, // 1 hour
            },
            risk_multiplier: 10000, // 1.0x
        }
    }

    /// Create ETH perpetual configuration
    pub fn eth_perp() -> Self {
        Self {
            symbol: *b"ETHP",
            asset_type: AssetType::Perpetual,
            max_position_bps: 4000, // 40% max
            min_liquidity_usd: 200000_000000, // $200K
            oracle_config: OracleConfig {
                primary_oracle: OracleSource::BinanceAPI { 
                    symbol: *b"ETHUSDT\0\0\0\0\0\0\0\0\0" 
                },
                fallback_oracles: vec![
                    OracleSource::Chainlink { 
                        feed_address: [0u8; 20] 
                    }
                ],
                max_deviation_bps: 200, // 2%
                max_staleness_secs: 300, // 5 minutes
            },
            risk_multiplier: 12000, // 1.2x (higher risk for perps)
        }
    }
}

impl OperationalParams {
    /// Create default operational parameters
    pub fn default() -> Self {
        Self {
            min_execution_interval: 300, // 5 minutes
            max_execution_timeout: 3600, // 1 hour
            gas_limits: GasLimits {
                max_gas_price_urgent: 100_000000000, // 100 gwei
                max_gas_price_normal: 50_000000000,  // 50 gwei
                max_gas_budget_per_epoch: 10_000000000000000, // 0.01 ETH
            },
            profit_taking: ProfitTakingParams {
                target_profit_bps: 1000, // 10%
                stop_loss_bps: 500,      // 5%
                trailing_stop_bps: 200,  // 2%
                min_take_interval_secs: 3600, // 1 hour
            },
            rebalancing: RebalancingParams {
                rebalance_threshold_bps: 200, // 2%
                min_rebalance_interval: 1800,  // 30 minutes
                max_rebalance_size_bps: 2500,  // 25%
            },
        }
    }
}

impl EmergencyConditions {
    /// Create default emergency conditions for VRBCA
    pub fn default_vrbca() -> Self {
        Self {
            max_portfolio_drawdown_bps: 2000, // 20%
            max_position_loss_bps: 1500,      // 15%
            min_market_liquidity: 50000_000000, // $50K
            max_funding_rate_bps: 10000,      // 100% annual
            oracle_failure_conditions: vec![
                OracleFailureCondition::PriceDeviation { max_deviation_bps: 500 },
                OracleFailureCondition::Staleness { max_staleness_secs: 3600 },
                OracleFailureCondition::InsufficientDiversity { min_sources: 2 },
            ],
            venue_halt_conditions: vec![
                VenueHaltCondition {
                    venue: *b"BINANCE\0\0\0\0\0\0\0\0\0",
                    condition: HaltCondition::APIUnavailable { max_downtime_secs: 600 },
                }
            ],
        }
    }
}

/// Errors that can occur during mandate validation
#[derive(Debug)]
pub enum MandateValidationError {
    /// No approved venues specified
    NoApprovedVenues,
    /// No approved assets specified
    NoApprovedAssets,
    /// Invalid risk configuration
    InvalidRiskConfig(String),
    /// Strategy code hash not set
    MissingCodeHash,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_mandate_creation() {
        let mandate = TradingMandate::default_vrbca();
        
        assert_eq!(mandate.version, 1);
        assert!(!mandate.approved_venues.is_empty());
        assert!(!mandate.approved_assets.is_empty());
        assert_eq!(mandate.risk_config.max_leverage_bps, 20000); // 2x
    }

    #[test]
    fn test_mandate_validation() {
        let mut mandate = TradingMandate::default_vrbca();
        mandate.strategy_code_hash = [1u8; 32]; // Set non-zero hash
        
        assert!(mandate.validate().is_ok());
        
        // Test validation failure
        mandate.approved_venues.clear();
        assert!(matches!(mandate.validate(), Err(MandateValidationError::NoApprovedVenues)));
    }

    #[test]
    fn test_mandate_hash_calculation() {
        let mandate = TradingMandate::default_vrbca();
        let hash1 = mandate.calculate_hash();
        let hash2 = mandate.calculate_hash();
        
        assert_eq!(hash1, hash2); // Should be deterministic
        
        // Different mandate should have different hash
        let mut mandate2 = mandate.clone();
        mandate2.version = 2;
        let hash3 = mandate2.calculate_hash();
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_venue_configurations() {
        let binance = TradingVenue::binance_default();
        let uniswap = TradingVenue::uniswap_default();
        
        assert!(binance.is_active);
        assert!(uniswap.is_active);
        
        // Binance should allow higher leverage
        assert!(binance.risk_params.max_leverage_bps > uniswap.risk_params.max_leverage_bps);
    }
}