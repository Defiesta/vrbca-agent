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

//! State module for position tracking and PnL calculations.
//!
//! This module handles the agent's position state, including individual positions,
//! portfolio-level metrics, and state transitions. All calculations use integer
//! arithmetic for deterministic execution in the zkVM.

/// A trading position held by the agent
#[derive(Clone, Debug)]
pub struct Position {
    /// Asset symbol (e.g., "ETH\0", "ETHP" for ETH perpetual)
    pub asset: [u8; 4],
    /// Trading venue where position is held
    pub venue: Venue,
    /// Position side (long or short)
    pub side: PositionSide,
    /// Position size (in asset units, scaled by 1e6 for precision)
    pub size: u128,
    /// Entry price (USD, scaled by 1e6)
    pub entry_price: u128,
    /// Current market price (USD, scaled by 1e6)
    pub current_price: u128,
    /// Margin used for this position (USD, scaled by 1e6)
    pub margin_used: u128,
    /// Unrealized PnL (USD, scaled by 1e6, can be negative)
    pub unrealized_pnl: i128,
}

/// Trading venue enumeration
#[derive(Clone, Debug, PartialEq)]
pub enum Venue {
    /// Binance centralized exchange
    Binance,
    /// Uniswap decentralized exchange
    Uniswap,
    /// Compound lending protocol
    Compound,
    /// Aave lending protocol
    Aave,
    /// Custom venue (for testing or future expansion)
    Custom([u8; 16]),
}

/// Position side enumeration
#[derive(Clone, Debug, PartialEq)]
pub enum PositionSide {
    /// Long position (buy asset, profit from price increase)
    Long,
    /// Short position (sell asset, profit from price decrease)
    Short,
}

/// Complete portfolio state at a given epoch
#[derive(Clone, Debug)]
pub struct PortfolioState {
    /// Epoch identifier
    pub epoch_id: u64,
    /// All active positions
    pub positions: Vec<Position>,
    /// Available cash reserves (USD, scaled by 1e6)
    pub cash_balance: u128,
    /// Total portfolio value including unrealized PnL (USD, scaled by 1e6)
    pub total_portfolio_value: u128,
    /// Peak portfolio value seen (for drawdown calculation)
    pub peak_portfolio_value: u128,
    /// Total margin used across all positions
    pub total_margin_used: u128,
    /// Cumulative realized PnL (USD, scaled by 1e6)
    pub cumulative_realized_pnl: i128,
    /// Timestamp of this state
    pub timestamp: u64,
    /// Hash of previous state (for chain of custody)
    pub prev_state_hash: [u8; 32],
}

/// Execution report from a trading venue
#[derive(Clone, Debug)]
pub struct ExecutionReport {
    /// Venue where execution occurred
    pub venue: Venue,
    /// Asset traded
    pub asset: [u8; 4],
    /// Trade side
    pub side: PositionSide,
    /// Quantity traded (scaled by 1e6)
    pub quantity: u128,
    /// Execution price (USD, scaled by 1e6)
    pub price: u128,
    /// Timestamp of execution
    pub timestamp: u64,
    /// Unique order identifier hash
    pub order_id_hash: [u8; 32],
    /// Venue signature for verification (ECDSA signature)
    pub venue_signature: [u8; 65],
    /// Fee paid for this trade (USD, scaled by 1e6)
    pub fee_paid: u128,
}

impl Position {
    /// Calculate absolute notional value of the position
    pub fn notional_value_abs(&self) -> u128 {
        (self.size * self.current_price) / 1_000000 // Adjust for scaling
    }

    /// Calculate delta exposure (directional market exposure)
    pub fn delta_exposure(&self) -> i128 {
        let notional = self.notional_value_abs() as i128;
        match self.side {
            PositionSide::Long => notional,
            PositionSide::Short => -notional,
        }
    }

    /// Update position with new market price
    pub fn update_price(&mut self, new_price: u128) {
        self.current_price = new_price;
        self.unrealized_pnl = self.calculate_unrealized_pnl();
    }

    /// Calculate unrealized PnL for this position
    pub fn calculate_unrealized_pnl(&self) -> i128 {
        let price_diff = match self.side {
            PositionSide::Long => self.current_price as i128 - self.entry_price as i128,
            PositionSide::Short => self.entry_price as i128 - self.current_price as i128,
        };
        
        (price_diff * self.size as i128) / 1_000000 // Adjust for scaling
    }

    /// Calculate return on margin for this position (basis points)
    pub fn return_on_margin_bps(&self) -> i128 {
        if self.margin_used == 0 {
            return 0;
        }
        (self.unrealized_pnl * 10000) / self.margin_used as i128
    }

    /// Check if position should be liquidated based on margin
    pub fn is_liquidatable(&self, liquidation_threshold_bps: u64) -> bool {
        let return_bps = self.return_on_margin_bps();
        return_bps <= -(liquidation_threshold_bps as i128)
    }

    /// Calculate position leverage (notional / margin used)
    pub fn leverage(&self) -> u128 {
        if self.margin_used == 0 {
            return 0;
        }
        (self.notional_value_abs() * 10000) / self.margin_used // Returns leverage in basis points
    }
}

impl PortfolioState {
    /// Create new portfolio state
    pub fn new(
        epoch_id: u64, 
        initial_capital: u128, 
        timestamp: u64,
        prev_state_hash: [u8; 32]
    ) -> Self {
        Self {
            epoch_id,
            positions: Vec::new(),
            cash_balance: initial_capital,
            total_portfolio_value: initial_capital,
            peak_portfolio_value: initial_capital,
            total_margin_used: 0,
            cumulative_realized_pnl: 0,
            timestamp,
            prev_state_hash,
        }
    }

    /// Apply execution report to update portfolio state
    pub fn apply_execution(&mut self, execution: &ExecutionReport) -> Result<(), StateError> {
        // Validate execution report
        self.validate_execution(execution)?;

        // Find or create position
        let position_index = self.find_position_index(&execution.asset, execution.venue);
        
        match position_index {
            Some(index) => {
                // Update existing position
                self.update_existing_position(index, execution)?;
            },
            None => {
                // Create new position
                self.create_new_position(execution)?;
            }
        }

        // Update cash balance (subtract trade cost + fees)
        let trade_cost = (execution.quantity * execution.price) / 1_000000;
        let total_cost = trade_cost + execution.fee_paid;
        
        match execution.side {
            PositionSide::Long => {
                // Buying: reduce cash
                if self.cash_balance < total_cost {
                    return Err(StateError::InsufficientCash);
                }
                self.cash_balance -= total_cost;
            },
            PositionSide::Short => {
                // Selling: increase cash (minus fees)
                self.cash_balance = self.cash_balance + trade_cost - execution.fee_paid;
            }
        }

        // Recalculate portfolio metrics
        self.recalculate_portfolio_metrics();
        
        Ok(())
    }

    /// Calculate total net delta across all positions
    pub fn net_delta(&self) -> i128 {
        self.positions.iter()
            .map(|p| p.delta_exposure())
            .sum()
    }

    /// Calculate total leverage across all positions
    pub fn total_leverage(&self) -> u128 {
        let total_notional: u128 = self.positions.iter()
            .map(|p| p.notional_value_abs())
            .sum();
        
        if self.total_portfolio_value == 0 {
            return 0;
        }
        
        (total_notional * 10000) / self.total_portfolio_value // Basis points
    }

    /// Calculate current drawdown from peak
    pub fn drawdown_bps(&self) -> u64 {
        if self.peak_portfolio_value == 0 {
            return 0;
        }
        
        let drawdown = self.peak_portfolio_value.saturating_sub(self.total_portfolio_value);
        ((drawdown * 10000) / self.peak_portfolio_value) as u64
    }

    /// Calculate portfolio-wide unrealized PnL
    pub fn total_unrealized_pnl(&self) -> i128 {
        self.positions.iter()
            .map(|p| p.unrealized_pnl)
            .sum()
    }

    /// Get positions by venue
    pub fn positions_by_venue(&self, venue: Venue) -> Vec<&Position> {
        self.positions.iter()
            .filter(|p| p.venue == venue)
            .collect()
    }

    /// Calculate state hash for verification
    pub fn calculate_state_hash(&self) -> [u8; 32] {
        // In production, this would use a standardized serialization
        // and cryptographic hash function (e.g., SHA-256)
        let mut hash_input = Vec::new();
        
        // Add epoch and timestamp
        hash_input.extend_from_slice(&self.epoch_id.to_be_bytes());
        hash_input.extend_from_slice(&self.timestamp.to_be_bytes());
        
        // Add portfolio value and cash balance
        hash_input.extend_from_slice(&self.total_portfolio_value.to_be_bytes());
        hash_input.extend_from_slice(&self.cash_balance.to_be_bytes());
        
        // Add position hashes
        for position in &self.positions {
            hash_input.extend_from_slice(&position.asset);
            hash_input.extend_from_slice(&position.size.to_be_bytes());
            hash_input.extend_from_slice(&position.entry_price.to_be_bytes());
        }
        
        // Add previous state hash
        hash_input.extend_from_slice(&self.prev_state_hash);
        
        // Simple hash for demonstration (use SHA-256 in production)
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let mut hasher = DefaultHasher::new();
        hash_input.hash(&mut hasher);
        let hash_u64 = hasher.finish();
        
        let mut result = [0u8; 32];
        result[0..8].copy_from_slice(&hash_u64.to_be_bytes());
        result
    }

    /// Validate execution report
    fn validate_execution(&self, execution: &ExecutionReport) -> Result<(), StateError> {
        // Check quantity is positive
        if execution.quantity == 0 {
            return Err(StateError::InvalidQuantity);
        }

        // Check price is reasonable (not zero, not extremely high)
        if execution.price == 0 || execution.price > 1000000_000000 { // $1M max price
            return Err(StateError::InvalidPrice);
        }

        // Check timestamp is reasonable (within last hour to next hour)
        let current_time = self.timestamp;
        if execution.timestamp < current_time.saturating_sub(3600) || 
           execution.timestamp > current_time + 3600 {
            return Err(StateError::InvalidTimestamp);
        }

        Ok(())
    }

    /// Find position index for given asset and venue
    fn find_position_index(&self, asset: &[u8; 4], venue: Venue) -> Option<usize> {
        self.positions.iter()
            .position(|p| p.asset == *asset && p.venue == venue)
    }

    /// Update existing position with execution
    fn update_existing_position(&mut self, index: usize, execution: &ExecutionReport) -> Result<(), StateError> {
        let position = &mut self.positions[index];
        
        match (position.side, execution.side) {
            (PositionSide::Long, PositionSide::Long) | (PositionSide::Short, PositionSide::Short) => {
                // Increasing position size
                let old_notional = position.size * position.entry_price;
                let new_notional = execution.quantity * execution.price;
                let total_size = position.size + execution.quantity;
                
                if total_size > 0 {
                    position.entry_price = (old_notional + new_notional) / total_size;
                }
                position.size = total_size;
            },
            (PositionSide::Long, PositionSide::Short) | (PositionSide::Short, PositionSide::Long) => {
                // Reducing or reversing position
                if execution.quantity >= position.size {
                    // Close position or reverse
                    let remaining_quantity = execution.quantity - position.size;
                    
                    if remaining_quantity > 0 {
                        // Reverse position
                        position.side = execution.side;
                        position.size = remaining_quantity;
                        position.entry_price = execution.price;
                    } else {
                        // Close position - remove from vector
                        self.positions.remove(index);
                        return Ok(());
                    }
                } else {
                    // Partial close
                    position.size -= execution.quantity;
                }
            }
        }
        
        // Update current price and margin
        position.current_price = execution.price;
        position.margin_used = position.notional_value_abs() / (position.leverage() / 10000).max(10000);
        position.unrealized_pnl = position.calculate_unrealized_pnl();
        
        Ok(())
    }

    /// Create new position from execution
    fn create_new_position(&mut self, execution: &ExecutionReport) -> Result<(), StateError> {
        let notional_value = (execution.quantity * execution.price) / 1_000000;
        let margin_estimate = notional_value / 2; // Assume 2x leverage initially
        
        let position = Position {
            asset: execution.asset,
            venue: execution.venue,
            side: execution.side,
            size: execution.quantity,
            entry_price: execution.price,
            current_price: execution.price,
            margin_used: margin_estimate,
            unrealized_pnl: 0, // New position has zero unrealized PnL
        };
        
        self.positions.push(position);
        Ok(())
    }

    /// Recalculate portfolio-level metrics
    fn recalculate_portfolio_metrics(&mut self) {
        // Update total margin used
        self.total_margin_used = self.positions.iter()
            .map(|p| p.margin_used)
            .sum();

        // Update unrealized PnL for all positions
        for position in &mut self.positions {
            position.unrealized_pnl = position.calculate_unrealized_pnl();
        }

        // Calculate total portfolio value
        let total_unrealized_pnl = self.total_unrealized_pnl();
        self.total_portfolio_value = (self.cash_balance as i128 + total_unrealized_pnl).max(0) as u128;

        // Update peak if necessary
        if self.total_portfolio_value > self.peak_portfolio_value {
            self.peak_portfolio_value = self.total_portfolio_value;
        }
    }
}

/// Errors that can occur during state operations
#[derive(Debug)]
pub enum StateError {
    /// Insufficient cash for trade
    InsufficientCash,
    /// Invalid quantity (zero or negative)
    InvalidQuantity,
    /// Invalid price
    InvalidPrice,
    /// Invalid timestamp
    InvalidTimestamp,
    /// Position not found
    PositionNotFound,
    /// Calculation overflow
    CalculationOverflow,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_position_calculations() {
        let mut position = Position {
            asset: *b"ETH\0",
            venue: Venue::Binance,
            side: PositionSide::Long,
            size: 2_000000,        // 2 ETH
            entry_price: 3200_000000, // $3200
            current_price: 3200_000000,
            margin_used: 3200_000000, // $3200 margin (2x leverage)
            unrealized_pnl: 0,
        };

        // Test notional value calculation
        assert_eq!(position.notional_value_abs(), 6400_000000); // $6400

        // Test delta exposure
        assert_eq!(position.delta_exposure(), 6400_000000);

        // Test price update and PnL calculation
        position.update_price(3300_000000); // Price goes to $3300
        assert!(position.unrealized_pnl > 0); // Should be profitable

        // Test leverage calculation
        let leverage = position.leverage();
        assert_eq!(leverage, 20000); // 2x leverage in basis points
    }

    #[test]
    fn test_portfolio_state_execution() {
        let mut portfolio = PortfolioState::new(
            1, 
            10000_000000, // $10,000 initial capital
            1640995200,   // Jan 1, 2022
            [0u8; 32]
        );

        let execution = ExecutionReport {
            venue: Venue::Binance,
            asset: *b"ETH\0",
            side: PositionSide::Long,
            quantity: 1_000000,        // 1 ETH
            price: 3200_000000,        // $3200
            timestamp: 1640995200,
            order_id_hash: [1u8; 32],
            venue_signature: [0u8; 65],
            fee_paid: 10_000000,       // $10 fee
        };

        assert!(portfolio.apply_execution(&execution).is_ok());
        
        // Check portfolio state after execution
        assert_eq!(portfolio.positions.len(), 1);
        assert!(portfolio.cash_balance < 10000_000000); // Cash reduced by trade + fees
        
        // Test net delta calculation
        let delta = portfolio.net_delta();
        assert!(delta > 0); // Should have positive delta from long position
    }

    #[test]
    fn test_position_side_matching() {
        assert_eq!(PositionSide::Long, PositionSide::Long);
        assert_ne!(PositionSide::Long, PositionSide::Short);
    }

    #[test]
    fn test_venue_matching() {
        assert_eq!(Venue::Binance, Venue::Binance);
        assert_ne!(Venue::Binance, Venue::Uniswap);
    }

    #[test]
    fn test_state_hash_calculation() {
        let portfolio = PortfolioState::new(1, 10000_000000, 1640995200, [0u8; 32]);
        let hash1 = portfolio.calculate_state_hash();
        let hash2 = portfolio.calculate_state_hash();
        
        assert_eq!(hash1, hash2); // Should be deterministic
    }

    #[test]
    fn test_drawdown_calculation() {
        let mut portfolio = PortfolioState::new(1, 10000_000000, 1640995200, [0u8; 32]);
        portfolio.peak_portfolio_value = 12000_000000; // Peak at $12,000
        portfolio.total_portfolio_value = 9600_000000;  // Current at $9,600
        
        let drawdown = portfolio.drawdown_bps();
        assert_eq!(drawdown, 2000); // 20% drawdown
    }
}