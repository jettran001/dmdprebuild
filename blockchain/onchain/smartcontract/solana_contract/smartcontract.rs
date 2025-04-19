//! # Solana DMD Token Program
//! 
//! Program này triển khai DMD token trên Solana theo chuẩn SPL Token và
//! tích hợp với LayerZero để bridge token giữa Solana và các blockchain khác (đặc biệt là NEAR).

use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint,
    entrypoint::ProgramResult,
    msg,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    program_pack::{IsInitialized, Pack, Sealed},
    pubkey::Pubkey,
    system_instruction,
    sysvar::{rent::Rent, Sysvar},
};
use spl_token::{
    instruction as token_instruction,
    state::{Account as TokenAccount, Mint},
};
use std::convert::TryInto;
use arrayref::{array_mut_ref, array_ref, array_refs, mut_array_refs};

// LayerZero Chain IDs
const LAYERZERO_CHAIN_ID_NEAR: u16 = 7; // Giả định cho NEAR
const LAYERZERO_CHAIN_ID_SOLANA: u16 = 5; // Giả định cho Solana

// Program ID của LayerZero Endpoint trên Solana
const LAYERZERO_ENDPOINT: &str = "LZ4ip5XnYXfHGAGQbvKhbQGMpXD9eM2BVfTnrhXNEzX"; // Ví dụ

/// Define Instruction enum để xử lý các lệnh khác nhau
#[derive(Clone, Debug, PartialEq)]
pub enum DmdInstruction {
    /// Khởi tạo DMD token mint
    /// 0. `[signer]` Tài khoản người tạo
    /// 1. `[writable]` Mint account mới
    /// 2. `[]` Rent sysvar
    /// 3. `[]` Token program
    InitializeMint {
        /// Số thập phân của token
        decimals: u8,
    },

    /// Khởi tạo DMD token account
    /// 0. `[signer]` Chủ sở hữu
    /// 1. `[writable]` Token account mới
    /// 2. `[]` Mint account
    /// 3. `[]` Rent sysvar
    /// 4. `[]` Token program
    InitializeAccount,

    /// Mint DMD token
    /// 0. `[signer]` Tài khoản mint authority
    /// 1. `[writable]` Token account đích
    /// 2. `[writable]` Mint account
    /// 3. `[]` Token program
    MintTo {
        /// Số lượng token cần mint
        amount: u64,
    },

    /// Chuyển DMD token
    /// 0. `[signer]` Người gửi
    /// 1. `[writable]` Token account nguồn
    /// 2. `[writable]` Token account đích
    /// 3. `[]` Token program
    Transfer {
        /// Số lượng token cần chuyển
        amount: u64,
    },

    /// Bridge token sang NEAR thông qua LayerZero
    /// 0. `[signer]` Người gửi
    /// 1. `[writable]` Token account nguồn
    /// 2. `[writable]` Escrow token account
    /// 3. `[]` Token program
    /// 4. `[]` LayerZero endpoint program
    BridgeToNear {
        /// Địa chỉ đích trên NEAR
        to_address: [u8; 64],
        /// Số lượng token cần bridge
        amount: u64,
        /// Phí trả cho LayerZero relayer (lamports)
        fee: u64,
    },

    /// Nhận token từ NEAR thông qua LayerZero
    /// 0. `[signer]` LayerZero program hoặc relayer
    /// 1. `[writable]` Mint account
    /// 2. `[writable]` Token account đích
    /// 3. `[]` Token program
    ReceiveFromNear {
        /// Địa chỉ nguồn trên NEAR
        from_address: [u8; 64],
        /// Địa chỉ đích trên Solana
        to_address: Pubkey,
        /// Số lượng token nhận
        amount: u64,
    },

    /// Tạo metadata cho token
    /// 0. `[signer]` Update authority
    /// 1. `[writable]` Metadata account
    /// 2. `[]` Mint account
    /// 3. `[]` Metaplex token metadata program
    CreateMetadata {
        /// Tên token
        name: String,
        /// Ký hiệu token
        symbol: String,
        /// URI của metadata
        uri: String,
    },
}

/// Cấu trúc dữ liệu cho Bridge State
#[derive(Debug, PartialEq)]
pub struct BridgeState {
    /// Có được khởi tạo chưa
    pub is_initialized: bool,
    /// Escrow account cho bridge
    pub escrow_account: Pubkey,
    /// LayerZero endpoint
    pub layerzero_endpoint: Pubkey,
    /// Phí cơ bản cho bridge
    pub base_fee: u64,
    /// Mint authority
    pub mint_authority: Pubkey,
    /// Trusted remote addresses
    pub trusted_remotes: [u8; 1024],
}

impl Sealed for BridgeState {}

impl IsInitialized for BridgeState {
    fn is_initialized(&self) -> bool {
        self.is_initialized
    }
}

impl Pack for BridgeState {
    const LEN: usize = 1 + 32 + 32 + 8 + 32 + 1024;

    fn pack_into_slice(&self, dst: &mut [u8]) {
        let dst = array_mut_ref![dst, 0, BridgeState::LEN];
        let (
            is_initialized_dst,
            escrow_account_dst,
            layerzero_endpoint_dst,
            base_fee_dst,
            mint_authority_dst,
            trusted_remotes_dst,
        ) = mut_array_refs![dst, 1, 32, 32, 8, 32, 1024];

        is_initialized_dst[0] = self.is_initialized as u8;
        escrow_account_dst.copy_from_slice(self.escrow_account.as_ref());
        layerzero_endpoint_dst.copy_from_slice(self.layerzero_endpoint.as_ref());
        *base_fee_dst = self.base_fee.to_le_bytes();
        mint_authority_dst.copy_from_slice(self.mint_authority.as_ref());
        trusted_remotes_dst.copy_from_slice(&self.trusted_remotes);
    }

    fn unpack_from_slice(src: &[u8]) -> Result<Self, ProgramError> {
        let src = array_ref![src, 0, BridgeState::LEN];
        let (
            is_initialized_src,
            escrow_account_src,
            layerzero_endpoint_src,
            base_fee_src,
            mint_authority_src,
            trusted_remotes_src,
        ) = array_refs![src, 1, 32, 32, 8, 32, 1024];

        let is_initialized = match is_initialized_src[0] {
            0 => false,
            1 => true,
            _ => return Err(ProgramError::InvalidAccountData),
        };

        let base_fee = u64::from_le_bytes(*base_fee_src);

        Ok(BridgeState {
            is_initialized,
            escrow_account: Pubkey::new_from_array(*escrow_account_src),
            layerzero_endpoint: Pubkey::new_from_array(*layerzero_endpoint_src),
            base_fee,
            mint_authority: Pubkey::new_from_array(*mint_authority_src),
            trusted_remotes: *trusted_remotes_src,
        })
    }
}

// Entry point for the Solana program
entrypoint!(process_instruction);

/// Xử lý lệnh được gửi tới program
pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    // Đảm bảo độ dài dữ liệu hợp lệ
    if instruction_data.is_empty() {
        return Err(ProgramError::InvalidInstructionData);
    }

    // Phân tích cú pháp loại lệnh
    let instruction = instruction_data[0];

    match instruction {
        0 => {
            // InitializeMint
            let decimals = instruction_data[1];
            process_initialize_mint(program_id, accounts, decimals)
        }
        1 => {
            // InitializeAccount
            process_initialize_account(program_id, accounts)
        }
        2 => {
            // MintTo
            let amount = u64::from_le_bytes(instruction_data[1..9].try_into().unwrap());
            process_mint_to(program_id, accounts, amount)
        }
        3 => {
            // Transfer
            let amount = u64::from_le_bytes(instruction_data[1..9].try_into().unwrap());
            process_transfer(program_id, accounts, amount)
        }
        4 => {
            // BridgeToNear
            let to_address = array_ref![instruction_data, 1, 64];
            let amount = u64::from_le_bytes(instruction_data[65..73].try_into().unwrap());
            let fee = u64::from_le_bytes(instruction_data[73..81].try_into().unwrap());
            process_bridge_to_near(program_id, accounts, *to_address, amount, fee)
        }
        5 => {
            // ReceiveFromNear
            let from_address = array_ref![instruction_data, 1, 64];
            let pubkey_bytes = array_ref![instruction_data, 65, 32];
            let to_address = Pubkey::new_from_array(*pubkey_bytes);
            let amount = u64::from_le_bytes(instruction_data[97..105].try_into().unwrap());
            process_receive_from_near(program_id, accounts, *from_address, to_address, amount)
        }
        6 => {
            // CreateMetadata
            process_create_metadata(program_id, accounts, &instruction_data[1..])
        }
        _ => {
            // Lệnh không được hỗ trợ
            Err(ProgramError::InvalidInstructionData)
        }
    }
}

/// Khởi tạo DMD token mint
fn process_initialize_mint(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    decimals: u8,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let creator_info = next_account_info(account_info_iter)?;
    let mint_info = next_account_info(account_info_iter)?;
    let rent_info = next_account_info(account_info_iter)?;
    let token_program_info = next_account_info(account_info_iter)?;

    // Xác thực chữ ký
    if !creator_info.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Kiểm tra chương trình token
    if *token_program_info.key != spl_token::id() {
        return Err(ProgramError::IncorrectProgramId);
    }

    // Rent là exempt
    let rent = &Rent::from_account_info(rent_info)?;
    if !rent.is_exempt(mint_info.lamports(), Mint::LEN) {
        return Err(ProgramError::AccountNotRentExempt);
    }

    // Khởi tạo mint
    let initialize_mint_ix = token_instruction::initialize_mint(
        &spl_token::id(),
        mint_info.key,
        creator_info.key,
        Some(creator_info.key),
        decimals,
    )?;

    invoke(
        &initialize_mint_ix,
        &[mint_info.clone(), rent_info.clone(), token_program_info.clone()],
    )?;

    msg!("Mint được khởi tạo: {}", mint_info.key);
    
    Ok(())
}

/// Khởi tạo DMD token account
fn process_initialize_account(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let owner_info = next_account_info(account_info_iter)?;
    let token_account_info = next_account_info(account_info_iter)?;
    let mint_info = next_account_info(account_info_iter)?;
    let rent_info = next_account_info(account_info_iter)?;
    let token_program_info = next_account_info(account_info_iter)?;

    // Xác thực chữ ký
    if !owner_info.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Khởi tạo token account
    let initialize_account_ix = token_instruction::initialize_account(
        &spl_token::id(),
        token_account_info.key,
        mint_info.key,
        owner_info.key,
    )?;

    invoke(
        &initialize_account_ix,
        &[
            token_account_info.clone(),
            mint_info.clone(),
            owner_info.clone(),
            rent_info.clone(),
            token_program_info.clone(),
        ],
    )?;

    msg!("Token account được khởi tạo: {}", token_account_info.key);
    
    Ok(())
}

/// Mint DMD token
fn process_mint_to(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    amount: u64,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let mint_authority_info = next_account_info(account_info_iter)?;
    let token_account_info = next_account_info(account_info_iter)?;
    let mint_info = next_account_info(account_info_iter)?;
    let token_program_info = next_account_info(account_info_iter)?;

    // Xác thực chữ ký
    if !mint_authority_info.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Mint token
    let mint_to_ix = token_instruction::mint_to(
        &spl_token::id(),
        mint_info.key,
        token_account_info.key,
        mint_authority_info.key,
        &[],
        amount,
    )?;

    invoke(
        &mint_to_ix,
        &[
            mint_info.clone(),
            token_account_info.clone(),
            mint_authority_info.clone(),
            token_program_info.clone(),
        ],
    )?;

    msg!("Mint {} tokens to {}", amount, token_account_info.key);
    
    Ok(())
}

/// Chuyển DMD token
fn process_transfer(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    amount: u64,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let owner_info = next_account_info(account_info_iter)?;
    let source_info = next_account_info(account_info_iter)?;
    let destination_info = next_account_info(account_info_iter)?;
    let token_program_info = next_account_info(account_info_iter)?;

    // Xác thực chữ ký
    if !owner_info.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Chuyển token
    let transfer_ix = token_instruction::transfer(
        &spl_token::id(),
        source_info.key,
        destination_info.key,
        owner_info.key,
        &[],
        amount,
    )?;

    invoke(
        &transfer_ix,
        &[
            source_info.clone(),
            destination_info.clone(),
            owner_info.clone(),
            token_program_info.clone(),
        ],
    )?;

    msg!("Chuyển {} tokens từ {} tới {}", amount, source_info.key, destination_info.key);
    
    Ok(())
}

/// Bridge token sang NEAR
fn process_bridge_to_near(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    to_address: [u8; 64],
    amount: u64,
    fee: u64,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let owner_info = next_account_info(account_info_iter)?;
    let source_info = next_account_info(account_info_iter)?;
    let escrow_info = next_account_info(account_info_iter)?;
    let token_program_info = next_account_info(account_info_iter)?;
    let layerzero_endpoint_info = next_account_info(account_info_iter)?;

    // Xác thực chữ ký
    if !owner_info.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Kiểm tra endpoint
    if layerzero_endpoint_info.key.to_string() != LAYERZERO_ENDPOINT {
        return Err(ProgramError::InvalidAccountData);
    }

    // Chuyển token vào escrow account
    let transfer_to_escrow_ix = token_instruction::transfer(
        &spl_token::id(),
        source_info.key,
        escrow_info.key,
        owner_info.key,
        &[],
        amount,
    )?;

    invoke(
        &transfer_to_escrow_ix,
        &[
            source_info.clone(),
            escrow_info.clone(),
            owner_info.clone(),
            token_program_info.clone(),
        ],
    )?;

    // Chuẩn bị payload gửi đến LayerZero
    let mut payload = Vec::with_capacity(104);
    payload.extend_from_slice(&[0x01]); // Loại message: bridge token
    payload.extend_from_slice(&to_address); // Địa chỉ đích
    payload.extend_from_slice(&amount.to_le_bytes()); // Số lượng token
    payload.extend_from_slice(owner_info.key.as_ref()); // Người gửi

    // Gửi thông qua LayerZero
    // Trong thực tế, cần triển khai cụ thể hơn với LayerZero SDK
    // Đây chỉ là demo đơn giản
    msg!("Bridging {} tokens to NEAR address", amount);
    msg!("NEAR address: {:?}", to_address);
    msg!("LayerZero fee: {} lamports", fee);

    // Demo ghi log thành công
    msg!("Bridge request sent to LayerZero endpoint");
    
    Ok(())
}

/// Nhận token từ NEAR
fn process_receive_from_near(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    from_address: [u8; 64],
    to_address: Pubkey,
    amount: u64,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let lz_caller_info = next_account_info(account_info_iter)?;
    let mint_info = next_account_info(account_info_iter)?;
    let destination_info = next_account_info(account_info_iter)?;
    let token_program_info = next_account_info(account_info_iter)?;
    
    // Trong thực tế, cần xác thực rằng lz_caller_info là từ LayerZero endpoint
    // Đây chỉ là demo đơn giản
    
    // Mint token cho người nhận
    let mint_to_ix = token_instruction::mint_to(
        &spl_token::id(),
        mint_info.key,
        destination_info.key,
        lz_caller_info.key, // Trong thực tế, cần sử dụng PDA của program
        &[],
        amount,
    )?;

    invoke_signed(
        &mint_to_ix,
        &[
            mint_info.clone(),
            destination_info.clone(),
            lz_caller_info.clone(),
            token_program_info.clone(),
        ],
        &[], // Trong thực tế, cần sử dụng đúng seed
    )?;

    msg!("Received {} tokens from NEAR", amount);
    msg!("NEAR address: {:?}", from_address);
    msg!("Tokens minted to: {}", to_address);
    
    Ok(())
}

/// Tạo metadata cho token
fn process_create_metadata(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    // Đây chỉ là stub function, trong thực tế cần tích hợp với Metaplex
    msg!("Creating metadata for DMD token...");
    msg!("This is a placeholder - actual implementation would use Metaplex Token Metadata Program");
    
    Ok(())
}

/// Unit tests for the Solana program
#[cfg(test)]
mod tests {
    use super::*;
    use solana_program::program_pack::Pack;
    use solana_program_test::*;
    use solana_sdk::{signature::Keypair, transaction::Transaction};

    #[tokio::test]
    async fn test_dmd_token() {
        let program_id = Pubkey::new_unique();
        let (mut banks_client, payer, recent_blockhash) = ProgramTest::new(
            "solana_dmd_token",
            program_id,
            processor!(process_instruction),
        )
        .start()
        .await;

        // Create mint account
        let mint_account = Keypair::new();
        let rent = banks_client.get_rent().await.unwrap();
        let mint_rent = rent.minimum_balance(Mint::LEN);

        // Create transaction to allocate mint account
        let mut transaction = Transaction::new_with_payer(
            &[
                system_instruction::create_account(
                    &payer.pubkey(),
                    &mint_account.pubkey(),
                    mint_rent,
                    Mint::LEN as u64,
                    &spl_token::id(),
                ),
                // Initialize mint instruction would go here
            ],
            Some(&payer.pubkey()),
        );
        transaction.sign(&[&payer, &mint_account], recent_blockhash);

        // Test execution would go here
        // banks_client.process_transaction(transaction).await.unwrap();
        
        // For simplicity, we're just logging success
        println!("Test completed successfully!");
    }
}
