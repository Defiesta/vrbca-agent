# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Architecture

This project implements a **Verifiable Risk-Bound Basis Capture Agent (VRBCA)** - a sophisticated delta-neutral trading system that captures funding rate arbitrage between spot and perpetual markets while maintaining verifiable risk constraints through RISC Zero zero-knowledge proofs.

## 🚀 Current Status: DEVELOPMENT COMPLETE ✅

- ✅ **Project Transformation**: Successfully converted from simple trading-signal to full VRBCA architecture
- ✅ **Core Modules**: Implemented strategy, risk, mandate, and state management systems
- ✅ **Guest Program**: RISC Zero zkVM program for verifiable strategy execution 
- ✅ **Smart Contracts**: Settlement, AgentRegistry, and Vault contracts for on-chain operations
- ✅ **Host Applications**: Epoch orchestrator and client applications
- ✅ **Configuration**: Risk limits, market parameters, and epoch settings
- ✅ **Build System**: Successfully compiles and runs all components

## 🏗️ VRBCA Architecture Overview

### Core Strategy Components

- **Basis Capture Strategy** (`core/strategy.rs`): Implements delta-neutral basis capture with funding rate persistence models
  - Funding rate threshold detection (minimum 0.20% for capture)
  - Liquidity scoring and position sizing
  - Linear regression for funding rate predictions
  - Market data processing from multiple venues

- **Risk Management** (`core/risk.rs`): Enforces strict risk constraints
  - Maximum leverage: 2x (20,000 basis points)
  - Maximum net delta: 1% (100 basis points) 
  - Maximum drawdown: 20%
  - Real-time position monitoring and halt conditions

- **Trading Mandate** (`core/mandate.rs`): Immutable strategy parameters
  - Strategy code hash verification for integrity
  - Venue configurations (Binance, Uniswap)
  - Emergency halt conditions and circuit breakers
  - Capital allocation limits per market

- **Portfolio State** (`core/state.rs`): Position and execution tracking
  - Multi-venue position management
  - PnL calculation and reporting
  - Execution report verification
  - State transitions and epoch management

### RISC Zero Integration

- **Guest Program** (`guests/vrbca/`): Verifiable computation in zkVM
  - **Current IMAGE_ID**: `0x7c54f587c232bb6da7aad2ac50b1d4fa9f69bdc03df4103cbfe64032b8413a98`
  - **Binary Size**: 300,180 bytes
  - Proves compliance with risk constraints
  - Verifies strategy execution integrity
  - Generates journals for on-chain settlement

- **Host Orchestrator** (`host/`): Epoch-based execution coordinator
  - Market data collection from multiple sources
  - Strategy signal generation and validation  
  - Multi-venue execution coordination
  - Proof generation via Boundless Market

### Smart Contracts

- **Settlement Contract** (`contracts/src/Settlement.sol`): Epoch settlement with proof verification
  - RISC Zero proof validation
  - State root updates and finalization
  - Emergency halt mechanisms
  - Slashing conditions for violations

- **Agent Registry** (`contracts/src/AgentRegistry.sol`): Agent and mandate management
  - Agent registration and verification
  - Mandate hash validation
  - Operator permissions and controls
  - Strategy parameter enforcement

- **Vault Contract** (`contracts/src/Vault.sol`): Capital management and investor operations
  - Deposit and withdrawal processing
  - Share token management
  - Performance fee calculation
  - Liquidity provider rewards

### Configuration System

- **Risk Configuration** (`config/risk.yaml`): Risk limits and monitoring
  - Leverage and delta constraints
  - Drawdown limits and halt conditions
  - Position sizing parameters
  - Emergency response settings

- **Market Configuration** (`config/markets.yaml`): Venue and asset settings
  - Exchange API configurations
  - Asset pair definitions
  - Oracle price feed sources
  - Liquidity thresholds

- **Epoch Configuration** (`config/epochs.yaml`): Timing and settlement
  - Proof generation schedules
  - Settlement windows
  - Timeout parameters
  - Retry mechanisms

## Common Development Commands

### Building
```bash
# Build all components (contracts, guest programs, host applications)
cargo build

# Build Solidity contracts
forge build

# Build RISC Zero guest programs
cargo build --package guests
```

### Testing
```bash
# Run VRBCA application (demonstrates successful transformation)
cargo run --bin app

# Run VRBCA host orchestrator
cargo run --bin vrbca-host

# Test smart contracts
forge test -vvv

# Test Rust components
cargo test
```

### Running the VRBCA System

```bash
# Run main VRBCA application
cargo run --bin app

# Run host orchestrator with custom ETH price
cargo run --bin vrbca-host -- --current-price 3500

# Generate new guest program binary (when making changes)
cargo build --package guests
```

## Environment Variables

**VRBCA Development Configuration**:
```bash
# Network settings
RPC_URL=https://base-mainnet.g.alchemy.com/v2/YOUR_API_KEY
CHAIN_ID=8453

# Contract addresses (will be deployed for VRBCA)
SETTLEMENT_ADDRESS=0x... # Settlement contract
AGENT_REGISTRY_ADDRESS=0x... # Agent registry contract  
VAULT_ADDRESS=0x... # Vault contract

# RISC Zero and Boundless integration
VERIFIER_ADDRESS=0x0b144e07a0826182b6b59788c34b32bfa86fb711
BOUNDLESS_MARKET_ADDRESS=0xfd152dadc5183870710fe54f939eae3ab9f0fe82
SET_VERIFIER_ADDRESS=0x1Ab08498CfF17b9723ED67143A050c8E8c2e3104

# Security credentials (use .env file)
PRIVATE_KEY=0x... # ⚠️ NEVER EXPOSE
PINATA_JWT=eyJ... # ⚠️ KEEP SECRET
```

## 🔒 **CRITICAL SECURITY PRACTICES**

### **NEVER expose private keys in commands!** Use `.env` file instead:

**SECURE Setup**:
1. Create `.env` file in project root:
```bash
# .env file (NEVER commit this to git)
RPC_URL=https://base-mainnet.g.alchemy.com/v2/YOUR_API_KEY
PRIVATE_KEY=0xYOUR_PRIVATE_KEY_HERE
CHAIN_ID=8453
BOUNDLESS_MARKET_ADDRESS=0xfd152dadc5183870710fe54f939eae3ab9f0fe82
SET_VERIFIER_ADDRESS=0x1Ab08498CfF17b9723ED67143A050c8E8c2e3104
PINATA_JWT=your_pinata_jwt_here
```

2. Add `.env` to `.gitignore`:
```bash
echo ".env" >> .gitignore
```

3. **SAFE Command** (reads from .env automatically):
```bash
# SECURE: No private keys in command line
cargo run --bin app
```

### **Security Checklist**:
- ✅ Use `.env` file for secrets
- ✅ Add `.env` to `.gitignore` 
- ✅ Use test wallets with minimal funds for development
- ✅ Clear shell history after accidental exposure: 
  - **Bash**: `history -c && history -w`
  - **Zsh**: `fc -p && > ~/.zsh_history && exec zsh`
- ✅ Use hardware wallets for production funds
- ✅ Never paste private keys in shared terminals/logs

## Development Patterns

### Strategy Development
- VRBCA implements delta-neutral basis capture between spot and perpetual markets
- Uses funding rate persistence models for signal generation
- Maintains market neutrality through coordinated hedging
- All calculations use integer arithmetic for zkVM determinism

### Risk Management Implementation
- Real-time constraint validation in both host and guest programs
- Circuit breakers halt execution when limits are exceeded
- Position sizing based on Kelly criterion with conservative parameters
- Multi-layered risk monitoring across venues

### Proof Generation Workflow
1. **Data Collection**: Aggregate market data from multiple sources
2. **Signal Generation**: Run basis capture strategy with risk validation
3. **Execution Coordination**: Execute trades across venues maintaining delta neutrality
4. **Proof Generation**: Submit epoch data to Boundless Market for zkVM execution
5. **Settlement**: Verify proofs on-chain and update portfolio state

### Smart Contract Integration
- Contracts verify RISC Zero proofs using latest verifier system
- State updates are atomic and include risk metric validation
- Emergency mechanisms allow immediate halt of operations
- Slashing conditions penalize constraint violations

## File Structure

```
├── core/                          # Core strategy modules
│   ├── strategy.rs               # Basis capture strategy implementation
│   ├── risk.rs                   # Risk management and constraints
│   ├── mandate.rs                # Trading mandate definitions
│   ├── state.rs                  # Portfolio and position state
│   └── mod.rs                    # Module exports
├── guests/                       # RISC Zero guest programs
│   ├── vrbca/                    # VRBCA guest program
│   │   ├── src/main.rs          # zkVM proof generation logic
│   │   └── Cargo.toml           # Guest dependencies
│   ├── build.rs                 # Build script for guest compilation
│   ├── src/lib.rs               # Generated methods exports
│   └── Cargo.toml               # Guests package configuration
├── host/                         # Host applications
│   ├── src/
│   │   ├── main.rs              # Epoch orchestrator
│   │   ├── inputs.rs            # Market data collection
│   │   ├── executor.rs          # Multi-venue execution
│   │   ├── prover.rs            # Proof generation coordination
│   │   └── model.rs             # Funding prediction models
│   └── Cargo.toml               # Host dependencies
├── apps/                         # Client applications
│   ├── src/main.rs              # Main VRBCA application entry point
│   └── Cargo.toml               # App dependencies
├── contracts/                    # Smart contracts
│   ├── src/
│   │   ├── Settlement.sol       # Epoch settlement with proof verification
│   │   ├── AgentRegistry.sol    # Agent and mandate management
│   │   ├── Vault.sol            # Capital management
│   │   └── ImageID.sol          # Generated guest program image IDs
│   ├── scripts/Deploy.s.sol     # Deployment scripts
│   └── test/                    # Contract tests
├── config/                       # Configuration files
│   ├── risk.yaml                # Risk limits and monitoring
│   ├── markets.yaml             # Venue and asset configurations
│   └── epochs.yaml              # Timing and settlement parameters
├── scripts/                      # Utility scripts
│   └── simulate_epoch.rs        # Epoch simulation and testing
└── Cargo.toml                   # Workspace configuration
```

## VRBCA Strategy Details

### Basis Capture Mechanics
- **Target**: Capture positive funding rates in perpetual markets while hedging spot exposure
- **Method**: Long perpetual + short spot when funding rate > threshold
- **Neutrality**: Maintain delta-neutral position to isolate funding alpha
- **Persistence**: Use linear regression to predict funding rate sustainability

### Risk Constraints (Immutable)
- **Maximum Leverage**: 2x across all positions
- **Maximum Net Delta**: 1% of portfolio value
- **Maximum Drawdown**: 20% from high water mark
- **Position Size Limit**: 10% of daily volume per venue
- **Minimum Funding Threshold**: 0.20% (20 basis points)

### Execution Flow
1. **Market Scan**: Identify positive funding rates above threshold
2. **Liquidity Check**: Verify sufficient liquidity on both spot and perpetual
3. **Signal Generation**: Calculate optimal position sizes within risk limits
4. **Coordinated Execution**: Simultaneously execute hedging trades
5. **Monitoring**: Continuous risk monitoring with automatic halts
6. **Settlement**: Periodic proof generation and on-chain settlement

### Performance Metrics
- **Sharpe Ratio Target**: > 1.5 (net of fees)
- **Maximum Daily VaR**: 2% of portfolio
- **Target APY**: 15-25% (depending on funding rate environment)
- **Correlation to ETH**: < 0.1 (delta-neutral validation)

## Troubleshooting

### Common Build Issues
- **Guest compilation errors**: Ensure `risc0-zkvm` dependencies are correctly versioned
- **Contract deployment failures**: Verify network configuration and gas settings
- **Proof generation timeouts**: Check Boundless Market connectivity and queue status

### Runtime Issues
- **Risk constraint violations**: Review position sizing and market conditions
- **Venue connectivity problems**: Check API credentials and rate limits
- **State synchronization errors**: Verify epoch timing and settlement windows

## Development Roadmap

### Phase 1: Core Implementation ✅
- [x] Strategy modules and risk management
- [x] RISC Zero guest program
- [x] Smart contract suite
- [x] Host orchestrator and applications
- [x] Configuration system

### Phase 2: Testing and Optimization
- [ ] Comprehensive unit and integration tests
- [ ] Gas optimization for contract interactions
- [ ] Performance benchmarking and tuning
- [ ] Security audit and penetration testing

### Phase 3: Production Deployment
- [ ] Mainnet contract deployment
- [ ] Production monitoring and alerting
- [ ] Investor dashboard and reporting
- [ ] Automated market making integration

### Phase 4: Advanced Features
- [ ] Multi-asset basis capture (BTC, SOL, etc.)
- [ ] Cross-chain arbitrage opportunities
- [ ] Machine learning alpha generation
- [ ] Institutional capital onboarding

## Rust Toolchain

This project uses Rust 1.89 as specified in `rust-toolchain.toml` for RISC Zero compatibility. The toolchain includes clippy, rustfmt, and rust-src components.

# important-instruction-reminders
Do what has been asked; nothing more, nothing less.
NEVER create files unless they're absolutely necessary for achieving your goal.
ALWAYS prefer editing an existing file to creating a new one.
NEVER proactively create documentation files (*.md) or README files. Only create documentation files if explicitly requested by the User.

      
      IMPORTANT: this context may or may not be relevant to your tasks. You should not respond to this context unless it is highly relevant to your task.