// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.20;

import {IRiscZeroVerifier} from "risc0/IRiscZeroVerifier.sol";
import {ImageID} from "./ImageID.sol";
import {Ownable} from "openzeppelin-contracts/contracts/access/Ownable.sol";

/// @title VRBCA Settlement Contract
/// @notice Handles epoch-based settlement and state transitions for the Verifiable Risk-Bound Basis Capture Agent
/// @dev This contract enforces continuation rules and validates zero-knowledge proofs from the agent
contract Settlement is Ownable {
    /// @notice RISC Zero verifier contract address
    IRiscZeroVerifier public immutable VERIFIER;
    
    /// @notice Image ID of the VRBCA guest program
    bytes32 public imageId;

    /// @notice Agent registry contract
    address public agentRegistry;

    /// @notice Current epoch for each agent
    mapping(bytes32 => uint64) public agentCurrentEpoch;
    
    /// @notice State root for each agent at each epoch
    mapping(bytes32 => mapping(uint64 => bytes32)) public agentStateRoots;
    
    /// @notice Agent halt status
    mapping(bytes32 => bool) public agentHalted;
    
    /// @notice Last settlement timestamp for each agent
    mapping(bytes32 => uint256) public agentLastSettlement;

    /// @notice Maximum time allowed between settlements (24 hours)
    uint256 public constant MAX_SETTLEMENT_INTERVAL = 24 hours;

    /// @notice Agent epoch data
    struct EpochData {
        uint64 epochId;
        bytes32 stateRoot;
        int128 netDelta;
        uint128 leverage;
        int128 realizedPnl;
        bytes32 positionsCommitment;
        bytes32 executionCommitment;
        bool haltFlag;
        uint256 timestamp;
    }

    /// @notice Agent epoch data storage
    mapping(bytes32 => mapping(uint64 => EpochData)) public agentEpochData;

    /// Events
    event EpochSettled(
        bytes32 indexed agentId,
        uint64 indexed epochId,
        bytes32 stateRoot,
        int128 netDelta,
        uint128 leverage,
        int128 realizedPnl,
        bool haltFlag
    );

    event AgentHalted(bytes32 indexed agentId, uint64 epochId, string reason);
    event AgentResumed(bytes32 indexed agentId, uint64 epochId);
    event ImageIdUpdated(bytes32 newImageId);

    /// Errors
    error InvalidAgent();
    error AgentAlreadyHalted();
    error InvalidEpoch();
    error ProofVerificationFailed();
    error UnauthorizedCaller();
    error SettlementTooFrequent();
    error SettlementOverdue();

    constructor(IRiscZeroVerifier _verifier, address _agentRegistry) Ownable(msg.sender) {
        VERIFIER = _verifier;
        agentRegistry = _agentRegistry;
        imageId = ImageID.VRBCA_ID; // Use VRBCA image ID
    }

    /// @notice Set the image ID for the VRBCA guest program
    /// @param _imageId The new image ID
    function setImageId(bytes32 _imageId) external onlyOwner {
        require(_imageId != bytes32(0), "Invalid image ID");
        imageId = _imageId;
        emit ImageIdUpdated(_imageId);
    }

    /// @notice Set the agent registry contract
    /// @param _agentRegistry The agent registry contract address
    function setAgentRegistry(address _agentRegistry) external onlyOwner {
        require(_agentRegistry != address(0), "Invalid agent registry");
        agentRegistry = _agentRegistry;
    }

    /// @notice Submit epoch settlement with zero-knowledge proof
    /// @param agentId The agent identifier
    /// @param epochId The epoch being settled
    /// @param stateRoot The new state root after epoch execution
    /// @param netDelta The net delta exposure in basis points
    /// @param leverage The current leverage in basis points
    /// @param realizedPnl The realized PnL from this epoch
    /// @param positionsCommitment Hash commitment of current positions
    /// @param executionCommitment Hash commitment of execution reports
    /// @param haltFlag Whether the agent should halt
    /// @param seal The RISC Zero proof seal
    function settleEpoch(
        bytes32 agentId,
        uint64 epochId,
        bytes32 stateRoot,
        int128 netDelta,
        uint128 leverage,
        int128 realizedPnl,
        bytes32 positionsCommitment,
        bytes32 executionCommitment,
        bool haltFlag,
        bytes calldata seal
    ) external {
        // Validate agent exists and caller is authorized
        _validateAgent(agentId);
        
        // Check if agent is halted
        if (agentHalted[agentId]) {
            revert AgentAlreadyHalted();
        }

        // Validate epoch sequence
        uint64 currentEpoch = agentCurrentEpoch[agentId];
        if (epochId != currentEpoch + 1) {
            revert InvalidEpoch();
        }

        // Check settlement timing
        _validateSettlementTiming(agentId);

        // Get previous state root
        bytes32 prevStateRoot = agentStateRoots[agentId][currentEpoch];

        // Construct journal for proof verification
        bytes memory journal = abi.encode(
            epochId,
            stateRoot,
            netDelta,
            leverage,
            realizedPnl,
            positionsCommitment,
            executionCommitment,
            haltFlag,
            prevStateRoot,
            agentId
        );

        // Verify the zero-knowledge proof
        try VERIFIER.verify(seal, imageId, sha256(journal)) {
            // Proof verified successfully
        } catch {
            revert ProofVerificationFailed();
        }

        // Update agent state
        agentCurrentEpoch[agentId] = epochId;
        agentStateRoots[agentId][epochId] = stateRoot;
        agentLastSettlement[agentId] = block.timestamp;

        // Store epoch data
        agentEpochData[agentId][epochId] = EpochData({
            epochId: epochId,
            stateRoot: stateRoot,
            netDelta: netDelta,
            leverage: leverage,
            realizedPnl: realizedPnl,
            positionsCommitment: positionsCommitment,
            executionCommitment: executionCommitment,
            haltFlag: haltFlag,
            timestamp: block.timestamp
        });

        // Handle halt flag
        if (haltFlag) {
            agentHalted[agentId] = true;
            emit AgentHalted(agentId, epochId, "Proof indicated halt condition");
        }

        emit EpochSettled(
            agentId,
            epochId,
            stateRoot,
            netDelta,
            leverage,
            realizedPnl,
            haltFlag
        );
    }

    /// @notice Emergency halt an agent (owner only)
    /// @param agentId The agent to halt
    /// @param reason The reason for halting
    function emergencyHalt(bytes32 agentId, string calldata reason) external onlyOwner {
        _validateAgent(agentId);
        
        if (agentHalted[agentId]) {
            revert AgentAlreadyHalted();
        }

        agentHalted[agentId] = true;
        emit AgentHalted(agentId, agentCurrentEpoch[agentId], reason);
    }

    /// @notice Resume a halted agent (owner only)
    /// @param agentId The agent to resume
    function resumeAgent(bytes32 agentId) external onlyOwner {
        _validateAgent(agentId);
        
        require(agentHalted[agentId], "Agent not halted");
        
        agentHalted[agentId] = false;
        emit AgentResumed(agentId, agentCurrentEpoch[agentId]);
    }

    /// @notice Force settlement if agent is overdue (anyone can call)
    /// @param agentId The agent that is overdue
    function forceSettlement(bytes32 agentId) external {
        _validateAgent(agentId);
        
        require(
            block.timestamp > agentLastSettlement[agentId] + MAX_SETTLEMENT_INTERVAL,
            "Agent not overdue"
        );

        // Halt the agent due to missed settlement
        agentHalted[agentId] = true;
        emit AgentHalted(agentId, agentCurrentEpoch[agentId], "Missed settlement deadline");
    }

    /// @notice Get current epoch data for an agent
    /// @param agentId The agent identifier
    /// @return epochData The current epoch data
    function getCurrentEpochData(bytes32 agentId) external view returns (EpochData memory) {
        uint64 currentEpoch = agentCurrentEpoch[agentId];
        return agentEpochData[agentId][currentEpoch];
    }

    /// @notice Get epoch data for a specific epoch
    /// @param agentId The agent identifier
    /// @param epochId The epoch identifier
    /// @return epochData The epoch data
    function getEpochData(bytes32 agentId, uint64 epochId) external view returns (EpochData memory) {
        return agentEpochData[agentId][epochId];
    }

    /// @notice Check if an agent is operational
    /// @param agentId The agent identifier
    /// @return isOperational True if agent is operational
    function isAgentOperational(bytes32 agentId) external view returns (bool) {
        return !agentHalted[agentId] && 
               block.timestamp <= agentLastSettlement[agentId] + MAX_SETTLEMENT_INTERVAL;
    }

    /// @notice Get agent risk metrics from latest epoch
    /// @param agentId The agent identifier
    /// @return netDelta The current net delta
    /// @return leverage The current leverage
    /// @return isHalted Whether the agent is halted
    function getAgentRiskMetrics(bytes32 agentId) external view returns (
        int128 netDelta,
        uint128 leverage,
        bool isHalted
    ) {
        uint64 currentEpoch = agentCurrentEpoch[agentId];
        EpochData memory epochData = agentEpochData[agentId][currentEpoch];
        
        return (epochData.netDelta, epochData.leverage, agentHalted[agentId]);
    }

    /// @notice Validate that an agent exists and caller is authorized
    function _validateAgent(bytes32 agentId) internal view {
        // In production, this would check the agent registry
        // For now, just check that agentId is not zero
        if (agentId == bytes32(0)) {
            revert InvalidAgent();
        }
        
        // TODO: Add agent registry validation
        // require(IAgentRegistry(agentRegistry).isValidAgent(agentId), "Invalid agent");
    }

    /// @notice Validate settlement timing constraints
    function _validateSettlementTiming(bytes32 agentId) internal view {
        uint256 lastSettlement = agentLastSettlement[agentId];
        
        // Ensure not settling too frequently (minimum 1 hour between settlements)
        if (lastSettlement != 0 && block.timestamp < lastSettlement + 1 hours) {
            revert SettlementTooFrequent();
        }
        
        // Ensure not overdue (if this is not the first settlement)
        if (lastSettlement != 0 && block.timestamp > lastSettlement + MAX_SETTLEMENT_INTERVAL) {
            revert SettlementOverdue();
        }
    }
}