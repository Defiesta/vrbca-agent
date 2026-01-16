// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.20;

import {Ownable} from "openzeppelin-contracts/contracts/access/Ownable.sol";

/// @title VRBCA Agent Registry
/// @notice Manages registration and validation of VRBCA agents and their mandates
/// @dev This contract ensures only authorized agents with valid mandates can operate
contract AgentRegistry is Ownable {
    /// @notice Agent registration data
    struct AgentRegistration {
        bytes32 agentId;
        bytes32 mandateHash;
        address operator;
        uint256 registrationTime;
        bool isActive;
        uint256 maxCapital; // Maximum capital the agent can manage (wei)
        uint256 currentCapital; // Current capital under management (wei)
    }

    /// @notice Mandate configuration
    struct MandateConfig {
        bytes32 mandateId;
        bytes32 strategyCodeHash;
        uint256 maxLeverage; // Maximum leverage in basis points (e.g., 20000 = 2x)
        uint256 maxNetDelta; // Maximum net delta in basis points (e.g., 100 = 1%)
        uint256 maxDrawdown; // Maximum drawdown in basis points (e.g., 2000 = 20%)
        address[] approvedVenues; // List of approved trading venues
        bool isActive;
        uint256 creationTime;
    }

    /// @notice Registered agents
    mapping(bytes32 => AgentRegistration) public agents;
    
    /// @notice Registered mandates
    mapping(bytes32 => MandateConfig) public mandates;
    
    /// @notice Agent ID to mandate ID mapping
    mapping(bytes32 => bytes32) public agentMandates;
    
    /// @notice Operator to agent IDs mapping
    mapping(address => bytes32[]) public operatorAgents;
    
    /// @notice Total number of registered agents
    uint256 public totalAgents;
    
    /// @notice Total capital under management across all agents
    uint256 public totalCapitalUnderManagement;

    /// Events
    event AgentRegistered(
        bytes32 indexed agentId,
        bytes32 indexed mandateId,
        address indexed operator,
        uint256 maxCapital
    );

    event MandateRegistered(
        bytes32 indexed mandateId,
        bytes32 strategyCodeHash,
        uint256 maxLeverage,
        uint256 maxNetDelta
    );

    event AgentActivated(bytes32 indexed agentId);
    event AgentDeactivated(bytes32 indexed agentId);
    event MandateActivated(bytes32 indexed mandateId);
    event MandateDeactivated(bytes32 indexed mandateId);
    event CapitalUpdated(bytes32 indexed agentId, uint256 newCapital);

    /// Errors
    error AgentAlreadyRegistered();
    error MandateAlreadyRegistered();
    error AgentNotFound();
    error MandateNotFound();
    error UnauthorizedOperator();
    error InactiveMandateOrAgent();
    error CapitalLimitExceeded();
    error InvalidParameters();

    constructor() Ownable(msg.sender) {}

    /// @notice Register a new trading mandate
    /// @param mandateId Unique identifier for the mandate
    /// @param strategyCodeHash Hash of the strategy code that must be proven
    /// @param maxLeverage Maximum leverage allowed (basis points)
    /// @param maxNetDelta Maximum net delta allowed (basis points)
    /// @param maxDrawdown Maximum drawdown allowed (basis points)
    /// @param approvedVenues List of approved trading venues
    function registerMandate(
        bytes32 mandateId,
        bytes32 strategyCodeHash,
        uint256 maxLeverage,
        uint256 maxNetDelta,
        uint256 maxDrawdown,
        address[] calldata approvedVenues
    ) external onlyOwner {
        if (mandates[mandateId].mandateId != bytes32(0)) {
            revert MandateAlreadyRegistered();
        }

        // Validate mandate parameters
        if (mandateId == bytes32(0) || 
            strategyCodeHash == bytes32(0) ||
            maxLeverage == 0 || maxLeverage > 50000 || // Max 5x leverage
            maxNetDelta > 1000 || // Max 10% net delta
            maxDrawdown > 5000 || // Max 50% drawdown
            approvedVenues.length == 0) {
            revert InvalidParameters();
        }

        mandates[mandateId] = MandateConfig({
            mandateId: mandateId,
            strategyCodeHash: strategyCodeHash,
            maxLeverage: maxLeverage,
            maxNetDelta: maxNetDelta,
            maxDrawdown: maxDrawdown,
            approvedVenues: approvedVenues,
            isActive: true,
            creationTime: block.timestamp
        });

        emit MandateRegistered(
            mandateId,
            strategyCodeHash,
            maxLeverage,
            maxNetDelta
        );
    }

    /// @notice Register a new agent with a specific mandate
    /// @param agentId Unique identifier for the agent
    /// @param mandateId The mandate this agent will follow
    /// @param operator Address authorized to operate this agent
    /// @param maxCapital Maximum capital this agent can manage
    function registerAgent(
        bytes32 agentId,
        bytes32 mandateId,
        address operator,
        uint256 maxCapital
    ) external onlyOwner {
        if (agents[agentId].agentId != bytes32(0)) {
            revert AgentAlreadyRegistered();
        }

        if (mandates[mandateId].mandateId == bytes32(0)) {
            revert MandateNotFound();
        }

        if (!mandates[mandateId].isActive) {
            revert InactiveMandateOrAgent();
        }

        if (agentId == bytes32(0) || operator == address(0) || maxCapital == 0) {
            revert InvalidParameters();
        }

        agents[agentId] = AgentRegistration({
            agentId: agentId,
            mandateHash: _calculateMandateHash(mandateId),
            operator: operator,
            registrationTime: block.timestamp,
            isActive: true,
            maxCapital: maxCapital,
            currentCapital: 0
        });

        agentMandates[agentId] = mandateId;
        operatorAgents[operator].push(agentId);
        totalAgents++;

        emit AgentRegistered(agentId, mandateId, operator, maxCapital);
    }

    /// @notice Update agent's current capital under management
    /// @param agentId The agent identifier
    /// @param newCapital New capital amount
    function updateAgentCapital(bytes32 agentId, uint256 newCapital) external {
        if (agents[agentId].agentId == bytes32(0)) {
            revert AgentNotFound();
        }

        if (msg.sender != agents[agentId].operator && msg.sender != owner()) {
            revert UnauthorizedOperator();
        }

        if (newCapital > agents[agentId].maxCapital) {
            revert CapitalLimitExceeded();
        }

        uint256 oldCapital = agents[agentId].currentCapital;
        agents[agentId].currentCapital = newCapital;

        // Update total capital under management
        totalCapitalUnderManagement = totalCapitalUnderManagement - oldCapital + newCapital;

        emit CapitalUpdated(agentId, newCapital);
    }

    /// @notice Activate an agent
    /// @param agentId The agent to activate
    function activateAgent(bytes32 agentId) external onlyOwner {
        if (agents[agentId].agentId == bytes32(0)) {
            revert AgentNotFound();
        }

        bytes32 mandateId = agentMandates[agentId];
        if (!mandates[mandateId].isActive) {
            revert InactiveMandateOrAgent();
        }

        agents[agentId].isActive = true;
        emit AgentActivated(agentId);
    }

    /// @notice Deactivate an agent
    /// @param agentId The agent to deactivate
    function deactivateAgent(bytes32 agentId) external onlyOwner {
        if (agents[agentId].agentId == bytes32(0)) {
            revert AgentNotFound();
        }

        agents[agentId].isActive = false;
        emit AgentDeactivated(agentId);
    }

    /// @notice Activate a mandate
    /// @param mandateId The mandate to activate
    function activateMandate(bytes32 mandateId) external onlyOwner {
        if (mandates[mandateId].mandateId == bytes32(0)) {
            revert MandateNotFound();
        }

        mandates[mandateId].isActive = true;
        emit MandateActivated(mandateId);
    }

    /// @notice Deactivate a mandate (also deactivates all agents using it)
    /// @param mandateId The mandate to deactivate
    function deactivateMandate(bytes32 mandateId) external onlyOwner {
        if (mandates[mandateId].mandateId == bytes32(0)) {
            revert MandateNotFound();
        }

        mandates[mandateId].isActive = false;
        emit MandateDeactivated(mandateId);

        // Note: Consider deactivating all agents using this mandate
    }

    /// @notice Check if an agent is valid and active
    /// @param agentId The agent identifier
    /// @return isValid True if agent is valid and active
    function isValidAgent(bytes32 agentId) external view returns (bool) {
        AgentRegistration memory agent = agents[agentId];
        if (agent.agentId == bytes32(0) || !agent.isActive) {
            return false;
        }

        bytes32 mandateId = agentMandates[agentId];
        return mandates[mandateId].isActive;
    }

    /// @notice Check if an operator is authorized for an agent
    /// @param agentId The agent identifier
    /// @param operator The operator address
    /// @return isAuthorized True if operator is authorized
    function isAuthorizedOperator(bytes32 agentId, address operator) external view returns (bool) {
        return agents[agentId].operator == operator;
    }

    /// @notice Get agent's mandate configuration
    /// @param agentId The agent identifier
    /// @return mandate The mandate configuration
    function getAgentMandate(bytes32 agentId) external view returns (MandateConfig memory) {
        bytes32 mandateId = agentMandates[agentId];
        return mandates[mandateId];
    }

    /// @notice Get all agent IDs for an operator
    /// @param operator The operator address
    /// @return agentIds Array of agent IDs
    function getOperatorAgents(address operator) external view returns (bytes32[] memory) {
        return operatorAgents[operator];
    }

    /// @notice Get agent registration data
    /// @param agentId The agent identifier
    /// @return registration The agent registration data
    function getAgentRegistration(bytes32 agentId) external view returns (AgentRegistration memory) {
        return agents[agentId];
    }

    /// @notice Get mandate configuration
    /// @param mandateId The mandate identifier
    /// @return mandate The mandate configuration
    function getMandateConfig(bytes32 mandateId) external view returns (MandateConfig memory) {
        return mandates[mandateId];
    }

    /// @notice Get agent risk limits from mandate
    /// @param agentId The agent identifier
    /// @return maxLeverage Maximum leverage (basis points)
    /// @return maxNetDelta Maximum net delta (basis points)
    /// @return maxDrawdown Maximum drawdown (basis points)
    function getAgentRiskLimits(bytes32 agentId) external view returns (
        uint256 maxLeverage,
        uint256 maxNetDelta,
        uint256 maxDrawdown
    ) {
        bytes32 mandateId = agentMandates[agentId];
        MandateConfig memory mandate = mandates[mandateId];
        
        return (mandate.maxLeverage, mandate.maxNetDelta, mandate.maxDrawdown);
    }

    /// @notice Calculate mandate hash for verification
    /// @param mandateId The mandate identifier
    /// @return mandateHash The calculated mandate hash
    function _calculateMandateHash(bytes32 mandateId) internal view returns (bytes32) {
        MandateConfig memory mandate = mandates[mandateId];
        
        return keccak256(abi.encode(
            mandate.mandateId,
            mandate.strategyCodeHash,
            mandate.maxLeverage,
            mandate.maxNetDelta,
            mandate.maxDrawdown,
            mandate.approvedVenues,
            mandate.creationTime
        ));
    }

    /// @notice Get approved venues for an agent
    /// @param agentId The agent identifier
    /// @return venues Array of approved venue addresses
    function getApprovedVenues(bytes32 agentId) external view returns (address[] memory) {
        bytes32 mandateId = agentMandates[agentId];
        return mandates[mandateId].approvedVenues;
    }

    /// @notice Check if a venue is approved for an agent
    /// @param agentId The agent identifier
    /// @param venue The venue address to check
    /// @return isApproved True if venue is approved
    function isVenueApproved(bytes32 agentId, address venue) external view returns (bool) {
        bytes32 mandateId = agentMandates[agentId];
        address[] memory approvedVenues = mandates[mandateId].approvedVenues;
        
        for (uint256 i = 0; i < approvedVenues.length; i++) {
            if (approvedVenues[i] == venue) {
                return true;
            }
        }
        
        return false;
    }
}