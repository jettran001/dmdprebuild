// SPDX-License-Identifier: MIT
pragma solidity 0.8.20;

/**
 * @title ChainRegistry
 * @dev Library for standardized chainId <-> chainName <-> remoteAddress mapping (standardized by .solguard)
 */
library ChainRegistry {
    struct ChainInfo {
        string name;
        bytes remoteAddress;
        bool supported;
        address wrappedToken;
        address unwrapper;
    }

    struct Registry {
        mapping(uint16 => ChainInfo) chains;
        mapping(string => uint16) nameToId;
        uint16[] supportedChainIds;
    }

    uint16 public constant MAX_CHAINS = 32;
    event ChainRegistryUpdated(uint16 indexed chainId, string name, bool supported, address indexed updater, uint256 timestamp);

    /**
     * @notice Register a new chain or update info (onlyOwner should be enforced in contract)
     */
    function setChain(
        Registry storage self,
        uint16 chainId,
        string memory name,
        bytes memory remoteAddress,
        bool supported,
        address owner,
        address caller
    ) internal {
        require(bytes(name).length > 0, "ChainRegistry: name cannot be empty");
        require(owner == caller, "ChainRegistry: only owner can update");
        // Check for duplicate name
        if (self.nameToId[name] != 0 && self.nameToId[name] != chainId) {
            revert("ChainRegistry: duplicate chain name");
        }
        // Limit number of chains
        if (supported) {
            bool found = false;
            for (uint i = 0; i < self.supportedChainIds.length; i++) {
                if (self.supportedChainIds[i] == chainId) {
                    found = true;
                    break;
                }
            }
            if (!found) {
                require(self.supportedChainIds.length < MAX_CHAINS, "ChainRegistry: too many chains");
                self.supportedChainIds.push(chainId);
            }
        }
        if (!supported) {
            // Safe swap-and-pop
            for (uint i = 0; i < self.supportedChainIds.length; i++) {
                if (self.supportedChainIds[i] == chainId) {
                    uint last = self.supportedChainIds.length - 1;
                    if (i != last) {
                        self.supportedChainIds[i] = self.supportedChainIds[last];
                    }
                    self.supportedChainIds.pop();
                    break;
                }
            }
        }
        self.chains[chainId] = ChainInfo({name: name, remoteAddress: remoteAddress, supported: supported, wrappedToken: address(0), unwrapper: address(0)});
        self.nameToId[name] = chainId;
        emit ChainRegistryUpdated(chainId, name, supported, caller, block.timestamp);
    }

    /**
     * @notice Get chain name by chainId
     */
    function getChainName(Registry storage self, uint16 chainId) internal view returns (string memory) {
        return self.chains[chainId].name;
    }

    /**
     * @notice Get chainId by name
     */
    function getChainId(Registry storage self, string memory name) internal view returns (uint16) {
        return self.nameToId[name];
    }

    /**
     * @notice Get remote address by chainId
     */
    function getRemoteAddress(Registry storage self, uint16 chainId) internal view returns (bytes memory) {
        return self.chains[chainId].remoteAddress;
    }

    /**
     * @notice Check if chain is supported
     */
    function isChainSupported(Registry storage self, uint16 chainId) internal view returns (bool) {
        return self.chains[chainId].supported;
    }

    /**
     * @notice Get all supported chainIds
     */
    function getSupportedChainIds(Registry storage self) internal view returns (uint16[] memory) {
        return self.supportedChainIds;
    }

    /**
     * @notice Set wrappedToken for a chain
     */
    function setWrappedToken(Registry storage self, uint16 chainId, address token) internal {
        self.chains[chainId].wrappedToken = token;
    }

    /**
     * @notice Get wrappedToken for a chain
     */
    function getWrappedToken(Registry storage self, uint16 chainId) internal view returns (address) {
        return self.chains[chainId].wrappedToken;
    }

    /**
     * @notice Set unwrapper for a chain
     */
    function setUnwrapper(Registry storage self, uint16 chainId, address unwrapper) internal {
        self.chains[chainId].unwrapper = unwrapper;
    }

    /**
     * @notice Get unwrapper for a chain
     */
    function getUnwrapper(Registry storage self, uint16 chainId) internal view returns (address) {
        return self.chains[chainId].unwrapper;
    }
} 