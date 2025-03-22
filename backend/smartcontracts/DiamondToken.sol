// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import "@openzeppelin/contracts/access/Ownable.sol";

contract DiamondToken is ERC20, Ownable {
    uint256 public constant MAX_SUPPLY = 100_000_000 * 10**18; // 100M tokens
    uint256 public buyTax = 2; // 2% tax khi mua
    uint256 public sellTax = 2; // 2% tax khi bán
    address public taxReceiver;

    constructor() ERC20("Diamond Token", "DMD") {
        taxReceiver = msg.sender;
        _mint(msg.sender, 10_000_000 * 10**18); // Mint ban đầu 10M tokens
    }

    function transfer(address to, uint256 amount) public override returns (bool) {
        uint256 taxAmount = 0;
        if (to != taxReceiver && msg.sender != owner()) { // Không tính tax cho owner hoặc taxReceiver
            taxAmount = (amount * sellTax) / 100;
            _transfer(msg.sender, taxReceiver, taxAmount);
        }
        _transfer(msg.sender, to, amount - taxAmount);
        return true;
    }

    function setTax(uint256 _buyTax, uint256 _sellTax) external onlyOwner {
        require(_buyTax <= 10 && _sellTax <= 10, "Tax cannot exceed 10%");
        buyTax = _buyTax;
        sellTax = _sellTax;
    }

    function setTaxReceiver(address _taxReceiver) external onlyOwner {
        require(_taxReceiver != address(0), "Invalid address");
        taxReceiver = _taxReceiver;
    }

    function mint(address to, uint256 amount) external onlyOwner {
        require(totalSupply() + amount <= MAX_SUPPLY, "Exceeds max supply");
        _mint(to, amount);
    }

    function burn(uint256 amount) external {
        _burn(msg.sender, amount);
    }
}