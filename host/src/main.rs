// VRBCA Host Application

use anyhow::Result;
use clap::{Parser, Subcommand};
use dotenvy::dotenv;
use tracing::{info, warn};

mod inputs;
mod executor;
mod model;
mod prover;
mod contracts;

/// Arguments for the VRBCA host application
#[derive(Parser, Debug)]
#[clap(name = "vrbca-host", about = "VRBCA Host Orchestrator")]
struct Args {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Run the main VRBCA orchestrator
    Run {
        /// Current ETH price for simulation
        #[clap(long, default_value = "3200")]
        current_price: u64,
    },
    /// Test market data collection
    TestMarketData {
        /// Symbol to test (default: ETHUSDT)
        #[clap(long, default_value = "ETHUSDT")]
        symbol: String,
    },
    /// Test strategy execution
    TestStrategy {
        /// Symbol to test (default: ETHUSDT)
        #[clap(long, default_value = "ETHUSDT")]
        symbol: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables
    let _ = dotenv();
    
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args = Args::parse();
    
    match args.command {
        Commands::Run { current_price } => {
            run_vrbca_orchestrator(current_price).await
        }
        Commands::TestMarketData { symbol } => {
            test_market_data_collection(&symbol).await
        }
        Commands::TestStrategy { symbol } => {
            test_strategy_execution(&symbol).await
        }
    }
}

/// Run the main VRBCA orchestrator
async fn run_vrbca_orchestrator(current_price: u64) -> Result<()> {
    use executor::ExecutionCoordinator;
    use model::ExtendedFundingModel;
    use prover::ProofGenerator;
    
    info!("🚀 VRBCA Host Orchestrator starting...");
    info!("💰 ETH price: ${}", current_price);
    
    // Initialize market data collection
    let _market_data = test_market_data_collection("ETHUSDT").await?;
    
    // Initialize components
    let _executor = ExecutionCoordinator::new();
    let _model = ExtendedFundingModel::new();
    let _prover = ProofGenerator::new();
    
    info!("✅ VRBCA components initialized successfully");
    info!("📊 Market data collection completed successfully");
    
    // TODO: In the full implementation, this would:
    // 1. ✅ Collect market data (implemented)
    // 2. Run strategy signals
    // 3. Execute trades
    // 4. Generate proofs
    
    Ok(())
}

/// Test market data collection system
async fn test_market_data_collection(symbol: &str) -> Result<()> {
    use inputs::{ProductionMarketDataCollector, MarketDataArgs};
    
    info!("🧪 Starting market data collection for {}", symbol);
    
    let market_config = MarketDataArgs {
        binance_api_key: std::env::var("BINANCE_API_KEY").ok(),
        binance_api_secret: std::env::var("BINANCE_API_SECRET").ok(),
        binance_use_testnet: false,
        
        rpc_url: std::env::var("RPC_URL").expect("RPC_URL must be set in .env file"),
        chain_id: 1, // Ethereum Mainnet
        
        chainlink_eth_feed: Some("0x5f4eC3Df9cbd43714FE2740f5E3616155c5b8419".to_string()),
        uniswap_router: Some("0xE592427A0AEce92De3Edee1F18E0157C05861564".to_string()),
        enable_uniswap: false, // Disabled due to RPC rate limits
        
        enable_websockets: false,
        data_quality_checks: true,
    };
    
    let collector = ProductionMarketDataCollector::new(market_config).await?;
    let market_data = collector.collect_enhanced_market_data(symbol).await?;
    
    info!("✅ Market data collected successfully:");
    info!("  📊 Spot Price: ${:.2}", market_data.market_data.spot_price as f64 / 1_000_000.0);
    info!("  ⚡ Perpetual Price: ${:.2}", market_data.market_data.perp_price as f64 / 1_000_000.0);
    info!("  📈 Funding Rate: {:.4}%", market_data.market_data.funding_rate as f64 / 10_000.0);
    info!("  🏆 Liquidity Score: {}", market_data.market_data.liquidity_score);
    
    Ok(())
}

/// Test strategy execution with market data
async fn test_strategy_execution(symbol: &str) -> Result<()> {
    use executor::ExecutionCoordinator;
    use prover::ProofGenerator;
    
    info!("📈 Testing strategy execution for symbol: {}", symbol);
    
    // Collect market data
    let market_data = {
        use inputs::{ProductionMarketDataCollector, MarketDataArgs};
        
        let market_config = MarketDataArgs {
            binance_api_key: std::env::var("BINANCE_API_KEY").ok(),
            binance_api_secret: std::env::var("BINANCE_API_SECRET").ok(),
            binance_use_testnet: false,
            
            rpc_url: std::env::var("RPC_URL").expect("RPC_URL must be set in .env file"),
            chain_id: 1, // Ethereum Mainnet
            
            chainlink_eth_feed: Some("0x5f4eC3Df9cbd43714FE2740f5E3616155c5b8419".to_string()),
            uniswap_router: Some("0xE592427A0AEce92De3Edee1F18E0157C05861564".to_string()),
            enable_uniswap: false, // Disabled due to RPC rate limits
            
            enable_websockets: false,
            data_quality_checks: true,
        };
        
        let collector = ProductionMarketDataCollector::new(market_config).await?;
        collector.collect_enhanced_market_data(symbol).await?
    };
    
    // Test strategy execution
    let executor = ExecutionCoordinator::new();
    executor.execute_basis_capture(&market_data).await?;
    
    // Test proof generation
    let prover = ProofGenerator::new();
    prover.generate_execution_proof(&market_data).await?;
    
    info!("✅ Strategy execution test completed successfully");
    
    Ok(())
}

