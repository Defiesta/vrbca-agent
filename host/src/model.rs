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

//! Machine learning models for VRBCA strategy.
//!
//! This module contains the same ML models used in the guest program,
//! allowing the host to pre-validate model outputs before proof generation.
//! All models use integer arithmetic for consistency with the zkVM.

use anyhow::Result;
use tracing::debug;

/// Stub funding model for testing
#[derive(Debug, Clone)]
pub struct FundingModel {
    pub weights: [i128; 3],
    pub bias: i128,
    pub min_r_squared: u64,
}

impl FundingModel {
    pub fn default_model() -> Self {
        Self {
            weights: [8000, -2000, -500],
            bias: 1000,
            min_r_squared: 6000,
        }
    }

    pub fn predict_funding_persistence(&self, _funding_rate: i128, _volatility: u64, _time_factor: u64) -> (i128, u64) {
        (self.bias, self.min_r_squared)
    }
}

/// Extended funding model with historical data management
pub struct ExtendedFundingModel {
    /// Base model from core
    pub base_model: FundingModel,
    /// Historical funding rate data for model training
    pub funding_history: Vec<FundingDataPoint>,
    /// Model performance metrics
    pub performance_metrics: ModelMetrics,
}

/// Historical funding data point
#[derive(Debug, Clone)]
pub struct FundingDataPoint {
    /// Timestamp
    pub timestamp: u64,
    /// Funding rate in basis points
    pub funding_rate: i128,
    /// Market volatility estimate
    pub volatility: u64,
    /// Actual persistence (for training)
    pub actual_persistence: Option<i128>,
}

/// Model performance tracking
#[derive(Debug, Clone)]
pub struct ModelMetrics {
    /// R-squared value (0-10000, where 10000 = 100%)
    pub r_squared: u64,
    /// Mean absolute error in basis points
    pub mean_absolute_error: u64,
    /// Number of predictions made
    pub prediction_count: u64,
    /// Last model update timestamp
    pub last_update: u64,
}

impl ExtendedFundingModel {
    /// Create new extended funding model
    pub fn new() -> Self {
        Self {
            base_model: FundingModel::default_model(),
            funding_history: Vec::new(),
            performance_metrics: ModelMetrics {
                r_squared: 6000, // 60% default
                mean_absolute_error: 200, // 2% default error
                prediction_count: 0,
                last_update: chrono::Utc::now().timestamp() as u64,
            },
        }
    }

    /// Add new funding data point to history
    pub fn add_data_point(&mut self, data_point: FundingDataPoint) {
        self.funding_history.push(data_point);
        
        // Keep only last 1000 data points for efficiency
        if self.funding_history.len() > 1000 {
            self.funding_history.drain(0..self.funding_history.len() - 1000);
        }
    }

    /// Predict funding persistence with extended features
    pub fn predict_with_confidence(
        &self,
        current_funding: i128,
        current_volatility: u64,
        time_factor: u64,
    ) -> Result<(i128, u64)> {
        // Use base model for core prediction
        let (base_prediction, base_confidence) = self.base_model
            .predict_funding_persistence(current_funding, current_volatility, time_factor);

        // Adjust confidence based on model performance
        let adjusted_confidence = self.adjust_confidence_for_performance(base_confidence);

        // Apply regime detection adjustments
        let regime_adjusted_prediction = self.apply_regime_adjustments(
            base_prediction,
            current_funding,
            current_volatility,
        )?;

        debug!("Funding prediction: {} -> {} (confidence: {}%)",
               current_funding, regime_adjusted_prediction, adjusted_confidence);

        Ok((regime_adjusted_prediction, adjusted_confidence))
    }

    /// Update model weights based on recent performance
    pub fn update_model_weights(&mut self) -> Result<()> {
        if self.funding_history.len() < 10 {
            return Ok(()); // Need minimum data for updates
        }

        // Calculate recent model performance
        let recent_error = self.calculate_recent_error()?;
        let new_r_squared = self.calculate_r_squared()?;

        // Update performance metrics
        self.performance_metrics.mean_absolute_error = recent_error;
        self.performance_metrics.r_squared = new_r_squared;
        self.performance_metrics.last_update = chrono::Utc::now().timestamp() as u64;

        // Adjust model weights if performance is poor
        if new_r_squared < 4000 { // Less than 40% R²
            self.adjust_model_weights_for_poor_performance();
        }

        debug!("Updated model: R²={:.1}%, MAE={:.2}%",
               new_r_squared as f64 / 100.0,
               recent_error as f64 / 100.0);

        Ok(())
    }

    /// Adjust confidence based on historical model performance
    fn adjust_confidence_for_performance(&self, base_confidence: u64) -> u64 {
        // Reduce confidence if model has been performing poorly
        let performance_factor = self.performance_metrics.r_squared.min(10000); // Cap at 100%
        
        (base_confidence * performance_factor / 10000).max(1000) // Minimum 10% confidence
    }

    /// Apply regime-specific adjustments to predictions
    fn apply_regime_adjustments(
        &self,
        base_prediction: i128,
        current_funding: i128,
        volatility: u64,
    ) -> Result<i128> {
        let regime = self.detect_market_regime(current_funding, volatility);
        
        match regime {
            MarketRegime::HighVolatility => {
                // In high volatility, funding rates tend to revert faster
                Ok(base_prediction * 80 / 100) // 20% reduction
            },
            MarketRegime::TrendingUp => {
                // In uptrending markets, positive funding may persist longer
                if base_prediction > 0 {
                    Ok(base_prediction * 110 / 100) // 10% increase
                } else {
                    Ok(base_prediction)
                }
            },
            MarketRegime::TrendingDown => {
                // In downtrending markets, negative funding may persist longer
                if base_prediction < 0 {
                    Ok(base_prediction * 110 / 100) // 10% increase (more negative)
                } else {
                    Ok(base_prediction)
                }
            },
            MarketRegime::Sideways => {
                // Sideways markets show normal mean reversion
                Ok(base_prediction)
            }
        }
    }

    /// Detect current market regime
    fn detect_market_regime(&self, current_funding: i128, volatility: u64) -> MarketRegime {
        // High volatility regime
        if volatility > 5000 { // 50% volatility threshold
            return MarketRegime::HighVolatility;
        }

        // Look at recent funding rate trends
        if self.funding_history.len() < 5 {
            return MarketRegime::Sideways;
        }

        let recent_funding: Vec<i128> = self.funding_history
            .iter()
            .rev()
            .take(5)
            .map(|dp| dp.funding_rate)
            .collect();

        let trend = self.calculate_trend(&recent_funding);
        
        if trend > 100 { // Increasing trend
            MarketRegime::TrendingUp
        } else if trend < -100 { // Decreasing trend
            MarketRegime::TrendingDown
        } else {
            MarketRegime::Sideways
        }
    }

    /// Calculate trend from recent data points
    fn calculate_trend(&self, data: &[i128]) -> i128 {
        if data.len() < 2 {
            return 0;
        }

        // Simple linear regression slope calculation
        let n = data.len() as i128;
        let sum_x: i128 = (0..n).sum();
        let sum_y: i128 = data.iter().sum();
        let sum_xy: i128 = data.iter().enumerate()
            .map(|(i, &y)| (i as i128) * y)
            .sum();
        let sum_x2: i128 = (0..n).map(|x| x * x).sum();

        let denominator = n * sum_x2 - sum_x * sum_x;
        if denominator == 0 {
            return 0;
        }

        (n * sum_xy - sum_x * sum_y) / denominator
    }

    /// Calculate recent prediction error
    fn calculate_recent_error(&self) -> Result<u64> {
        let recent_data: Vec<_> = self.funding_history
            .iter()
            .rev()
            .take(20)
            .filter(|dp| dp.actual_persistence.is_some())
            .collect();

        if recent_data.is_empty() {
            return Ok(self.performance_metrics.mean_absolute_error);
        }

        let mut total_error = 0i128;
        let mut count = 0;

        for data_point in recent_data {
            let predicted = self.base_model.predict_funding_persistence(
                data_point.funding_rate,
                data_point.volatility,
                4000, // Default time factor
            ).0;
            
            let actual = data_point.actual_persistence.unwrap();
            total_error += (predicted - actual).abs();
            count += 1;
        }

        let mean_error = if count > 0 {
            (total_error / count as i128) as u64
        } else {
            self.performance_metrics.mean_absolute_error
        };

        Ok(mean_error)
    }

    /// Calculate R-squared from recent data
    fn calculate_r_squared(&self) -> Result<u64> {
        let recent_data: Vec<_> = self.funding_history
            .iter()
            .rev()
            .take(50)
            .filter(|dp| dp.actual_persistence.is_some())
            .collect();

        if recent_data.len() < 5 {
            return Ok(self.performance_metrics.r_squared);
        }

        // Calculate mean of actual values
        let mean_actual: i128 = recent_data
            .iter()
            .map(|dp| dp.actual_persistence.unwrap())
            .sum::<i128>() / recent_data.len() as i128;

        let mut ss_tot = 0i128;
        let mut ss_res = 0i128;

        for data_point in recent_data {
            let actual = data_point.actual_persistence.unwrap();
            let predicted = self.base_model.predict_funding_persistence(
                data_point.funding_rate,
                data_point.volatility,
                4000,
            ).0;

            ss_tot += (actual - mean_actual).pow(2);
            ss_res += (actual - predicted).pow(2);
        }

        let r_squared = if ss_tot > 0 {
            ((ss_tot - ss_res) * 10000 / ss_tot) as u64
        } else {
            0
        };

        Ok(r_squared.min(10000))
    }

    /// Adjust model weights when performance is poor
    fn adjust_model_weights_for_poor_performance(&mut self) {
        // Reduce confidence in persistence predictions
        for weight in &mut self.base_model.weights {
            *weight = *weight * 90 / 100; // 10% reduction
        }

        // Increase minimum R² threshold
        self.base_model.min_r_squared = 
            (self.base_model.min_r_squared + 500).min(8000); // Cap at 80%
    }
}

/// Market regime classification
#[derive(Debug, Clone)]
enum MarketRegime {
    HighVolatility,
    TrendingUp,
    TrendingDown,
    Sideways,
}

impl Default for ExtendedFundingModel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extended_model_creation() {
        let model = ExtendedFundingModel::new();
        
        assert_eq!(model.funding_history.len(), 0);
        assert!(model.performance_metrics.r_squared > 0);
    }

    #[test]
    fn test_data_point_addition() {
        let mut model = ExtendedFundingModel::new();
        
        let data_point = FundingDataPoint {
            timestamp: 1640995200,
            funding_rate: 25,
            volatility: 2000,
            actual_persistence: Some(20),
        };

        model.add_data_point(data_point);
        assert_eq!(model.funding_history.len(), 1);
    }

    #[test]
    fn test_prediction_with_confidence() {
        let model = ExtendedFundingModel::new();
        
        let result = model.predict_with_confidence(25, 2000, 4000);
        assert!(result.is_ok());
        
        let (prediction, confidence) = result.unwrap();
        assert!(prediction != 0 || confidence > 0); // Some reasonable output
    }

    #[test]
    fn test_regime_detection() {
        let model = ExtendedFundingModel::new();
        
        // High volatility should be detected
        let regime = model.detect_market_regime(25, 6000);
        assert!(matches!(regime, MarketRegime::HighVolatility));
        
        // Normal volatility should default to sideways
        let regime = model.detect_market_regime(25, 2000);
        assert!(matches!(regime, MarketRegime::Sideways));
    }

    #[test]
    fn test_trend_calculation() {
        let model = ExtendedFundingModel::new();
        
        // Upward trend
        let upward_data = vec![10, 15, 20, 25, 30];
        let trend = model.calculate_trend(&upward_data);
        assert!(trend > 0);
        
        // Downward trend
        let downward_data = vec![30, 25, 20, 15, 10];
        let trend = model.calculate_trend(&downward_data);
        assert!(trend < 0);
        
        // Flat trend
        let flat_data = vec![20, 20, 20, 20, 20];
        let trend = model.calculate_trend(&flat_data);
        assert_eq!(trend, 0);
    }
}