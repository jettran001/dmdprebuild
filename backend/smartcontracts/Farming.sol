// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import "@openzeppelin/contracts/access/Ownable.sol";
import "@openzeppelin/contracts/token/ERC20/IERC20.sol";

contract Farming is Ownable {
    IERC20 public stakingToken; // Token dùng để stake (DMD)
    uint256 public totalStaked;
    uint256 public rewardRate = 1 * 10**18; // 1 DMD mỗi giây cho mỗi 100 DMD staked
    uint256 public constant LOCK_PERIOD = 30 days;

    struct Stake {
        uint256 amount;
        uint256 startTime;
        uint256 lastClaim;
    }

    mapping(address => Stake) public stakes;

    event Staked(address indexed user, uint256 amount);
    event Unstaked(address indexed user, uint256 amount);
    event RewardClaimed(address indexed user, uint256 amount);

    constructor(address _stakingToken) {
        stakingToken = IERC20(_stakingToken);
    }

    function stake(uint256 amount) external {
        require(amount > 0, "Amount must be greater than 0");
        stakingToken.transferFrom(msg.sender, address(this), amount);

        Stake storage userStake = stakes[msg.sender];
        if (userStake.amount > 0) {
            claimRewards();
        }

        userStake.amount += amount;
        userStake.startTime = block.timestamp;
        userStake.lastClaim = block.timestamp;
        totalStaked += amount;

        emit Staked(msg.sender, amount);
    }

    function unstake(uint256 amount) external {
        Stake storage userStake = stakes[msg.sender];
        require(userStake.amount >= amount, "Insufficient staked amount");
        require(block.timestamp >= userStake.startTime + LOCK_PERIOD, "Stake is locked");

        claimRewards();
        userStake.amount -= amount;
        totalStaked -= amount;
        stakingToken.transfer(msg.sender, amount);

        emit Unstaked(msg.sender, amount);
    }

    function claimRewards() public {
        Stake storage userStake = stakes[msg.sender];
        uint256 timeElapsed = block.timestamp - userStake.lastClaim;
        uint256 reward = (userStake.amount * rewardRate * timeElapsed) / (100 * 10**18);

        if (reward > 0) {
            userStake.lastClaim = block.timestamp;
            stakingToken.transfer(msg.sender, reward);
            emit RewardClaimed(msg.sender, reward);
        }
    }

    function setRewardRate(uint256 _rewardRate) external onlyOwner {
        rewardRate = _rewardRate;
    }
}