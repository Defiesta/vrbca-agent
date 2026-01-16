// Uniswap V3 contract integration for Ethereum mainnet

use anyhow::{Result, Context, bail};
use ethers::prelude::*;
use std::sync::Arc;
use rust_decimal::Decimal;
use rust_decimal::prelude::*;
use tracing::{info, debug, warn};
use ethers::utils::keccak256;
use ethers::abi::{encode, Token};

use crate::inputs::{PricePoint, LiquidityMetrics};

// Uniswap V3 contract addresses on Ethereum mainnet
pub const MAINNET_FACTORY: &str = "0x1F98431c8aD98523631AE4a59f267346ea31F984";
pub const MAINNET_QUOTER_V2: &str = "0x61fFE014bA17989E743c5f6cb21bF9697530B21e";

// Token addresses on Ethereum mainnet
pub const MAINNET_WETH: &str = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2";
pub const MAINNET_USDC: &str = "0xA0b86991c6218b36c1d19d4a2e9eb0ce3606eb48";

// Known high-liquidity pools on mainnet
pub const MAINNET_USDC_WETH_POOL_005: &str = "0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640"; // 0.05% fee tier (highest liquidity)
pub const MAINNET_USDC_WETH_POOL_03: &str = "0x8ad599c3a0ff1de082011efddc58f1908eb6e6d8"; // 0.3% fee tier

// Uniswap V3 pool init code hash (used for deterministic address computation)
pub const POOL_INIT_CODE_HASH: &str = "0xe34f199b19b2b4f47f68442619d555527d244f78a3297ea89325f843f87b8b54";

// Fee tiers
pub const FEE_LOW: u32 = 500;     // 0.05%
pub const FEE_MEDIUM: u32 = 3000; // 0.3%
pub const FEE_HIGH: u32 = 10000;  // 1%

// Generate contract ABIs using ethers abigen macro
abigen!(
    IUniswapV3Factory,
    r#"[{"type":"function","name":"getPool","inputs":[{"name":"token0","type":"address"},{"name":"token1","type":"address"},{"name":"fee","type":"uint24"}],"outputs":[{"name":"pool","type":"address"}],"stateMutability":"view"}]"#
);

abigen!(
    IUniswapV3Pool,
    r#"[
        {"type":"function","name":"slot0","inputs":[],"outputs":[{"name":"sqrtPriceX96","type":"uint160"},{"name":"tick","type":"int24"},{"name":"observationIndex","type":"uint16"},{"name":"observationCardinality","type":"uint16"},{"name":"observationCardinalityNext","type":"uint16"},{"name":"feeProtocol","type":"uint8"},{"name":"unlocked","type":"bool"}],"stateMutability":"view"},
        {"type":"function","name":"liquidity","inputs":[],"outputs":[{"name":"","type":"uint128"}],"stateMutability":"view"},
        {"type":"function","name":"token0","inputs":[],"outputs":[{"name":"","type":"address"}],"stateMutability":"view"},
        {"type":"function","name":"token1","inputs":[],"outputs":[{"name":"","type":"address"}],"stateMutability":"view"},
        {"type":"function","name":"fee","inputs":[],"outputs":[{"name":"","type":"uint24"}],"stateMutability":"view"}
    ]"#
);

abigen!(
    IQuoterV2,
    r#"[{"type":"function","name":"quoteExactInputSingle","inputs":[{"name":"tokenIn","type":"address"},{"name":"tokenOut","type":"address"},{"name":"fee","type":"uint24"},{"name":"amountIn","type":"uint256"},{"name":"sqrtPriceLimitX96","type":"uint160"}],"outputs":[{"name":"amountOut","type":"uint256"},{"name":"sqrtPriceX96After","type":"uint160"},{"name":"initializedTicksCrossed","type":"uint32"},{"name":"gasEstimate","type":"uint256"}],"stateMutability":"nonpayable"}]"#
);

abigen!(
    IERC20,
    r#"[
        {"type":"function","name":"decimals","inputs":[],"outputs":[{"name":"","type":"uint8"}],"stateMutability":"view"},
        {"type":"function","name":"symbol","inputs":[],"outputs":[{"name":"","type":"string"}],"stateMutability":"view"},
        {"type":"function","name":"name","inputs":[],"outputs":[{"name":"","type":"string"}],"stateMutability":"view"}
    ]"#
);

/// Uniswap V3 pool configuration
#[derive(Debug, Clone)]
pub struct PoolConfig {
    pub token0: Address,
    pub token1: Address,
    pub fee: u32,
    pub token0_decimals: u8,
    pub token1_decimals: u8,
    pub token0_symbol: String,
    pub token1_symbol: String,
}

/// Uniswap V3 integration client
pub struct UniswapV3Client {
    provider: Arc<Provider<Http>>,
    factory: IUniswapV3Factory<Provider<Http>>,
    quoter: IQuoterV2<Provider<Http>>,
    
    // Cached pool configurations
    pool_configs: std::collections::HashMap<Address, PoolConfig>,
    
    // Common pools
    weth_usdc_pool: Option<Address>,
}

impl UniswapV3Client {
    /// Create new Uniswap V3 client
    pub async fn new(rpc_url: &str) -> Result<Self> {
        let provider = Provider::<Http>::try_from(rpc_url)
            .context("Failed to create Ethereum provider")?;
        let provider = Arc::new(provider);

        let factory_address: Address = MAINNET_FACTORY.parse()
            .context("Invalid factory address")?;
        let quoter_address: Address = MAINNET_QUOTER_V2.parse()
            .context("Invalid quoter address")?;

        let factory = IUniswapV3Factory::new(factory_address, provider.clone());
        let quoter = IQuoterV2::new(quoter_address, provider.clone());

        let mut client = Self {
            provider,
            factory,
            quoter,
            pool_configs: std::collections::HashMap::new(),
            weth_usdc_pool: None,
        };

        // Initialize WETH/USDC pool
        client.init_weth_usdc_pool().await?;

        Ok(client)
    }

    /// Initialize the main WETH/USDC pool
    async fn init_weth_usdc_pool(&mut self) -> Result<()> {
        let weth: Address = MAINNET_WETH.parse()?;
        let usdc: Address = MAINNET_USDC.parse()?;

        // Use the highest liquidity pool (0.05% fee) directly
        info!("Using highest liquidity USDC/WETH pool (0.05% fee) for reliable data");
        let pool_addr = MAINNET_USDC_WETH_POOL_005.parse::<Address>()
            .context("Invalid USDC/WETH pool address")?;
            
        info!("Initialized WETH/USDC pool: {} (0.05% fee) on Ethereum mainnet", pool_addr);
        self.weth_usdc_pool = Some(pool_addr);
        
        // For the 0.05% USDC/WETH pool, USDC is token0, WETH is token1
        let config = PoolConfig {
            token0: usdc, // USDC is token0 in this pool
            token1: weth, // WETH is token1
            fee: FEE_LOW, // 0.05% = 500 bps
            token0_decimals: 6,  // USDC decimals
            token1_decimals: 18, // WETH decimals
            token0_symbol: "USDC".to_string(),
            token1_symbol: "WETH".to_string(),
        };
        self.pool_configs.insert(pool_addr, config);
        
        return Ok(());

        // This code should never be reached
        warn!("No WETH/USDC pool found on Ethereum mainnet");
        Ok(())
    }

    /// Get pool address using deterministic computation (like Uniswap SDK)
    pub fn compute_pool_address(&self, token0: Address, token1: Address, fee: u32) -> Result<Address> {
        // Ensure token0 < token1 (Uniswap requirement)
        let (token_a, token_b) = if token0 < token1 {
            (token0, token1)
        } else {
            (token1, token0)
        };

        // Encode the salt: abi.encode(token0, token1, fee)
        let salt = keccak256(encode(&[
            Token::Address(token_a),
            Token::Address(token_b), 
            Token::Uint(U256::from(fee)),
        ]));

        // Compute CREATE2 address: keccak256(0xff + factory + salt + init_code_hash)[12:]
        let factory_address = self.factory.address();
        let init_code_hash = POOL_INIT_CODE_HASH.parse::<H256>()
            .context("Invalid pool init code hash")?;

        let mut create2_input = Vec::new();
        create2_input.push(0xff); // CREATE2 prefix
        create2_input.extend_from_slice(factory_address.as_bytes());
        create2_input.extend_from_slice(&salt);
        create2_input.extend_from_slice(init_code_hash.as_bytes());

        let hash = keccak256(&create2_input);
        let pool_address = Address::from_slice(&hash[12..]);

        debug!("Computed pool address for {:#x}/{:#x} ({}bps): {:#x}", token_a, token_b, fee, pool_address);
        Ok(pool_address)
    }

    /// Get pool address from factory (fallback method)
    pub async fn get_pool_address(&self, token0: Address, token1: Address, fee: u32) -> Result<Address> {
        // First try deterministic computation
        match self.compute_pool_address(token0, token1, fee) {
            Ok(addr) => return Ok(addr),
            Err(e) => {
                debug!("Deterministic computation failed: {}, falling back to factory call", e);
            }
        }

        // Fallback to factory call
        let (token_a, token_b) = if token0 < token1 {
            (token0, token1)
        } else {
            (token1, token0)
        };

        let pool_address = self.factory
            .get_pool(token_a, token_b, fee)
            .call()
            .await
            .context("Failed to get pool address from factory")?;

        debug!("Pool address for {:#x}/{:#x} ({}bps): {:#x}", token_a, token_b, fee, pool_address);
        Ok(pool_address)
    }

    /// Get pool configuration including token details
    pub async fn get_pool_config(&self, pool_address: Address) -> Result<PoolConfig> {
        let pool = IUniswapV3Pool::new(pool_address, self.provider.clone());

        let token0 = pool.token_0().call().await?;
        let token1 = pool.token_1().call().await?;
        let fee = pool.fee().call().await?;

        // Get token details
        let token0_contract = IERC20::new(token0, self.provider.clone());
        let token1_contract = IERC20::new(token1, self.provider.clone());

        let token0_decimals = token0_contract.decimals().call().await?;
        let token0_symbol = token0_contract.symbol().call().await?;

        let token1_decimals = token1_contract.decimals().call().await?;
        let token1_symbol = token1_contract.symbol().call().await?;

        Ok(PoolConfig {
            token0,
            token1,
            fee,
            token0_decimals,
            token1_decimals,
            token0_symbol,
            token1_symbol,
        })
    }

    /// Get current spot price for ETH/USD using QuoterV2
    pub async fn get_eth_usd_spot_price(&self) -> Result<PricePoint> {
        let _pool_address = self.weth_usdc_pool
            .ok_or_else(|| anyhow::anyhow!("WETH/USDC pool not initialized"))?;

        let config = self.pool_configs.get(&_pool_address)
            .ok_or_else(|| anyhow::anyhow!("Pool config not found"))?;

        // Use QuoterV2 to get price by simulating a 1 WETH swap
        let weth_address = MAINNET_WETH.parse().unwrap();
        let usdc_address = MAINNET_USDC.parse().unwrap();
        let amount_in = U256::from(10u128.pow(18)); // 1 WETH = 1e18
        
        // Quote: WETH -> USDC to get ETH price in USD
        let quote_result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.quoter.quote_exact_input_single(
                weth_address,
                usdc_address, 
                config.fee,
                amount_in, // 1e18 (1 WETH)
                U256::zero() // sqrtPriceLimitX96 = 0 (no limit)
            ).call()
        )
        .await
        .context("QuoterV2 call timed out after 5 seconds")?
        .context("Failed to get quote from QuoterV2")?;

        let amount_out = quote_result.0; // USDC amount out
        
        // Convert to price: amount_out (USDC, 6 decimals) / amount_in (WETH, 18 decimals)
        let usdc_out = amount_out.as_u128() as f64 / 10f64.powi(6);
        let eth_in = amount_in.as_u128() as f64 / 10f64.powi(18);
        let price = usdc_out / eth_in;
        
        debug!("QuoterV2: {} WETH -> {} USDC = ${:.2}/ETH", eth_in, usdc_out, price);

        Ok(PricePoint {
            price: Decimal::from_f64(price)
                .ok_or_else(|| anyhow::anyhow!("Failed to convert price to Decimal"))?,
            source: "uniswap_v3_mainnet".to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs(),
            volume_24h: None,
            liquidity: None, // Don't need liquidity for price query
        })
    }

    /// Get liquidity metrics for ETH/USD pool
    pub async fn get_eth_usd_liquidity_metrics(&self) -> Result<LiquidityMetrics> {
        let pool_address = self.weth_usdc_pool
            .ok_or_else(|| anyhow::anyhow!("WETH/USDC pool not initialized"))?;

        let pool = IUniswapV3Pool::new(pool_address, self.provider.clone());
        let config = self.pool_configs.get(&pool_address)
            .ok_or_else(|| anyhow::anyhow!("Pool config not found"))?;

        // Get current state
        let slot0 = pool.slot_0().call().await?;
        let liquidity = pool.liquidity().call().await?;

        // Calculate market impact using quoter
        let test_amount = U256::from(10u128.pow(18)); // 1 ETH
        let market_impact = self.calculate_market_impact(
            MAINNET_WETH.parse()?,
            MAINNET_USDC.parse()?,
            config.fee,
            test_amount,
        ).await.unwrap_or(Decimal::from_f64(0.01).unwrap()); // Default 1% if calculation fails

        // Calculate spread (this is approximate for AMM)
        let weth_address = MAINNET_WETH.parse().unwrap();
        let is_token0_weth = config.token0 == weth_address;
        
        let _current_price = self.sqrt_price_x96_to_price(
            slot0.0,
            config.token0_decimals,
            config.token1_decimals,
            is_token0_weth,
        )?;

        // Estimate spread based on fee tier
        let spread_bps = config.fee as u64 / 100; // Convert fee to basis points

        Ok(LiquidityMetrics {
            bid_depth_usd: Decimal::from(liquidity) / Decimal::from(2), // Rough estimate
            ask_depth_usd: Decimal::from(liquidity) / Decimal::from(2),
            spread_bps,
            market_impact_1m: market_impact,
            volume_24h: Decimal::ZERO, // Would need event processing
            source: "uniswap_v3_mainnet".to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs(),
        })
    }

    /// Convert sqrtPriceX96 to human-readable price
    fn sqrt_price_x96_to_price(
        &self,
        sqrt_price_x96: U256,
        decimals0: u8,
        decimals1: u8,
        is_token0_base: bool, // true if token0 is the base token (ETH)
    ) -> Result<Decimal> {
        if sqrt_price_x96.is_zero() {
            return Ok(Decimal::ZERO);
        }

        // Convert to f64 for calculation
        let sqrt_price = sqrt_price_x96.as_u128() as f64 / (1u128 << 96) as f64;
        let price = sqrt_price * sqrt_price;

        // Adjust for decimal differences
        let decimal_adjustment = 10f64.powi((decimals1 as i32) - (decimals0 as i32));
        let adjusted_price = price * decimal_adjustment;

        // If token0 is not the base token, we need to invert the price
        let final_price = if is_token0_base {
            adjusted_price
        } else {
            1.0 / adjusted_price
        };

        Decimal::from_f64(final_price)
            .ok_or_else(|| anyhow::anyhow!("Failed to convert price to Decimal"))
    }

    /// Calculate market impact for a given trade size
    async fn calculate_market_impact(
        &self,
        token_in: Address,
        token_out: Address,
        fee: u32,
        amount_in: U256,
    ) -> Result<Decimal> {
        // Use quoter to get expected output
        let quote_result = self.quoter
            .quote_exact_input_single(token_in, token_out, fee, amount_in, U256::zero())
            .call()
            .await
            .context("Failed to get quote from quoter")?;

        let amount_out = quote_result.0;

        // Calculate effective price
        if amount_out.is_zero() || amount_in.is_zero() {
            return Ok(Decimal::ZERO);
        }

        // Simple price impact calculation
        // This is a rough estimate - more sophisticated impact calculation would require
        // comparing with current pool price
        let effective_rate = amount_out.as_u128() as f64 / amount_in.as_u128() as f64;
        
        // Estimate impact as percentage of trade size
        // For larger trades, impact would be higher
        let trade_size_impact = (amount_in.as_u128() as f64 / 1e18) * 0.001; // 0.1% per ETH
        
        Ok(Decimal::from_f64(trade_size_impact.min(0.05)) // Cap at 5%
            .unwrap_or(Decimal::from_f64(0.01).unwrap()))
    }

    /// Get TWAP (Time-Weighted Average Price) from pool observations
    pub async fn get_twap(&self, period_seconds: u32) -> Result<Decimal> {
        let pool_address = self.weth_usdc_pool
            .ok_or_else(|| anyhow::anyhow!("WETH/USDC pool not initialized"))?;

        let pool = IUniswapV3Pool::new(pool_address, self.provider.clone());
        let config = self.pool_configs.get(&pool_address)
            .ok_or_else(|| anyhow::anyhow!("Pool config not found"))?;

        // Simplified TWAP - use current price as fallback
        // Note: Full TWAP requires observe() function in ABI
        warn!("TWAP calculation simplified - using current price as fallback");
        
        let slot0 = pool.slot_0().call().await
            .context("Failed to get pool slot0 for TWAP")?;
        
        let sqrt_price_x96 = slot0.0; // First element is sqrtPriceX96
        let price = self.sqrt_price_x96_to_price(sqrt_price_x96, config.token0_decimals, config.token1_decimals, true)?;

        Ok(price)
    }

    /// Convert tick to price
    fn tick_to_price(&self, tick: i32, decimals0: u8, decimals1: u8) -> Result<Decimal> {
        // Price = 1.0001^tick * (10^(decimals1 - decimals0))
        let base_price = 1.0001_f64.powi(tick);
        let decimal_adjustment = 10f64.powi((decimals1 as i32) - (decimals0 as i32));
        let final_price = base_price * decimal_adjustment;

        Decimal::from_f64(final_price)
            .ok_or_else(|| anyhow::anyhow!("Failed to convert tick price to Decimal"))
    }

    /// Verify if a pool exists and has liquidity
    async fn verify_pool_exists(&self, pool_addr: Address) -> bool {
        let pool = IUniswapV3Pool::new(pool_addr, self.provider.clone());
        
        // Try to get basic pool information
        match pool.liquidity().call().await {
            Ok(liquidity) => {
                debug!("Pool {} has liquidity: {}", pool_addr, liquidity);
                liquidity > 0
            }
            Err(e) => {
                debug!("Pool {} verification failed: {}", pool_addr, e);
                false
            }
        }
    }

    /// Check if pool exists and is active
    pub async fn is_pool_active(&self, token0: Address, token1: Address, fee: u32) -> bool {
        match self.compute_pool_address(token0, token1, fee) {
            Ok(pool_addr) => self.verify_pool_exists(pool_addr).await,
            Err(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_contract_addresses() {
        // Test that all addresses are valid
        assert!(MAINNET_FACTORY.parse::<Address>().is_ok());
        assert!(MAINNET_QUOTER_V2.parse::<Address>().is_ok());
        assert!(MAINNET_WETH.parse::<Address>().is_ok());
        assert!(MAINNET_USDC.parse::<Address>().is_ok());
    }

    #[test]
    fn test_fee_constants() {
        assert_eq!(FEE_LOW, 500);
        assert_eq!(FEE_MEDIUM, 3000);
        assert_eq!(FEE_HIGH, 10000);
    }

    #[tokio::test]
    async fn test_client_creation() {
        // This test would need a valid RPC URL to pass
        let result = UniswapV3Client::new("https://invalid-url").await;
        assert!(result.is_err()); // Expected to fail with invalid URL
    }
}