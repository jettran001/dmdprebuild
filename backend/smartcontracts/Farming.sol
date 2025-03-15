// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import "@openzeppelin/contracts/token/ERC721/ERC721.sol";

contract Farming is ERC721 {
    IERC20 public dmdToken;
    address public reserveWallet;
    uint256 public nftCounter;

    struct NFT {
        string uri;
        uint256 price;
        bool forSale;
    }

    mapping(uint256 => NFT) public nfts;
    mapping(address => uint256) public stakes;

    constructor(address _dmdToken, address _reserveWallet) ERC721("DiamondNFT", "DNFT") {
        dmdToken = IERC20(_dmdToken);
        reserveWallet = _reserveWallet;
    }

    function stake(uint256 amount) public {
        require(dmdToken.transferFrom(msg.sender, address(this), amount), "Transfer failed");
        stakes[msg.sender] += amount;
    }

    function withdraw(uint256 amount) public {
        require(stakes[msg.sender] >= amount, "Insufficient stake");
        stakes[msg.sender] -= amount;
        require(dmdToken.transfer(msg.sender, amount), "Transfer failed");
    }

    function mintNFT(string memory uri, uint256 price) public returns (uint256) {
        nftCounter++;
        _mint(msg.sender, nftCounter);
        nfts[nftCounter] = NFT(uri, price, true);
        return nftCounter;
    }

    function buyNFT(uint256 nftId) public {
        NFT memory nft = nfts[nftId];
        require(nft.forSale, "Not for sale");
        require(dmdToken.transferFrom(msg.sender, reserveWallet, nft.price), "Transfer failed");
        _transfer(ownerOf(nftId), msg.sender, nftId);
        nfts[nftId].forSale = false;
    }

    function getAllNFTs() public view returns (NFT[] memory) {
        NFT[] memory allNFTs = new NFT[](nftCounter);
        for (uint256 i = 1; i <= nftCounter; i++) {
            allNFTs[i - 1] = nfts[i];
        }
        return allNFTs;
    }

    function getPrice() public pure returns (uint256) {
        return 100; // Giả lập
    }
}