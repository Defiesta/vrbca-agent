# VRBCA Agent - Verifiable Risk-Bound Basis Capture Agent

A delta-neutral DeFi trading system that captures funding rate arbitrage between spot and perpetual markets while maintaining verifiable risk constraints through RISC Zero zero-knowledge proofs.

## Overview

The VRBCA (Verifiable Risk-Bound Basis Capture Agent) implements an institutional-grade basis capture strategy that:

- **Captures funding rate spreads** between perpetual and spot markets
- **Maintains delta neutrality** through coordinated hedging across venues
- **Enforces immutable risk constraints** via zero-knowledge proofs
- **Provides transparent performance** tracking for institutional investors
- **Operates across multiple venues** including Binance (CEX) and Uniswap V3 (DEX)

## Quick Start

### Prerequisites
1. Install RISC Zero:
   ```bash
   curl -L https://risczero.com/install | bash
   rzup install
   ```

2. Install Foundry:
   ```bash
   curl -L https://foundry.paradigm.xyz | bash
   foundryup
   ```

### Running VRBCA

1. **Demo the VRBCA system**:
   ```bash
   cargo run --bin app
   ```

2. **Run the epoch orchestrator**:
   ```bash
   cargo run --bin vrbca-host -- --current-price 3500
   ```

3. **Build all components**:
   ```bash
   cargo build && forge build
   ```


## 🧪 Development

### Testing
```bash
# Test Rust components
cargo test

# Test smart contracts
forge test -vvv

# Run integration tests
cargo test --package guests --test integration
```

### Building
```bash
# Build guest programs
cargo build --package guests

# Build host applications
cargo build --package vrbca-app --package vrbca-host

# Build contracts
forge build
```


## Documentation

- [SPECIFICATIONS.md](./SPECIFICATIONS.md): Complete technical specifications and workflow diagrams

## Contributing

1. Fork the repository
2. Create a feature branch: `git checkout -b feature/amazing-feature`
3. Commit your changes: `git commit -m 'Add amazing feature'`
4. Push to the branch: `git push origin feature/amazing-feature`
5. Open a Pull Request

## License

This project is licensed under the Apache License 2.0 - see the [LICENSE](LICENSE) file for details.

## ⚠️ Disclaimer

This software is provided for educational and research purposes. Trading cryptocurrencies involves substantial risk and may result in significant financial losses. The VRBCA system is experimental technology and should only be used by sophisticated investors who understand the risks involved. Always perform your own due diligence and consider consulting with financial advisors before making investment decisions.

---

**Built with RISC Zero, Boundless Network, and Foundry for institutional-grade DeFi trading.**