// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import "./IERC165.sol";

/**
 * @dev Interface sử dụng cho bất kỳ contract nào muốn hỗ trợ safeTransfers
 * từ ERC1155 asset contracts.
 */
interface IERC1155Receiver is IERC165 {
    /**
     * @dev Handles the receipt of a single ERC1155 token type.
     *
     * @param operator Địa chỉ gọi hàm safeTransferFrom
     * @param from Địa chỉ token được chuyển từ đó
     * @param id ID của token được chuyển
     * @param value Số lượng token được chuyển
     * @param data Data bổ sung, không có định dạng cụ thể
     *
     * @return Bytes4 `bytes4(keccak256("onERC1155Received(address,address,uint256,uint256,bytes)"))`
     */
    function onERC1155Received(
        address operator,
        address from,
        uint256 id,
        uint256 value,
        bytes calldata data
    ) external returns (bytes4);

    /**
     * @dev Handles the receipt of a multiple ERC1155 token types.
     *
     * @param operator Địa chỉ gọi hàm safeBatchTransferFrom
     * @param from Địa chỉ token được chuyển từ đó
     * @param ids Mảng của token IDs
     * @param values Mảng số lượng token cho mỗi ID
     * @param data Data bổ sung, không có định dạng cụ thể
     *
     * @return Bytes4 `bytes4(keccak256("onERC1155BatchReceived(address,address,uint256[],uint256[],bytes)"))`
     */
    function onERC1155BatchReceived(
        address operator,
        address from,
        uint256[] calldata ids,
        uint256[] calldata values,
        bytes calldata data
    ) external returns (bytes4);
} 