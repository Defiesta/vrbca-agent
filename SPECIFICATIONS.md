# Project Report — Verifiable Risk-Bound Basis Capture Agent (VRBCA)

## Agent Execution Description 

The agent allocates capital to capture funding-rate and spot–perp basis while remaining provably delta-neutral. At fixed epochs, it consumes relayer-attested market data and execution reports from approved venues.
A RISC Zero guest program verifies that the strategy logic, linear model inference, and deterministic risk constraints were correctly applied to those inputs.
A zero-knowledge proof authorizes on-chain settlement and capital continuation without trusting the relayer’s behavior beyond attestation correctness.
Trades may occur on Binance or DEXs, but capital state advances only with valid proofs referencing an immutable mandate.
Failure to produce proofs, or proof-indicated violations, halts execution and triggers unwind procedures.

⸻

## VRBCA Implementation Workflow Diagram

```
┌─────────────────────────────────────────────────────────────────────────────────────┐
│                           VRBCA EPOCH EXECUTION FLOW                                │
├─────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                     │
│  ┌──────────────┐     ┌──────────────┐     ┌──────────────┐     ┌──────────────┐    │
│  │   MARKET     │────▶│   STRATEGY   │────▶│  EXECUTION   │────▶│   SETTLEMENT │    │
│  │   SCANNING   │     │  VALIDATION  │     │ COORDINATION │     │ & PROOF GEN  │    │
│  └──────────────┘     └──────────────┘     └──────────────┘     └──────────────┘    │
│                                                                                     │
└─────────────────────────────────────────────────────────────────────────────────────┘

┌─────────────────┐
│ 1. MARKET DATA  │ 
│    COLLECTION   │ 
└─────────┬───────┘
          │
          ▼
┌─────────────────────────────────────┐
│ Host Orchestrator (host/src/main.rs)│
├─────────────────────────────────────┤
│ • MarketDataCollector::collect()    │
│   ├─ Binance API (perp prices)      │
│   ├─ Chainlink oracles (spot)       │
│   └─ Uniswap V3 pools (liquidity)   │
│ • FundingModel::predict()           │
│   └─ Linear regression analysis     │
└─────────┬───────────────────────────┘
          │ MarketData struct
          ▼
┌─────────────────────────────────────┐
│ 2. STRATEGY EXECUTION               │
├─────────────────────────────────────┤
│ BasisCaptureStrategy::generate()    │
│ (core/strategy.rs)                  │
├─────────────────────────────────────┤
│ • Check funding_rate ≥ 20 bps       │
│ • Verify liquidity_score ≥ 700      │
│ • Calculate position_size           │
│ • Enforce risk constraints:         │
│   ├─ leverage ≤ 2x                  │
│   ├─ net_delta ≤ 1%                 │
│   └─ max_drawdown ≤ 20%             │
└─────────┬───────────────────────────┘
          │ StrategySignal
          ▼
┌─────────────────────────────────────┐
│ 3. RISK VALIDATION                  │ 
├─────────────────────────────────────┤
│ RiskValidator::validate_constraints │
│ (core/risk.rs)                      │
├─────────────────────────────────────┤
│ • Portfolio risk metrics            │
│ • Emergency halt conditions         │
│ • Position sizing limits            │
│ • Mandate compliance check          │
└─────────┬───────────────────────────┘
          │ ValidationResult
          ▼
┌─────────────────────────────────────┐
│ 4. MULTI-VENUE EXECUTION            │
├─────────────────────────────────────┤
│ ExecutionCoordinator::execute()     │
│ (host/src/executor.rs)              │
├─────────────────────────────────────┤
│ Parallel execution:                 │
│ ┌─────────────┐ ┌─────────────┐     │
│ │   BINANCE   │ │  UNISWAP V3 │     │
│ │    (PERP)   │ │   (SPOT)    │     │
│ ├─────────────┤ ├─────────────┤     │
│ │ Long ETH-   │ │ Short ETH/  │     │
│ │ PERP at     │ │ USDC at     │     │
│ │ 2x leverage │ │ spot price  │     │
│ └─────────────┘ └─────────────┘     │
│                                     │
│ Result: Net delta ≈ 0%              │
│         Capture funding spread      │
└─────────┬───────────────────────────┘
          │ ExecutionReport[]
          ▼
┌─────────────────────────────────────┐
│ 5. STATE AGGREGATION                │
├─────────────────────────────────────┤
│ PortfolioState::update()            │
│ (core/state.rs)                     │
├─────────────────────────────────────┤
│ • Aggregate positions               │
│ • Calculate realized PnL            │
│ • Update risk metrics               │
│ • Generate state_root hash          │
└─────────┬───────────────────────────┘
          │ EpochData
          ▼
┌─────────────────────────────────────┐
│ 6. PROOF GENERATION                 │
├─────────────────────────────────────┤
│ ProofGenerator::submit_to_boundless │
│ (host/src/prover.rs)                │
├─────────────────────────────────────┤
│ • Serialize epoch inputs            │
│ • Submit to Boundless Market        │
│ • Wait for zkVM execution           │
│   │                                 │
│   ▼ RISC Zero zkVM                  │
│ ┌─────────────────────────────────┐ │
│ │ VRBCA Guest Program             │ │
│ │ (guests/vrbca/src/main.rs)      │ │
│ ├─────────────────────────────────┤ │
│ │ 1. Decode epoch inputs          │ │
│ │ 2. Re-run strategy validation   │ │
│ │ 3. Verify risk constraints:     │ │
│ │    ✓ leverage ≤ 20,000 bps      │ │
│ │    ✓ net_delta ≤ 100 bps        │ │
│ │    ✓ execution integrity        │ │
│ │ 4. Generate journal:            │ │
│ │    [epoch_id, state_root,       │ │
│ │     net_delta, leverage,        │ │
│ │     realized_pnl, positions_    │ │
│ │     hash, execution_hash,       │ │
│ │     halt_flag]                  │ │
│ │ 5. env::commit_slice(journal)   │ │
│ └─────────────────────────────────┘ │
│                                     │
│ • Receive proof + receipt           │
└─────────┬───────────────────────────┘
          │ zkProof + Journal
          ▼
┌─────────────────────────────────────┐
│ 7. AGENT & MANDATE VERIFICATION     │
├─────────────────────────────────────┤
│ AgentRegistry.sol::validateAgent()  │
│ (contracts/src/AgentRegistry.sol)   │
├─────────────────────────────────────┤
│ • Verify agent is registered        │
│ • Check mandate hash matches        │
│ • Validate strategy code hash       │
│ • Confirm capital limits not        │
│   exceeded (max 50% of vault)       │
│ • Check operator permissions        │
└─────────┬───────────────────────────┘
          │ AgentValidation
          ▼
┌─────────────────────────────────────┐
│ 8. VAULT CAPITAL MANAGEMENT         │
├─────────────────────────────────────┤
│ Vault.sol::updateCapitalAllocation  │
│ (contracts/src/Vault.sol)           │
├─────────────────────────────────────┤
│ • Track capital allocation per      │
│   agent (agentAllocations mapping)  │
│ • Update agent performance metrics: │
│   ├─ totalPnl (realized profits)    │
│   ├─ currentValue (portfolio value) │
│   ├─ sharpeRatio calculation        │
│   ├─ maxDrawdown tracking           │
│   └─ highWaterMark updates          │
│ • Calculate management & perf fees  │
│ • Process pending withdrawals       │
└─────────┬───────────────────────────┘
          │ VaultUpdate
          ▼
┌─────────────────────────────────────┐
│ 9. ON-CHAIN SETTLEMENT              │
├─────────────────────────────────────┤
│ Settlement.sol::settleEpoch()       │
│ (contracts/src/Settlement.sol)      │
├─────────────────────────────────────┤
│ • Verify RISC Zero proof            │
│ • Validate journal format           │
│ • Check IMAGE_ID matches            │
│ • Cross-reference with AgentRegistry│
│ • Update portfolio state root       │
│ • Trigger Vault capital update      │
│ • Emit EpochSettled event           │
│   │                                 │
│   ├─ If halt_flag = true:           │
│   │  ├─ Trigger emergency unwind    │
│   │  ├─ Freeze agent operations     │
│   │  └─ Notify Vault for recovery   │
│   │                                 │
│   └─ If valid: Continue to next     │
│      epoch with updated state       │
└─────────┬───────────────────────────┘
          │
          ▼
┌─────────────────────────────────────┐
│ 10. INVESTOR OPERATIONS             │
├─────────────────────────────────────┤
│ Vault.sol (Investor Interface)      │
├─────────────────────────────────────┤
│ • Process new deposits in           │
│   supported tokens (USDC/USDT/DAI)  │
│ • Handle withdrawal requests with   │
│   7-day delay for risk management   │
│ • Distribute performance fees to    │
│   operators and protocol            │
│ • Update investor share tokens      │
│ • Provide real-time performance     │
│   metrics and transparency          │
└─────────────────────────────────────┘

┌─────────────────────────────────────┐
│ CONTINUOUS MONITORING               │
├─────────────────────────────────────┤
│ • Funding rate tracking             │
│ • Liquidity monitoring              │
│ • Risk metric alerts                │
│ • Performance analytics             │
│ • Emergency circuit breakers        │
└─────────────────────────────────────┘

KEY IMPLEMENTATION DETAILS:
═══════════════════════════

• EPOCH FREQUENCY: 4-hour intervals
• RISK CONSTRAINTS: Immutable, enforced in zkVM
• DELTA NEUTRALITY: Maintained via coordinated hedging
• PROOF VERIFICATION: RISC Zero + Boundless Market
• STATE PERSISTENCE: On-chain via Settlement contract
• EMERGENCY HALTS: Automatic on constraint violations
• VENUE SUPPORT: Binance (CEX) + Uniswap V3 (DEX)

CAPITAL MANAGEMENT:
═══════════════════

• VAULT DEPOSITS: Multi-token support (USDC, USDT, DAI)
• AGENT ALLOCATION: Maximum 50% of vault per agent
• WITHDRAWAL DELAY: 7-day minimum for risk management
• PERFORMANCE FEES: Calculated on high-water mark basis
• MANAGEMENT FEES: Annual fees in basis points
• CAPITAL LIMITS: Enforced via AgentRegistry validation

GOVERNANCE & SECURITY:
═════════════════════

• AGENT REGISTRATION: Mandatory via AgentRegistry
• MANDATE VERIFICATION: Strategy code hash validation
• OPERATOR PERMISSIONS: Role-based access control
• EMERGENCY PROCEDURES: Multi-layered circuit breakers
• AUDIT TRAIL: Complete on-chain transaction history
• INVESTOR PROTECTION: Automated risk monitoring
```

⸻

## Use Case Example — End-to-End Agent Workflow

### Market conditions (arbitrary example):
- ETH spot price: $2,500
- ETH perpetual price: $2,508
- Funding rate: +0.015% every 8 hours (~16.4% annualized)
- Liquidity: sufficient on Binance (perp) and Uniswap v3 (spot)

### Step-by-step workflow:
1. Observation (off-chain)
The host collects market data and constructs canonical inputs: prices, funding rate, margin requirements, and current vault exposure.
2. Decision (guest execution)
The RISC0 guest program:
    - Runs a linear regression scoring funding persistence
    - Verifies funding exceeds the minimum threshold
    - Computes a delta-neutral position size capped at 2× leverage and ≤1% net delta
3. Proof generation (Boundless)
Boundless generates a zk proof that:
    - The fixed strategy code hash was used
    - The model output was correctly computed
    - All risk constraints were satisfied
4. Execution (optimistic)
A relayer opens:
    - Long ETH spot
    - Short ETH perpetual
according to the proven allocation.
5. Settlement (on-chain)
The proof is submitted on-chain to authorize the epoch state update; funding accrues over time.
6. Exit condition
When funding drops below threshold or liquidity degrades, the next epoch proof authorizes a full unwind.

⸻

## Project Structure (RISC0 + Boundless–Aligned)
```
vrbca-agent/
├── README.md
├── Cargo.toml
├── rust-toolchain.toml
│
├── methods/                         # RISC0 guest programs
│   ├── Cargo.toml
│   └── guest/
│       ├── Cargo.toml
│       └── src/
│           └── main.rs              # Guest entrypoint (strategy + risk verification)
│
├── host/                            # Off-chain orchestrator
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs                  # Epoch runner
│       ├── inputs.rs                # Market data normalization
│       ├── model.rs                 # Linear regression (mirrors guest logic)
│       ├── executor.rs              # Relayer coordination
│       └── prover.rs                # Boundless proof submission
│
├── core/
│   ├── strategy.rs                  # Basis & funding logic (shared)
│   ├── risk.rs                      # Deterministic constraints (shared)
│   ├── mandate.rs                   # Immutable mandate definition
│   └── state.rs                     # Positions, exposure, PnL
│
├── contracts/
│   ├── Vault.sol                    # Deposits / withdrawals
│   ├── AgentRegistry.sol            # Agent + mandate registration
│   ├── RiscZeroVerifier.sol         # zk proof verification
│   └── Settlement.sol               # Epoch settlement & accounting
│
├── config/
│   ├── risk.yaml                    # Leverage, delta, drawdown limits
│   ├── markets.yaml                 # Approved assets & venues
│   └── epochs.yaml                  # Proof cadence
│
└── scripts/
    ├── simulate_epoch.rs            # Dry-run guest execution
    ├── submit_proof.rs              # On-chain proof submission
    └── emergency_unwind.rs          # Forced unwind path

```

## Guest Inputs (Public + Private Witness)

### Public Inputs (Committed)

These are hash-committed and referenced on-chain.

```
struct PublicInputs {
    epoch_id: u64,
    agent_id: [u8; 32],
    mandate_hash: [u8; 32],
    prev_state_root: [u8; 32],
}
```

### Private Witness Inputs

These are not revealed, only proven.
```
struct PrivateInputs {
    // Market data
    spot_price: u128,
    perp_price: u128,
    funding_rate: i128,
    funding_interval_secs: u64,
    liquidity_score: u64,

    // Model inputs
    model_weights: Vec<i128>,
    model_features: Vec<i128>,

    // Previous state
    positions: Vec<Position>,
    margin_used: u128,
    unrealized_pnl: i128,

    // Execution report (Binance)
    execution_reports: Vec<ExecutionReport>,

    // Risk config
    max_leverage: u128,
    max_delta: u128,
    max_drawdown: u128,
}
```
### Execution Report Format (Binance)
```
struct ExecutionReport {
    venue_id: u8, // e.g. 1 = Binance
    symbol: [u8; 16],
    side: Side,
    quantity: u128,
    price: u128,
    timestamp: u64,
    order_id_hash: [u8; 32],
    venue_signature: [u8; 65], // ECDSA
}
```
Inside the guest:
- Signatures are verified
- Order hashes are validated
- Net position delta is recomputed

## Guest Logic (What Is Proven)

Inside guest/main.rs, the zkVM proves that:
1. Strategy code hash matches the registered mandate
2. Linear regression output was computed correctly
3. Funding threshold condition was satisfied
4. Net delta ≤ max_delta
5. Leverage ≤ max_leverage
6. Drawdown ≤ max_drawdown
7. State transition is consistent

No external trust is required for these claims.


## Journal Outputs (On-Chain Visible)

This is all the chain ever sees.
```
struct Journal {
    epoch_id: u64,
    new_state_root: [u8; 32],
    net_delta: i128,
    leverage: u128,
    realized_pnl: i128,
    positions_commitment: [u8; 32],
    execution_commitment: [u8; 32],
    halt_flag: bool,
}
```
## Settlement Contract Responsibilities

The settlement contract:
- Verifies the RISC0 proof
- Checks mandate_hash consistency
- Advances the agent state root
- Releases or freezes capital
- Emits events for transparency
- Triggers halt/unwind if halt_flag == true

It never needs to know Binance internals.

## Architectural Notes 
- Guest program is minimal, deterministic, and fully reproducible
- Host program is untrusted but economically constrained
- Relayers are bonded and slashable
- On-chain contracts enforce continuation rules, not strategy logic
- Proofs are epoch-based, not per-trade, ensuring scalability

## Strategic takeaway

This agent is valuable not because it trades better than humans, but because it proves—cryptographically—that it never trades outside its mandate.