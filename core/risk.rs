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

//! Risk management module for delta-neutral and leverage constraints.
//!
//! This module implements deterministic risk checks that are enforced in the zkVM
//! to ensure the agent never trades outside its risk mandate. All calculations
//! use integer arithmetic for verifiable computation.

use crate::state::Position;

/// Risk configuration parameters that define the agent's risk limits
#[derive(Clone, Debug)]
pub struct RiskConfig {
    /// Maximum leverage allowed (basis points, e.g., 20000 = 2.0x)
    pub max_leverage_bps: u64,
    /// Maximum net delta as percentage of portfolio (basis points, e.g., 100 = 1%)
    pub max_net_delta_bps: u64,
    /// Maximum drawdown from peak portfolio value (basis points, e.g., 2000 = 20%)
    pub max_drawdown_bps: u64,
    /// Maximum position size per asset (basis points of total capital)
    pub max_position_size_bps: u64,
    /// Minimum capital reserve (basis points, e.g., 1000 = 10% must remain uninvested)
    pub min_capital_reserve_bps: u64,
    /// Maximum correlation between positions (-10000 to 10000, where 10000 = 100% correlation)
    pub max_position_correlation: i64,
}

/// Risk metrics calculated from current portfolio state
#[derive(Clone, Debug)]
pub struct RiskMetrics {
    /// Current leverage (basis points)
    pub leverage_bps: u64,
    /// Net delta exposure (basis points of portfolio)
    pub net_delta_bps: i64,
    /// Current drawdown from peak (basis points)
    pub drawdown_bps: u64,
    /// Value at Risk over 24 hours (basis points)
    pub var_24h_bps: u64,
    /// Portfolio beta to ETH
    pub portfolio_beta: i64,
    /// Concentration risk (largest position as % of portfolio)
    pub concentration_bps: u64,
}

/// Result of risk constraint validation
#[derive(Clone, Debug)]
pub struct RiskValidation {
    /// Whether all constraints are satisfied
    pub is_valid: bool,
    /// Specific constraint violations
    pub violations: Vec<RiskViolation>,
    /// Current risk metrics
    pub metrics: RiskMetrics,
}

/// Types of risk constraint violations
#[derive(Clone, Debug)]
pub enum RiskViolation {
    /// Leverage exceeds maximum allowed
    LeverageExceeded { current: u64, max: u64 },
    /// Net delta exceeds maximum allowed
    DeltaExceeded { current: i64, max: u64 },
    /// Drawdown exceeds maximum allowed
    DrawdownExceeded { current: u64, max: u64 },
    /// Position size exceeds maximum allowed
    PositionSizeExceeded { asset: [u8; 4], current: u64, max: u64 },
    /// Capital reserve below minimum
    InsufficientReserve { current: u64, min: u64 },
    /// Correlation between positions too high
    CorrelationExceeded { current: i64, max: i64 },
}

impl RiskConfig {
    /// Create default risk configuration for VRBCA
    pub fn default_vrbca() -> Self {
        Self {
            max_leverage_bps: 20000,      // 2.0x max leverage
            max_net_delta_bps: 100,       // 1% max net delta
            max_drawdown_bps: 2000,       // 20% max drawdown
            max_position_size_bps: 5000,  // 50% max position size
            min_capital_reserve_bps: 1000, // 10% min cash reserve
            max_position_correlation: 3000, // 30% max correlation
        }
    }

    /// Create conservative risk configuration
    pub fn conservative() -> Self {
        Self {
            max_leverage_bps: 15000,      // 1.5x max leverage
            max_net_delta_bps: 50,        // 0.5% max net delta
            max_drawdown_bps: 1000,       // 10% max drawdown
            max_position_size_bps: 3000,  // 30% max position size
            min_capital_reserve_bps: 2000, // 20% min cash reserve
            max_position_correlation: 2000, // 20% max correlation
        }
    }

    /// Validate current portfolio state against risk constraints
    pub fn validate_portfolio(
        &self,
        positions: &[Position],
        total_capital: u128,
        peak_portfolio_value: u128,
        current_portfolio_value: u128
    ) -> RiskValidation {
        let mut violations = Vec::new();
        
        // Calculate current risk metrics
        let metrics = self.calculate_risk_metrics(
            positions, 
            total_capital, 
            peak_portfolio_value, 
            current_portfolio_value
        );

        // Check leverage constraint
        if metrics.leverage_bps > self.max_leverage_bps {
            violations.push(RiskViolation::LeverageExceeded {
                current: metrics.leverage_bps,
                max: self.max_leverage_bps,
            });
        }

        // Check delta constraint
        let delta_abs = metrics.net_delta_bps.abs() as u64;
        if delta_abs > self.max_net_delta_bps {
            violations.push(RiskViolation::DeltaExceeded {
                current: metrics.net_delta_bps,
                max: self.max_net_delta_bps,
            });
        }

        // Check drawdown constraint
        if metrics.drawdown_bps > self.max_drawdown_bps {
            violations.push(RiskViolation::DrawdownExceeded {
                current: metrics.drawdown_bps,
                max: self.max_drawdown_bps,
            });
        }

        // Check position size constraints
        self.check_position_sizes(positions, total_capital, &mut violations);

        // Check capital reserve
        let deployed_capital = self.calculate_deployed_capital(positions);
        let reserve_bps = ((total_capital - deployed_capital) * 10000) / total_capital;
        if reserve_bps < self.min_capital_reserve_bps as u128 {
            violations.push(RiskViolation::InsufficientReserve {
                current: reserve_bps as u64,
                min: self.min_capital_reserve_bps,
            });
        }

        RiskValidation {
            is_valid: violations.is_empty(),
            violations,
            metrics,
        }
    }

    /// Calculate comprehensive risk metrics for the portfolio
    fn calculate_risk_metrics(
        &self,
        positions: &[Position],
        total_capital: u128,
        peak_portfolio_value: u128,
        current_portfolio_value: u128
    ) -> RiskMetrics {
        // Calculate leverage
        let total_notional = positions.iter()
            .map(|p| p.notional_value_abs())
            .sum::<u128>();
        let leverage_bps = if total_capital > 0 {
            ((total_notional * 10000) / total_capital) as u64
        } else { 0 };

        // Calculate net delta
        let net_delta = positions.iter()
            .map(|p| p.delta_exposure())
            .sum::<i128>();
        let net_delta_bps = if total_capital > 0 {
            ((net_delta.abs() as u128 * 10000) / total_capital) as i64 * net_delta.signum()
        } else { 0 };

        // Calculate drawdown
        let drawdown_bps = if peak_portfolio_value > 0 {
            let drawdown = peak_portfolio_value.saturating_sub(current_portfolio_value);
            ((drawdown * 10000) / peak_portfolio_value) as u64
        } else { 0 };

        // Calculate Value at Risk (simplified)
        let var_24h_bps = self.calculate_var_24h(positions, total_capital);

        // Calculate portfolio beta (simplified to ETH exposure)
        let eth_exposure = positions.iter()
            .filter(|p| p.asset == *b"ETH\0")
            .map(|p| p.delta_exposure())
            .sum::<i128>();
        let portfolio_beta = if total_capital > 0 {
            ((eth_exposure * 10000) / total_capital as i128) as i64
        } else { 0 };

        // Calculate concentration risk
        let largest_position = positions.iter()
            .map(|p| p.notional_value_abs())
            .max()
            .unwrap_or(0);
        let concentration_bps = if total_capital > 0 {
            ((largest_position * 10000) / total_capital) as u64
        } else { 0 };

        RiskMetrics {
            leverage_bps,
            net_delta_bps,
            drawdown_bps,
            var_24h_bps,
            portfolio_beta,
            concentration_bps,
        }
    }

    /// Check individual position size constraints
    fn check_position_sizes(
        &self,
        positions: &[Position],
        total_capital: u128,
        violations: &mut Vec<RiskViolation>
    ) {
        for position in positions {
            let position_size_bps = if total_capital > 0 {
                ((position.notional_value_abs() * 10000) / total_capital) as u64
            } else { 0 };

            if position_size_bps > self.max_position_size_bps {
                violations.push(RiskViolation::PositionSizeExceeded {
                    asset: position.asset,
                    current: position_size_bps,
                    max: self.max_position_size_bps,
                });
            }
        }
    }

    /// Calculate total deployed capital
    fn calculate_deployed_capital(&self, positions: &[Position]) -> u128 {
        positions.iter()
            .map(|p| p.margin_used)
            .sum()
    }

    /// Calculate 24-hour Value at Risk (simplified model)
    fn calculate_var_24h(&self, positions: &[Position], total_capital: u128) -> u64 {
        // Simplified VaR calculation based on position volatilities
        // In production, this would use historical correlations and volatilities
        let total_risk = positions.iter()
            .map(|p| {
                let vol_estimate = 2000; // 20% daily volatility assumption
                let position_value = p.notional_value_abs();
                (position_value * vol_estimate as u128) / 10000
            })
            .sum::<u128>();

        if total_capital > 0 {
            ((total_risk * 10000) / total_capital) as u64
        } else { 0 }
    }
}

/// Delta hedging utilities for maintaining market neutrality
pub struct DeltaHedger {
    /// Target delta (typically 0 for market neutral)
    pub target_delta: i128,
    /// Delta tolerance in basis points
    pub delta_tolerance_bps: u64,
    /// Minimum hedge size to avoid over-hedging small positions
    pub min_hedge_size: u128,
}

impl DeltaHedger {
    /// Create default delta hedger for market-neutral strategy
    pub fn market_neutral() -> Self {
        Self {
            target_delta: 0,
            delta_tolerance_bps: 50, // 0.5% tolerance
            delta_tolerance_bps: 50,
            min_hedge_size: 100_000000, // $100 minimum hedge
        }
    }

    /// Calculate required hedge to achieve target delta
    pub fn calculate_hedge(
        &self,
        positions: &[Position],
        target_portfolio_value: u128
    ) -> Option<HedgeRecommendation> {
        let current_delta = positions.iter()
            .map(|p| p.delta_exposure())
            .sum::<i128>();

        let target_delta_absolute = (self.target_delta * target_portfolio_value as i128) / 10000;
        let delta_difference = current_delta - target_delta_absolute;

        // Check if hedging is needed
        let tolerance_absolute = (self.delta_tolerance_bps as u128 * target_portfolio_value) / 10000;
        if delta_difference.abs() as u128 <= tolerance_absolute {
            return None; // No hedging needed
        }

        // Check minimum hedge size
        if (delta_difference.abs() as u128) < self.min_hedge_size {
            return None; // Too small to hedge
        }

        Some(HedgeRecommendation {
            hedge_size: delta_difference.abs() as u128,
            hedge_direction: if delta_difference > 0 { HedgeDirection::Short } else { HedgeDirection::Long },
            urgency: self.calculate_hedge_urgency(delta_difference, target_portfolio_value),
        })
    }

    /// Calculate urgency of hedge based on delta magnitude
    fn calculate_hedge_urgency(&self, delta_difference: i128, portfolio_value: u128) -> HedgeUrgency {
        let delta_pct = if portfolio_value > 0 {
            (delta_difference.abs() as u128 * 10000) / portfolio_value
        } else { 0 };

        if delta_pct > 500 { // > 5%
            HedgeUrgency::Critical
        } else if delta_pct > 200 { // > 2%
            HedgeUrgency::High
        } else if delta_pct > 100 { // > 1%
            HedgeUrgency::Medium
        } else {
            HedgeUrgency::Low
        }
    }
}

/// Hedging recommendation from delta hedger
#[derive(Clone, Debug)]
pub struct HedgeRecommendation {
    /// Size of hedge in USD (scaled by 1e6)
    pub hedge_size: u128,
    /// Direction of hedge
    pub hedge_direction: HedgeDirection,
    /// Urgency level
    pub urgency: HedgeUrgency,
}

/// Direction for delta hedge
#[derive(Clone, Debug)]
pub enum HedgeDirection {
    /// Long exposure to reduce net short delta
    Long,
    /// Short exposure to reduce net long delta
    Short,
}

/// Urgency level for hedging
#[derive(Clone, Debug)]
pub enum HedgeUrgency {
    /// Immediate hedging required (>5% delta)
    Critical,
    /// Hedging recommended within 1 hour (2-5% delta)
    High,
    /// Hedging recommended within 4 hours (1-2% delta)
    Medium,
    /// Hedging can wait (0.5-1% delta)
    Low,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Position, PositionSide, Venue};

    #[test]
    fn test_risk_config_validation() {
        let config = RiskConfig::default_vrbca();
        let positions = vec![
            Position {
                asset: *b"ETH\0",
                venue: Venue::Binance,
                side: PositionSide::Long,
                size: 1_000000,        // 1 ETH
                entry_price: 3200_000000, // $3200
                current_price: 3200_000000,
                margin_used: 1600_000000, // $1600 margin (2x leverage)
                unrealized_pnl: 0,
            }
        ];

        let total_capital = 10000_000000; // $10,000
        let validation = config.validate_portfolio(
            &positions, 
            total_capital, 
            total_capital, 
            total_capital
        );

        assert!(validation.is_valid); // Should pass with default settings
    }

    #[test]
    fn test_leverage_violation() {
        let config = RiskConfig::conservative();
        let positions = vec![
            Position {
                asset: *b"ETH\0",
                venue: Venue::Binance,
                side: PositionSide::Long,
                size: 5_000000,        // 5 ETH
                entry_price: 3200_000000,
                current_price: 3200_000000,
                margin_used: 8000_000000, // $8000 margin
                unrealized_pnl: 0,
            }
        ];

        let total_capital = 10000_000000; // $10,000
        let validation = config.validate_portfolio(
            &positions, 
            total_capital, 
            total_capital, 
            total_capital
        );

        assert!(!validation.is_valid);
        assert!(matches!(validation.violations[0], RiskViolation::LeverageExceeded { .. }));
    }

    #[test]
    fn test_delta_hedger() {
        let hedger = DeltaHedger::market_neutral();
        let positions = vec![
            Position {
                asset: *b"ETH\0",
                venue: Venue::Uniswap,
                side: PositionSide::Long,
                size: 2_000000,        // 2 ETH long
                entry_price: 3200_000000,
                current_price: 3200_000000,
                margin_used: 6400_000000,
                unrealized_pnl: 0,
            }
        ];

        let portfolio_value = 10000_000000;
        let hedge = hedger.calculate_hedge(&positions, portfolio_value);

        assert!(hedge.is_some()); // Should recommend hedge for unbalanced position
        let recommendation = hedge.unwrap();
        assert!(matches!(recommendation.hedge_direction, HedgeDirection::Short));
    }
}