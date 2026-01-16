// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.20;

import {IERC20} from "openzeppelin-contracts/contracts/token/ERC20/IERC20.sol";
import {SafeERC20} from "openzeppelin-contracts/contracts/token/ERC20/utils/SafeERC20.sol";
import {ReentrancyGuard} from "openzeppelin-contracts/contracts/utils/ReentrancyGuard.sol";
import {Ownable} from "openzeppelin-contracts/contracts/access/Ownable.sol";

/// @title VRBCA Vault Contract
/// @notice Manages deposits, withdrawals, and capital allocation for VRBCA agents
/// @dev Handles investor funds and tracks performance across multiple agents
contract Vault is Ownable, ReentrancyGuard {
    using SafeERC20 for IERC20;

    /// @notice Agent registry contract
    address public agentRegistry;
    
    /// @notice Settlement contract
    address public settlementContract;

    /// @notice Supported deposit tokens (e.g., USDC, USDT, DAI)
    mapping(address => bool) public supportedTokens;
    
    /// @notice Agent allocations (agentId => allocated capital in USD)
    mapping(bytes32 => uint256) public agentAllocations;
    
    /// @notice User deposits per token
    mapping(address => mapping(address => uint256)) public userDeposits; // user => token => amount
    
    /// @notice Total deposits per token
    mapping(address => uint256) public totalDeposits;
    
    /// @notice Agent performance tracking
    mapping(bytes32 => AgentPerformance) public agentPerformance;
    
    /// @notice Withdrawal requests
    mapping(address => WithdrawalRequest) public withdrawalRequests;
    
    /// @notice Fee configuration
    struct FeeConfig {
        uint256 managementFeeBps; // Annual management fee in basis points
        uint256 performanceFeeBps; // Performance fee in basis points
        uint256 withdrawalFeeBps; // Withdrawal fee in basis points
        address feeRecipient; // Address to receive fees
    }
    
    /// @notice Agent performance data
    struct AgentPerformance {
        uint256 totalPnl; // Cumulative PnL in USD
        uint256 highWaterMark; // Highest portfolio value achieved
        uint256 currentValue; // Current portfolio value
        uint256 lastUpdateTime; // Last performance update timestamp
        uint256 sharpeRatio; // Sharpe ratio * 10000 for precision
        uint256 maxDrawdown; // Maximum drawdown experienced
    }
    
    /// @notice Withdrawal request data
    struct WithdrawalRequest {
        uint256 amount; // Amount to withdraw in USD
        address token; // Preferred withdrawal token
        uint256 requestTime; // When withdrawal was requested
        bool isPending; // Whether request is still pending
    }

    /// @notice Current fee configuration
    FeeConfig public feeConfig;
    
    /// @notice Minimum withdrawal delay (7 days)
    uint256 public constant MIN_WITHDRAWAL_DELAY = 7 days;
    
    /// @notice Maximum single allocation (50% of vault)
    uint256 public constant MAX_ALLOCATION_BPS = 5000;
    
    /// @notice Total vault value in USD (with 6 decimal precision)
    uint256 public totalVaultValue;

    /// Events
    event Deposited(
        address indexed user,
        address indexed token,
        uint256 amount,
        uint256 usdValue
    );

    event WithdrawalRequested(
        address indexed user,
        uint256 amount,
        address token,
        uint256 requestTime
    );

    event WithdrawalExecuted(
        address indexed user,
        uint256 amount,
        address token
    );

    event AgentAllocation(
        bytes32 indexed agentId,
        uint256 newAllocation,
        uint256 previousAllocation
    );

    event PerformanceUpdated(
        bytes32 indexed agentId,
        uint256 newValue,
        int256 pnl,
        uint256 sharpeRatio
    );

    event FeeConfigUpdated(
        uint256 managementFeeBps,
        uint256 performanceFeeBps,
        uint256 withdrawalFeeBps
    );

    /// Errors
    error TokenNotSupported();
    error InsufficientFunds();
    error AllocationTooLarge();
    error WithdrawalNotReady();
    error InvalidAgent();
    error UnauthorizedUpdate();
    error InvalidFeeConfiguration();

    constructor(
        address _agentRegistry,
        address _settlementContract,
        address _feeRecipient
    ) Ownable(msg.sender) {
        agentRegistry = _agentRegistry;
        settlementContract = _settlementContract;
        
        feeConfig = FeeConfig({
            managementFeeBps: 200, // 2% annual
            performanceFeeBps: 2000, // 20% of profits
            withdrawalFeeBps: 50, // 0.5% withdrawal fee
            feeRecipient: _feeRecipient
        });
    }

    /// @notice Add a supported deposit token
    /// @param token Token address to support
    function addSupportedToken(address token) external onlyOwner {
        require(token != address(0), "Invalid token address");
        supportedTokens[token] = true;
    }

    /// @notice Remove a supported deposit token
    /// @param token Token address to remove
    function removeSupportedToken(address token) external onlyOwner {
        supportedTokens[token] = false;
    }

    /// @notice Deposit funds into the vault
    /// @param token The token to deposit
    /// @param amount The amount to deposit
    function deposit(address token, uint256 amount) external nonReentrant {
        if (!supportedTokens[token]) {
            revert TokenNotSupported();
        }
        
        require(amount > 0, "Deposit amount must be positive");

        // Transfer tokens from user
        IERC20(token).safeTransferFrom(msg.sender, address(this), amount);
        
        // Convert to USD value (assuming 6 decimal stablecoins like USDC)
        uint256 usdValue = amount; // Simplified: assume 1:1 with USD
        
        // Update user and total deposits
        userDeposits[msg.sender][token] += amount;
        totalDeposits[token] += amount;
        totalVaultValue += usdValue;

        emit Deposited(msg.sender, token, amount, usdValue);
    }

    /// @notice Request withdrawal from vault
    /// @param amount Amount to withdraw in USD
    /// @param token Preferred token for withdrawal
    function requestWithdrawal(uint256 amount, address token) external {
        if (!supportedTokens[token]) {
            revert TokenNotSupported();
        }

        require(amount > 0, "Withdrawal amount must be positive");
        
        // Check user has sufficient balance
        uint256 userBalance = getUserTotalBalance(msg.sender);
        if (userBalance < amount) {
            revert InsufficientFunds();
        }

        // Create withdrawal request
        withdrawalRequests[msg.sender] = WithdrawalRequest({
            amount: amount,
            token: token,
            requestTime: block.timestamp,
            isPending: true
        });

        emit WithdrawalRequested(msg.sender, amount, token, block.timestamp);
    }

    /// @notice Execute pending withdrawal after delay period
    function executeWithdrawal() external nonReentrant {
        WithdrawalRequest storage request = withdrawalRequests[msg.sender];
        
        require(request.isPending, "No pending withdrawal");
        
        if (block.timestamp < request.requestTime + MIN_WITHDRAWAL_DELAY) {
            revert WithdrawalNotReady();
        }

        uint256 amount = request.amount;
        address token = request.token;
        
        // Calculate withdrawal fee
        uint256 fee = (amount * feeConfig.withdrawalFeeBps) / 10000;
        uint256 netAmount = amount - fee;

        // Check vault has sufficient balance
        require(totalDeposits[token] >= netAmount, "Insufficient vault balance");

        // Update state
        userDeposits[msg.sender][token] -= amount;
        totalDeposits[token] -= netAmount;
        totalVaultValue -= amount;
        
        // Clear withdrawal request
        request.isPending = false;

        // Transfer tokens to user
        IERC20(token).safeTransfer(msg.sender, netAmount);
        
        // Transfer fee to fee recipient
        if (fee > 0) {
            IERC20(token).safeTransfer(feeConfig.feeRecipient, fee);
        }

        emit WithdrawalExecuted(msg.sender, netAmount, token);
    }

    /// @notice Allocate capital to an agent (owner only)
    /// @param agentId The agent to allocate capital to
    /// @param amount Amount to allocate in USD
    function allocateToAgent(bytes32 agentId, uint256 amount) external onlyOwner {
        // Validate agent exists and is active
        // This would call the agent registry in production
        require(agentId != bytes32(0), "Invalid agent ID");
        
        // Check allocation doesn't exceed limits
        if (amount > (totalVaultValue * MAX_ALLOCATION_BPS) / 10000) {
            revert AllocationTooLarge();
        }

        uint256 previousAllocation = agentAllocations[agentId];
        agentAllocations[agentId] = amount;

        emit AgentAllocation(agentId, amount, previousAllocation);
    }

    /// @notice Update agent performance (called by settlement contract)
    /// @param agentId The agent whose performance to update
    /// @param newValue New portfolio value
    /// @param realizedPnl Realized PnL from latest epoch
    function updateAgentPerformance(
        bytes32 agentId,
        uint256 newValue,
        int256 realizedPnl
    ) external {
        require(msg.sender == settlementContract, "Unauthorized caller");

        AgentPerformance storage perf = agentPerformance[agentId];
        
        // Update PnL
        if (realizedPnl >= 0) {
            perf.totalPnl += uint256(realizedPnl);
        } else {
            if (perf.totalPnl >= uint256(-realizedPnl)) {
                perf.totalPnl -= uint256(-realizedPnl);
            } else {
                perf.totalPnl = 0;
            }
        }

        // Update current value
        perf.currentValue = newValue;
        
        // Update high water mark
        if (newValue > perf.highWaterMark) {
            perf.highWaterMark = newValue;
        }

        // Calculate and update drawdown
        if (perf.highWaterMark > 0) {
            uint256 currentDrawdown = ((perf.highWaterMark - newValue) * 10000) / perf.highWaterMark;
            if (currentDrawdown > perf.maxDrawdown) {
                perf.maxDrawdown = currentDrawdown;
            }
        }

        // Update Sharpe ratio (simplified calculation)
        perf.sharpeRatio = _calculateSharpeRatio(agentId);
        perf.lastUpdateTime = block.timestamp;

        emit PerformanceUpdated(agentId, newValue, realizedPnl, perf.sharpeRatio);
    }

    /// @notice Update fee configuration (owner only)
    /// @param managementFeeBps Annual management fee in basis points
    /// @param performanceFeeBps Performance fee in basis points
    /// @param withdrawalFeeBps Withdrawal fee in basis points
    function updateFeeConfig(
        uint256 managementFeeBps,
        uint256 performanceFeeBps,
        uint256 withdrawalFeeBps
    ) external onlyOwner {
        if (managementFeeBps > 1000 || // Max 10% management fee
            performanceFeeBps > 5000 || // Max 50% performance fee
            withdrawalFeeBps > 1000) {  // Max 10% withdrawal fee
            revert InvalidFeeConfiguration();
        }

        feeConfig.managementFeeBps = managementFeeBps;
        feeConfig.performanceFeeBps = performanceFeeBps;
        feeConfig.withdrawalFeeBps = withdrawalFeeBps;

        emit FeeConfigUpdated(managementFeeBps, performanceFeeBps, withdrawalFeeBps);
    }

    /// @notice Get user's total balance across all tokens (in USD)
    /// @param user The user address
    /// @return totalBalance Total balance in USD
    function getUserTotalBalance(address user) public view returns (uint256) {
        uint256 totalBalance = 0;
        
        // In production, would iterate through all supported tokens
        // For now, simplified implementation
        
        return totalBalance;
    }

    /// @notice Get vault performance metrics
    /// @return totalValue Total vault value
    /// @return totalPnl Total PnL across all agents
    /// @return avgSharpeRatio Average Sharpe ratio
    function getVaultMetrics() external view returns (
        uint256 totalValue,
        uint256 totalPnl,
        uint256 avgSharpeRatio
    ) {
        // Implementation would aggregate across all agents
        totalValue = totalVaultValue;
        totalPnl = 0; // Calculate from all agents
        avgSharpeRatio = 0; // Calculate weighted average
    }

    /// @notice Get agent allocation and performance
    /// @param agentId The agent identifier
    /// @return allocation Current allocation
    /// @return performance Performance data
    function getAgentInfo(bytes32 agentId) external view returns (
        uint256 allocation,
        AgentPerformance memory performance
    ) {
        return (agentAllocations[agentId], agentPerformance[agentId]);
    }

    /// @notice Calculate Sharpe ratio for an agent (simplified)
    /// @param agentId The agent identifier
    /// @return sharpeRatio Sharpe ratio * 10000
    function _calculateSharpeRatio(bytes32 agentId) internal view returns (uint256) {
        // Simplified Sharpe ratio calculation
        // In production, would use historical returns and volatility
        AgentPerformance memory perf = agentPerformance[agentId];
        
        if (perf.currentValue == 0) {
            return 0;
        }
        
        // Return simplified metric based on PnL and drawdown
        if (perf.maxDrawdown == 0) {
            return 10000; // Perfect score if no drawdown
        }
        
        return (perf.totalPnl * 10000) / perf.maxDrawdown;
    }
}