// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import "@openzeppelin/contracts/access/Ownable.sol";
import "./DiamondToken.sol";

contract DiamondMining is Ownable {
    DiamondToken public diamondToken;
    uint256 public rewardRate = 10 * 10**18; // 10 DMD mỗi block
    uint256 public lastRewardBlock;
    mapping(address => uint256) public rewards;

    event RewardClaimed(address indexed user, uint256 amount);

    constructor(address _diamondToken) {
        diamondToken = DiamondToken(_diamondToken);
        lastRewardBlock = block.number;
    }

    function updateRewards() public {
        uint256 blocksPassed = block.number - lastRewardBlock;
        if (blocksPassed > 0) {
            uint256 totalReward = blocksPassed * rewardRate;
            rewards[msg.sender] += totalReward;
            lastRewardBlock = block.number;
        }
    }

    function claimRewards() external {
        updateRewards();
        uint256 reward = rewards[msg.sender];
        require(reward > 0, "No rewards to claim");

        rewards[msg.sender] = 0;
        diamondToken.mint(msg.sender, reward);
        emit RewardClaimed(msg.sender, reward);
    }

    function setRewardRate(uint256 _rewardRate) external onlyOwner {
        rewardRate = _rewardRate;
    }
}