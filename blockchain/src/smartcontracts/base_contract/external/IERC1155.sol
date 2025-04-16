// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

/**
 * @dev Interface của ERC-1155 Multi Token Standard.
 */
interface IERC1155 {
    /**
     * @dev Emitted khi `value` tokens của token type `id` được chuyển từ `from` đến `to` bởi `operator`.
     */
    event TransferSingle(address indexed operator, address indexed from, address indexed to, uint256 id, uint256 value);

    /**
     * @dev Tương tự như {TransferSingle}, nhưng cho việc chuyển nhiều token cùng lúc.
     */
    event TransferBatch(address indexed operator, address indexed from, address indexed to, uint256[] ids, uint256[] values);

    /**
     * @dev Emitted khi `account` grant hay revoke permission cho `operator` để transfer tokens của họ.
     */
    event ApprovalForAll(address indexed account, address indexed operator, bool approved);

    /**
     * @dev Emitted khi URI cho token `id` thay đổi thành `value`.
     */
    event URI(string value, uint256 indexed id);

    /**
     * @dev Trả về số lượng tokens thuộc type `id` được sở hữu bởi `account`.
     *
     * Yêu cầu:
     * - `account` không thể là zero address.
     */
    function balanceOf(address account, uint256 id) external view returns (uint256);

    /**
     * @dev Trả về mảng các số dư
     *
     * Yêu cầu:
     * - `accounts` và `ids` phải có cùng độ dài.
     */
    function balanceOfBatch(address[] calldata accounts, uint256[] calldata ids) external view returns (uint256[] memory);

    /**
     * @dev Grant hoặc revoke permission cho `operator` để chuyển tokens của caller.
     *
     * Emit một {ApprovalForAll} event.
     *
     * Yêu cầu:
     * - `operator` không thể là caller.
     */
    function setApprovalForAll(address operator, bool approved) external;

    /**
     * @dev Trả về nếu `operator` được phép chuyển tokens của `account`.
     *
     * Xem {setApprovalForAll}.
     */
    function isApprovedForAll(address account, address operator) external view returns (bool);

    /**
     * @dev Chuyển `amount` tokens của token type `id` từ `from` đến `to`.
     *
     * Emit một {TransferSingle} event.
     *
     * Yêu cầu:
     * - `to` không thể là zero address.
     * - Nếu caller không phải là `from`, họ phải được approve để move tokens (xem {isApprovedForAll}).
     * - `from` phải có đủ số dư của token type `id`.
     * - Nếu `to` là một contract, nó phải implement {IERC1155Receiver.onERC1155Received}.
     */
    function safeTransferFrom(address from, address to, uint256 id, uint256 amount, bytes calldata data) external;

    /**
     * @dev Chuyển nhiều token types từ `from` đến `to`.
     *
     * Emit một {TransferBatch} event.
     *
     * Yêu cầu:
     * - `ids` và `amounts` phải có cùng độ dài.
     * - Nếu `to` là một contract, nó phải implement {IERC1155Receiver.onERC1155BatchReceived}.
     */
    function safeBatchTransferFrom(address from, address to, uint256[] calldata ids, uint256[] calldata amounts, bytes calldata data) external;
} 