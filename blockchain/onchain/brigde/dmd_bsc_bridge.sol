// SPDX-License-Identifier: MIT
pragma solidity ^0.8.17;

import "@openzeppelin/contracts/access/Ownable.sol";
import "@openzeppelin/contracts/security/Pausable.sol";
import "@openzeppelin/contracts/security/ReentrancyGuard.sol";
import "@layerzerolabs/solidity-examples/contracts/lzApp/NonblockingLzApp.sol";

// Interface cho DMD Token
interface IDiamondToken {
    function bridgeMint(address to, uint256 amount) external;
    function balanceOf(address account, uint256 id) external view returns (uint256);
    function burn(address account, uint256 id, uint256 value) external;
    function safeTransferFrom(address from, address to, uint256 id, uint256 amount, bytes calldata data) external;
}

/**
 * @title DmdBscBridge
 * @dev Bridge contract for DMD token on BSC, connecting with NEAR (main chain)
 * @notice BSC là sub chain, NEAR là main chain chứa tổng cung thực
 */
contract DmdBscBridge is Ownable, Pausable, ReentrancyGuard, NonblockingLzApp {
    // DMD Token contract
    IDiamondToken public dmdToken;
    
    // Token ID for DMD fungible token
    uint256 public constant DMD_TOKEN_ID = 0;
    
    // LayerZero chain IDs
    uint16 public constant NEAR_CHAIN_ID = 115; // Assuming NEAR chain ID on LayerZero
    uint16 public constant SOLANA_CHAIN_ID = 168; // Assuming Solana chain ID on LayerZero
    
    // Bridge fee percentages (in basis points, 1% = 100)
    uint16 public bridgeFeePercent = 10; // 0.1% default
    
    // Remote addresses
    mapping(uint16 => bytes) public trustedRemotes;
    
    // Supported chains for bridging
    mapping(uint16 => bool) public supportedChains;
    
    // Events
    event BridgeToNear(address indexed from, bytes nearAddress, uint256 amount, uint256 fee);
    event BridgeToSolana(address indexed from, bytes solanaAddress, uint256 amount, uint256 fee);
    event TokensReceived(uint16 indexed fromChainId, bytes fromAddress, address toAddress, uint256 amount);
    event TrustedRemoteUpdated(uint16 indexed chainId, bytes remoteAddress);
    event SupportedChainUpdated(uint16 indexed chainId, bool supported);
    event BridgeFeeUpdated(uint16 newFeePercent);
    
    /**
     * @dev Initialize the bridge contract
     * @param _dmdToken Address of DMD token contract
     * @param _lzEndpoint Address of LayerZero endpoint
     */
    constructor(address _dmdToken, address _lzEndpoint) NonblockingLzApp(_lzEndpoint) {
        require(_dmdToken != address(0), "DMD token address cannot be zero");
        dmdToken = IDiamondToken(_dmdToken);
        
        // Set NEAR as a supported chain by default
        supportedChains[NEAR_CHAIN_ID] = true;
    }
    
    /**
     * @dev Bridge DMD tokens to NEAR
     * @param nearAddress Destination address on NEAR (encoded as bytes)
     * @param amount Amount of DMD to bridge
     */
    function bridgeToNear(bytes calldata nearAddress, uint256 amount) external payable whenNotPaused nonReentrant {
        require(supportedChains[NEAR_CHAIN_ID], "NEAR bridging not supported");
        require(amount > 0, "Amount must be greater than 0");
        
        // Check if user has enough balance
        require(dmdToken.balanceOf(msg.sender, DMD_TOKEN_ID) >= amount, "Insufficient DMD balance");
        
        // Calculate fee
        uint256 fee = (amount * bridgeFeePercent) / 10000; // Convert basis points to percentage
        uint256 amountAfterFee = amount - fee;
        
        // Burn tokens (tokens are sent back to NEAR)
        dmdToken.burn(msg.sender, DMD_TOKEN_ID, amount);
        
        // Encode payload
        bytes memory payload = abi.encode(msg.sender, nearAddress, amountAfterFee);
        
        // Estimate LayerZero fee
        (uint256 lzFee, ) = _estimateFees(NEAR_CHAIN_ID, payload);
        require(msg.value >= lzFee, "Insufficient ETH for LayerZero fee");
        
        // Send message to NEAR via LayerZero
        _lzSend(NEAR_CHAIN_ID, payload, payable(msg.sender), address(0), bytes(""), msg.value);
        
        emit BridgeToNear(msg.sender, nearAddress, amountAfterFee, fee);
    }
    
    /**
     * @dev Bridge DMD tokens to Solana
     * @param solanaAddress Destination address on Solana (encoded as bytes)
     * @param amount Amount of DMD to bridge
     */
    function bridgeToSolana(bytes calldata solanaAddress, uint256 amount) external payable whenNotPaused nonReentrant {
        require(supportedChains[SOLANA_CHAIN_ID], "Solana bridging not supported");
        require(amount > 0, "Amount must be greater than 0");
        
        // Check if user has enough balance
        require(dmdToken.balanceOf(msg.sender, DMD_TOKEN_ID) >= amount, "Insufficient DMD balance");
        
        // Calculate fee
        uint256 fee = (amount * bridgeFeePercent) / 10000;
        uint256 amountAfterFee = amount - fee;
        
        // Burn tokens (sent from NEAR to Solana directly)
        dmdToken.burn(msg.sender, DMD_TOKEN_ID, amount);
        
        // Encode payload
        bytes memory payload = abi.encode(msg.sender, solanaAddress, amountAfterFee);
        
        // Estimate LayerZero fee
        (uint256 lzFee, ) = _estimateFees(NEAR_CHAIN_ID, payload);
        require(msg.value >= lzFee, "Insufficient ETH for LayerZero fee");
        
        // Send relay message to NEAR (which will bridge to Solana)
        _lzSend(NEAR_CHAIN_ID, payload, payable(msg.sender), address(0), bytes(""), msg.value);
        
        emit BridgeToSolana(msg.sender, solanaAddress, amountAfterFee, fee);
    }
    
    /**
     * @dev Handle message from LayerZero (implements NonblockingLzApp)
     * @param _srcChainId Source chain ID
     * @param _srcAddress Source address
     * @param _nonce Message nonce
     * @param _payload Message payload
     */
    function _nonblockingLzReceive(
        uint16 _srcChainId,
        bytes memory _srcAddress,
        uint64 _nonce,
        bytes memory _payload
    ) internal override {
        require(supportedChains[_srcChainId], "Chain not supported");
        
        // Verify trusted remote
        require(_srcAddress.length > 0 && trustedRemotes[_srcChainId].length > 0, "Invalid source");
        require(keccak256(_srcAddress) == keccak256(trustedRemotes[_srcChainId]), "Source not trusted");
        
        // Decode payload
        (address toAddress, uint256 amount) = abi.decode(_payload, (address, uint256));
        
        // Mint tokens to recipient
        dmdToken.bridgeMint(toAddress, amount);
        
        emit TokensReceived(_srcChainId, _srcAddress, toAddress, amount);
    }
    
    /**
     * @dev Estimate gas fee for bridging
     * @param _dstChainId Destination chain ID
     * @param _payload Message payload
     * @return nativeFee Native fee for gas
     * @return zroFee ZRO token fee
     */
    function _estimateFees(uint16 _dstChainId, bytes memory _payload) internal view returns (uint256 nativeFee, uint256 zroFee) {
        return _lzEndpoint.estimateFees(_dstChainId, address(this), _payload, false, bytes(""));
    }
    
    /**
     * @dev Estimate gas fee for bridging (public version)
     * @param _dstChainId Destination chain ID
     * @param _toAddress Destination address
     * @param _amount Amount to bridge
     * @return Total native fee
     */
    function estimateBridgeFee(uint16 _dstChainId, address _toAddress, uint256 _amount) public view returns (uint256) {
        bytes memory payload = abi.encode(_toAddress, _amount);
        (uint256 nativeFee, ) = _estimateFees(_dstChainId, payload);
        return nativeFee;
    }
    
    /**
     * @dev Set trusted remote for a chain
     * @param _chainId Chain ID
     * @param _remoteAddress Remote address (encoded as bytes)
     */
    function setTrustedRemote(uint16 _chainId, bytes calldata _remoteAddress) external onlyOwner {
        trustedRemotes[_chainId] = _remoteAddress;
        emit TrustedRemoteUpdated(_chainId, _remoteAddress);
    }
    
    /**
     * @dev Set supported chain status
     * @param _chainId Chain ID
     * @param _supported Whether the chain is supported
     */
    function setSupportedChain(uint16 _chainId, bool _supported) external onlyOwner {
        supportedChains[_chainId] = _supported;
        emit SupportedChainUpdated(_chainId, _supported);
    }
    
    /**
     * @dev Set bridge fee percentage
     * @param _feePercent New fee percentage (in basis points)
     */
    function setBridgeFee(uint16 _feePercent) external onlyOwner {
        require(_feePercent <= 1000, "Fee cannot exceed 10%");
        bridgeFeePercent = _feePercent;
        emit BridgeFeeUpdated(_feePercent);
    }
    
    /**
     * @dev Pause bridging
     */
    function pause() external onlyOwner {
        _pause();
    }
    
    /**
     * @dev Unpause bridging
     */
    function unpause() external onlyOwner {
        _unpause();
    }
    
    /**
     * @dev Withdraw collected fees
     * @param _token Token address (use zero address for ETH)
     * @param _amount Amount to withdraw
     */
    function withdrawFees(address _token, uint256 _amount) external onlyOwner {
        if (_token == address(0)) {
            (bool success, ) = msg.sender.call{value: _amount}("");
            require(success, "ETH transfer failed");
        } else {
            // For future token fee handling
        }
    }
    
    /**
     * @dev Receive function to accept ETH
     */
    receive() external payable {}
} 