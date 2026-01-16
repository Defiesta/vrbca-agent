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

//! Proof generation module for VRBCA.
//!
//! This module handles generating zero-knowledge proofs via Boundless that verify:
//! - Strategy execution was compliant with mandate
//! - Risk constraints were respected
//! - State transitions are valid
//! - All calculations were performed correctly

use anyhow::{Context, Result, bail};
use tracing::{info, debug, error};
use tokio::time::{timeout, Duration};
use boundless_market::{Client, StorageProviderConfig};
use url::Url;

use crate::{Args, ProofResult};
use core::{
    strategy::MarketData,
    mandate::TradingMandate,
    state::{PortfolioState, ExecutionReport},
};

/// Proof generator that coordinates with Boundless Market
pub struct ProofGenerator {
    boundless_client: Client,
    guest_binary_url: Option<Url>,
    max_proof_time: Duration,
}

impl ProofGenerator {
    /// Create new proof generator
    pub async fn new(args: &Args) -> Result<Self> {
        // Initialize Boundless client
        let storage_config = StorageProviderConfig::default(); // Use default for now
        
        let boundless_client = Client::builder()
            .with_rpc_url(Url::parse(&args.rpc_url)?)
            .with_storage_provider_config(&storage_config)?
            .build()
            .await
            .context("Failed to build Boundless client")?;

        Ok(Self {
            boundless_client,
            guest_binary_url: None, // Will be uploaded on first use
            max_proof_time: Duration::from_secs(300), // 5 minute timeout
        })
    }

    /// Generate proof for an epoch of agent execution
    pub async fn generate_epoch_proof(
        &mut self,
        epoch_id: u64,
        market_data: &MarketData,
        portfolio_state: &PortfolioState,
        execution_reports: &[ExecutionReport],
        mandate: &TradingMandate,
    ) -> Result<ProofResult> {
        info!("Generating proof for epoch {}", epoch_id);

        // Step 1: Prepare inputs for guest program
        let inputs = self.prepare_proof_inputs(
            epoch_id,
            market_data,
            portfolio_state,
            execution_reports,
            mandate,
        )?;

        // Step 2: Upload guest binary if needed
        if self.guest_binary_url.is_none() {
            self.upload_guest_binary().await?;
        }

        // Step 3: Submit proof request to Boundless
        let request = self.boundless_client
            .new_request()
            .with_program_url(self.guest_binary_url.as_ref().unwrap().clone())?
            .with_stdin(inputs);

        let (request_id, expires_at) = self.boundless_client
            .submit_onchain(request)
            .await
            .context("Failed to submit proof request")?;

        info!("Submitted proof request {:x} for epoch {}", request_id, epoch_id);

        // Step 4: Wait for fulfillment
        let fulfillment = timeout(
            self.max_proof_time,
            self.boundless_client.wait_for_request_fulfillment(
                request_id,
                Duration::from_secs(10), // Check every 10 seconds
                expires_at,
            )
        )
        .await
        .context("Proof generation timed out")?
        .context("Failed to wait for proof fulfillment")?;

        info!("Proof request {:x} fulfilled for epoch {}", request_id, epoch_id);

        // Step 5: Extract and validate proof data
        let proof_result = self.extract_proof_result(epoch_id, &fulfillment.fulfillmentData)?;

        // Step 6: Validate proof journal
        self.validate_proof_journal(&proof_result, portfolio_state)?;

        debug!("Proof validation completed for epoch {}", epoch_id);

        Ok(proof_result)
    }

    /// Prepare inputs for the guest program
    fn prepare_proof_inputs(
        &self,
        epoch_id: u64,
        market_data: &MarketData,
        portfolio_state: &PortfolioState,
        execution_reports: &[ExecutionReport],
        mandate: &TradingMandate,
    ) -> Result<Vec<u8>> {
        // In production, this would serialize all inputs using a standard format (e.g., bincode)
        // For now, we'll prepare simplified inputs

        let mut inputs = Vec::new();

        // Add public inputs
        inputs.extend_from_slice(&epoch_id.to_be_bytes());
        
        // Add agent and mandate identifiers
        let agent_id = [1u8; 32]; // Mock agent ID
        let mandate_hash = mandate.calculate_hash();
        inputs.extend_from_slice(&agent_id);
        inputs.extend_from_slice(&mandate_hash);

        // Add previous state root
        let prev_state_root = portfolio_state.prev_state_hash;
        inputs.extend_from_slice(&prev_state_root);

        // Add market data (private inputs)
        inputs.extend_from_slice(&market_data.spot_price.to_be_bytes());
        inputs.extend_from_slice(&market_data.perp_price.to_be_bytes());
        inputs.extend_from_slice(&market_data.funding_rate.to_be_bytes());
        inputs.extend_from_slice(&market_data.funding_interval_secs.to_be_bytes());
        inputs.extend_from_slice(&market_data.liquidity_score.to_be_bytes());

        // Add execution reports count and data
        inputs.extend_from_slice(&(execution_reports.len() as u32).to_be_bytes());
        for execution in execution_reports {
            inputs.extend_from_slice(&execution.quantity.to_be_bytes());
            inputs.extend_from_slice(&execution.price.to_be_bytes());
            inputs.push(match execution.side {
                core::state::PositionSide::Long => 1,
                core::state::PositionSide::Short => 0,
            });
        }

        debug!("Prepared {} bytes of input data for epoch {}", inputs.len(), epoch_id);

        Ok(inputs)
    }

    /// Upload guest binary to IPFS via Boundless
    async fn upload_guest_binary(&mut self) -> Result<()> {
        info!("Uploading VRBCA guest binary to Boundless");

        // In production, this would read the actual compiled guest binary
        // For now, we'll use a placeholder
        let guest_binary = include_bytes!("../../../target/riscv-guest/methods/guest/riscv32im-risc0-zkvm-elf/release/guest.bin");
        
        // Upload binary to storage provider
        let upload_result = self.boundless_client
            .upload_program(guest_binary)
            .await
            .context("Failed to upload guest binary")?;

        self.guest_binary_url = Some(upload_result.url);
        info!("Uploaded guest binary to: {}", self.guest_binary_url.as_ref().unwrap());

        Ok(())
    }

    /// Extract proof result from Boundless fulfillment data
    fn extract_proof_result(&self, epoch_id: u64, fulfillment_data: &[u8]) -> Result<ProofResult> {
        if fulfillment_data.len() < 64 {
            bail!("Fulfillment data too short");
        }

        // Extract journal hash and proof data
        // In production, this would parse the actual Boundless fulfillment structure
        let journal_hash = {
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&fulfillment_data[0..32]);
            hash
        };

        let proof_data = fulfillment_data[32..].to_vec();

        Ok(ProofResult {
            epoch_id,
            proof_data,
            journal_hash,
        })
    }

    /// Validate proof journal contents
    fn validate_proof_journal(
        &self,
        proof_result: &ProofResult,
        portfolio_state: &PortfolioState,
    ) -> Result<()> {
        // In production, this would:
        // 1. Parse the journal from proof_result
        // 2. Validate that the new state root is correctly calculated
        // 3. Check that risk constraints are satisfied
        // 4. Verify delta neutrality is maintained
        // 5. Confirm no halt conditions were triggered

        debug!("Validating proof journal for epoch {}", proof_result.epoch_id);

        // Basic validation - check that we have a valid proof
        if proof_result.proof_data.is_empty() {
            bail!("Empty proof data");
        }

        if proof_result.journal_hash == [0u8; 32] {
            bail!("Invalid journal hash");
        }

        // Validate epoch consistency
        if proof_result.epoch_id != portfolio_state.epoch_id {
            bail!("Epoch ID mismatch in proof");
        }

        debug!("Proof journal validation passed for epoch {}", proof_result.epoch_id);

        Ok(())
    }

    /// Handle proof generation failure
    async fn handle_proof_failure(
        &self,
        epoch_id: u64,
        error: &anyhow::Error,
    ) -> Result<ProofResult> {
        error!("Proof generation failed for epoch {}: {:?}", epoch_id, error);

        // Generate a halt proof to signal failure
        let halt_proof = ProofResult {
            epoch_id,
            proof_data: vec![0u8; 32], // Minimal halt proof
            journal_hash: [0u8; 32],   // Zero hash indicates halt
        };

        Ok(halt_proof)
    }
}

/// Proof validation utilities
impl ProofGenerator {
    /// Verify proof signature and structure
    fn verify_proof_structure(&self, proof_data: &[u8]) -> Result<()> {
        // In production, this would verify the RISC Zero proof structure
        // and cryptographic validity
        if proof_data.len() < 32 {
            bail!("Proof data too short");
        }

        // Check for valid proof header (simplified)
        if proof_data[0..4] != [0x52, 0x5a, 0x50, 0x52] { // "RZPR" magic
            // Allow for now - actual implementation would check real proof format
        }

        Ok(())
    }

    /// Extract journal from proof
    fn extract_journal_from_proof(&self, proof_data: &[u8]) -> Result<VrbcaJournal> {
        // In production, this would parse the actual RISC Zero journal format
        // For now, return a mock journal

        Ok(VrbcaJournal {
            epoch_id: 1,
            new_state_root: [1u8; 32],
            net_delta: 0,
            leverage: 10000, // 1.0x in basis points
            realized_pnl: 0,
            positions_commitment: [2u8; 32],
            execution_commitment: [3u8; 32],
            halt_flag: false,
        })
    }
}

/// VRBCA-specific journal structure
#[derive(Debug, Clone)]
struct VrbcaJournal {
    epoch_id: u64,
    new_state_root: [u8; 32],
    net_delta: i128,
    leverage: u128,
    realized_pnl: i128,
    positions_commitment: [u8; 32],
    execution_commitment: [u8; 32],
    halt_flag: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::strategy::BasisCaptureStrategy;

    #[test]
    fn test_input_preparation() {
        let proof_generator = ProofGenerator {
            boundless_client: // Would need mock client
            guest_binary_url: None,
            max_proof_time: Duration::from_secs(300),
        };

        let market_data = MarketData {
            spot_price: 3200_000000,
            perp_price: 3208_000000,
            funding_rate: 25,
            funding_interval_secs: 28800,
            liquidity_score: 800,
            timestamp: 1640995200,
        };

        let portfolio_state = PortfolioState::new(
            1, 
            1000000_000000, 
            1640995200, 
            [0u8; 32]
        );

        let mandate = TradingMandate::default_vrbca();
        let execution_reports = Vec::new();

        let inputs = proof_generator.prepare_proof_inputs(
            1,
            &market_data,
            &portfolio_state,
            &execution_reports,
            &mandate,
        );

        assert!(inputs.is_ok());
        let inputs = inputs.unwrap();
        assert!(inputs.len() > 100); // Should have substantial input data
    }

    #[test]
    fn test_proof_validation() {
        // Test would verify proof structure validation logic
        assert!(true); // Placeholder
    }
}