// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

/**
 * @dev Interface của chuẩn ERC165 - Standard Interface Detection.
 *
 * Cho phép smart contracts tuyên bố hỗ trợ interface để cho phép các contract khác kiểm tra.
 * Xem thêm: https://eips.ethereum.org/EIPS/eip-165
 */
interface IERC165 {
    /**
     * @dev Trả về `true` nếu contract này implements interface được chỉ định `interfaceId`.
     * Chi tiết xem tại: https://eips.ethereum.org/EIPS/eip-165#how-interfaces-are-identified
     */
    function supportsInterface(bytes4 interfaceId) external view returns (bool);
} 