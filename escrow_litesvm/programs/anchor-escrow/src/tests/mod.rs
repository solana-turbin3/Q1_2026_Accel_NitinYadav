#[cfg(test)]
mod tests {

    use {
        anchor_lang::{
            prelude::{msg, Clock},
            solana_program::program_pack::Pack,
            AccountDeserialize, InstructionData, ToAccountMetas,
        },
        anchor_spl::{
            associated_token::{self, spl_associated_token_account},
            token::spl_token,
        },
        litesvm::LiteSVM,
        litesvm_token::{
            spl_token::ID as TOKEN_PROGRAM_ID, CreateAssociatedTokenAccount, CreateMint, MintTo,
        },
        solana_account::Account,
        solana_address::Address,
        solana_instruction::Instruction,
        solana_keypair::Keypair,
        solana_message::Message,
        solana_native_token::LAMPORTS_PER_SOL,
        solana_pubkey::Pubkey,
        solana_rpc_client::rpc_client::RpcClient,
        solana_sdk_ids::system_program::ID as SYSTEM_PROGRAM_ID,
        solana_signer::Signer,
        solana_transaction::Transaction,
        std::{path::PathBuf, str::FromStr},
    };

    static PROGRAM_ID: Pubkey = crate::ID;

    /// Setup function to initialize LiteSVM, load program, create mints, and fund maker's ATA
    /// Returns: (LiteSVM instance, payer keypair, mint_a, mint_b, maker_ata_a)
    fn setup() -> (LiteSVM, Keypair, Pubkey, Pubkey, Pubkey) {
        // Initialize LiteSVM and payer
        let mut program = LiteSVM::new();
        let payer = Keypair::new();

        // Airdrop some SOL to the payer keypair
        program
            .airdrop(&payer.pubkey(), 100 * LAMPORTS_PER_SOL)
            .expect("Failed to airdrop SOL to payer");

        // Load program SO file
        let so_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/deploy/anchor_escrow.so");

        let program_data = std::fs::read(so_path).expect("Failed to read program SO file");

        program.add_program(PROGRAM_ID, &program_data);

        // Example on how to Load an account from devnet
        // LiteSVM does not have access to real Solana network data since it does not have network access,
        // so we use an RPC client to fetch account data from devnet
        let rpc_client = RpcClient::new("https://api.devnet.solana.com");
        let account_address =
            Address::from_str("DRYvf71cbF2s5wgaJQvAGkghMkRcp5arvsK2w97vXhi2").unwrap();
        let fetched_account = rpc_client
            .get_account(&account_address)
            .expect("Failed to fetch account from devnet");

        // Set the fetched account in the LiteSVM environment
        // This allows us to simulate interactions with this account during testing
        program
            .set_account(
                payer.pubkey(),
                Account {
                    lamports: fetched_account.lamports,
                    data: fetched_account.data,
                    owner: Pubkey::from(fetched_account.owner.to_bytes()),
                    executable: fetched_account.executable,
                    rent_epoch: fetched_account.rent_epoch,
                },
            )
            .unwrap();

        msg!("Lamports of fetched account: {}", fetched_account.lamports);

        let maker = payer.pubkey();

        // Create two mints (Mint A and Mint B) with 6 decimal places and the maker as the authority
        let mint_a = CreateMint::new(&mut program, &payer)
            .decimals(6)
            .authority(&maker)
            .send()
            .unwrap();
        msg!("Mint A: {}\n", mint_a);

        let mint_b = CreateMint::new(&mut program, &payer)
            .decimals(6)
            .authority(&maker)
            .send()
            .unwrap();
        msg!("Mint B: {}\n", mint_b);

        // Create the maker's associated token account for Mint A
        let maker_ata_a = CreateAssociatedTokenAccount::new(&mut program, &payer, &mint_a)
            .owner(&maker)
            .send()
            .unwrap();
        msg!("Maker ATA A: {}\n", maker_ata_a);

        // Mint 1,000 tokens (with 6 decimal places) of Mint A to the maker's associated token account
        MintTo::new(&mut program, &payer, &mint_a, &maker_ata_a, 1000000000)
            .send()
            .unwrap();

        // Return the LiteSVM instance, payer keypair, both mints, and maker's ATA
        (program, payer, mint_a, mint_b, maker_ata_a)
    }

    /// Helper function to execute Make instruction
    /// Creates escrow and vault, deposits tokens from maker
    /// Returns: (escrow PDA, vault PDA)
    fn execute_make(
        program: &mut LiteSVM,
        payer: &Keypair,
        maker: Pubkey,
        mint_a: Pubkey,
        mint_b: Pubkey,
        maker_ata_a: Pubkey,
        seed: u64,
        deposit: u64,
        receive: u64,
        waiting_time: i64,
    ) -> (Pubkey, Pubkey) {
        // Derive the escrow PDA using maker's pubkey and seed
        let escrow = Pubkey::find_program_address(
            &[b"escrow", maker.as_ref(), &seed.to_le_bytes()],
            &PROGRAM_ID,
        )
        .0;

        // Derive the vault PDA (associated token account owned by escrow)
        let vault = associated_token::get_associated_token_address(&escrow, &mint_a);

        // Create Make instruction
        let make_ix = Instruction {
            program_id: PROGRAM_ID,
            accounts: crate::accounts::Make {
                maker: maker,
                mint_a: mint_a,
                mint_b: mint_b,
                maker_ata_a: maker_ata_a,
                escrow: escrow,
                vault: vault,
                associated_token_program: spl_associated_token_account::ID,
                token_program: TOKEN_PROGRAM_ID,
                system_program: SYSTEM_PROGRAM_ID,
            }
            .to_account_metas(None),
            data: crate::instruction::Make {
                deposit,
                seed,
                receive,
                waiting_time,
            }
            .data(),
        };

        // Create and send transaction
        let message = Message::new(&[make_ix], Some(&payer.pubkey()));
        let blockhash = program.latest_blockhash();
        let transaction = Transaction::new(&[&payer], message, blockhash);

        let tx = program.send_transaction(transaction).unwrap();
        msg!("Make transaction successful");
        msg!("CUs Consumed: {}", tx.compute_units_consumed);
        msg!("Tx Signature: {}", tx.signature);

        (escrow, vault)
    }

    /// Helper function to setup initial state and execute the Make instruction
    /// This creates the escrow and vault, deposits tokens
    /// Returns: (LiteSVM, payer, mint_a, mint_b, maker_ata_a, escrow PDA, vault PDA)
    fn setup_with_make(
        seed: u64,
        deposit: u64,
        receive: u64,
        waiting_time: i64,
    ) -> (LiteSVM, Keypair, Pubkey, Pubkey, Pubkey, Pubkey, Pubkey) {
        // Get initial setup (mints, maker_ata_a with tokens)
        let (mut program, payer, mint_a, mint_b, maker_ata_a) = setup();
        let maker = payer.pubkey();

        // Use helper to execute Make instruction
        let (escrow, vault) = execute_make(
            &mut program,
            &payer,
            maker,
            mint_a,
            mint_b,
            maker_ata_a,
            seed,
            deposit,
            receive,
            waiting_time,
        );

        // Return everything needed for subsequent tests
        (program, payer, mint_a, mint_b, maker_ata_a, escrow, vault)
    }

    #[test]
    fn test_make() {
        // Setup the test environment (mints and maker's ATA)
        let (mut program, payer, mint_a, mint_b, maker_ata_a) = setup();
        let maker = payer.pubkey();

        // Execute Make instruction using helper function
        let (escrow, vault) = execute_make(
            &mut program,
            &payer,
            maker,
            mint_a,
            mint_b,
            maker_ata_a,
            123u64,
            10,
            10,
            0,
        );

        msg!("Escrow PDA: {}\n", escrow);
        msg!("Vault PDA: {}\n", vault);

        // Verify the vault account and escrow account data after the "Make" instruction
        let vault_account = program.get_account(&vault).unwrap();
        let vault_data = spl_token::state::Account::unpack(&vault_account.data).unwrap();
        assert_eq!(vault_data.amount, 10, "Vault should have 10 tokens");
        assert_eq!(vault_data.owner, escrow, "Vault owner should be escrow PDA");
        assert_eq!(vault_data.mint, mint_a, "Vault mint should be mint_a");

        let escrow_account = program.get_account(&escrow).unwrap();
        let escrow_data =
            crate::state::Escrow::try_deserialize(&mut escrow_account.data.as_ref()).unwrap();
        assert_eq!(escrow_data.seed, 123u64, "Escrow seed should be 123");
        assert_eq!(escrow_data.maker, maker, "Escrow maker should match");
        assert_eq!(escrow_data.mint_a, mint_a, "Escrow mint_a should match");
        assert_eq!(escrow_data.mint_b, mint_b, "Escrow mint_b should match");
        assert_eq!(
            escrow_data.receive, 10,
            "Escrow receive amount should be 10"
        );

        msg!("\nAll Make assertions passed!");
    }

    #[test]
    fn test_refund() {
        // Use helper function to setup and execute Make instruction
        // This gives us an escrow with deposited tokens ready to be refunded
        let (mut program, payer, mint_a, _mint_b, maker_ata_a, escrow, vault) =
            setup_with_make(123u64, 10, 10, 0);

        let maker = payer.pubkey();

        // Check maker's ATA_A balance before refund
        let maker_ata_before = program.get_account(&maker_ata_a).unwrap();
        let maker_ata_data_before =
            spl_token::state::Account::unpack(&maker_ata_before.data).unwrap();
        msg!(
            "Maker ATA balance before refund: {}",
            maker_ata_data_before.amount
        );

        // Execute Refund instruction
        let refund_ix = Instruction {
            program_id: PROGRAM_ID,
            accounts: crate::accounts::Refund {
                maker: maker,
                mint_a: mint_a,
                maker_ata_a: maker_ata_a,
                escrow: escrow,
                vault: vault,
                associated_token_program: spl_associated_token_account::ID,
                token_program: TOKEN_PROGRAM_ID,
                system_program: SYSTEM_PROGRAM_ID,
            }
            .to_account_metas(None),
            data: crate::instruction::Refund {}.data(),
        };

        let refund_message = Message::new(&[refund_ix], Some(&payer.pubkey()));
        let refund_blockhash = program.latest_blockhash();
        let refund_transaction = Transaction::new(&[&payer], refund_message, refund_blockhash);

        let refund_tx = program.send_transaction(refund_transaction).unwrap();
        msg!("\nRefund transaction successful");
        msg!("CUs Consumed: {}", refund_tx.compute_units_consumed);
        msg!("Tx Signature: {}", refund_tx.signature);

        // Verify the results

        // Check that tokens were returned to maker's ATA_A
        let maker_ata_after = program.get_account(&maker_ata_a).unwrap();
        let maker_ata_data_after =
            spl_token::state::Account::unpack(&maker_ata_after.data).unwrap();
        msg!(
            "Maker ATA balance after refund: {}",
            maker_ata_data_after.amount
        );
        assert_eq!(
            maker_ata_data_after.amount,
            maker_ata_data_before.amount + 10,
            "Tokens should be returned to maker"
        );

        msg!("\nAll refund assertions passed!");
    }

    #[test]
    fn test_take() {
        // Use helper function to setup and execute Make instruction
        // This creates the escrow with maker's tokens deposited
        let (mut program, payer, mint_a, mint_b, _maker_ata_a, escrow, vault) =
            setup_with_make(123u64, 10, 40, 0);

        let maker = payer.pubkey();

        // Setup TAKER (new user who will take the offer)
        let taker = Keypair::new();
        program
            .airdrop(&taker.pubkey(), 100 * LAMPORTS_PER_SOL)
            .unwrap();

        // Create taker's ATA for Mint A (to receive tokens from escrow)
        let taker_ata_a = CreateAssociatedTokenAccount::new(&mut program, &taker, &mint_a)
            .owner(&taker.pubkey())
            .send()
            .unwrap();
        msg!("Taker ATA A: {}\n", taker_ata_a);

        // Create taker's ATA for Mint B (to send tokens to maker)
        let taker_ata_b = CreateAssociatedTokenAccount::new(&mut program, &taker, &mint_b)
            .owner(&taker.pubkey())
            .send()
            .unwrap();
        msg!("Taker ATA B: {}\n", taker_ata_b);

        // Mint tokens to taker's ATA B (so taker has tokens to trade)
        MintTo::new(&mut program, &payer, &mint_b, &taker_ata_b, 1000000000)
            .send()
            .unwrap();


        let maker_ata_b =
            spl_associated_token_account::get_associated_token_address(&maker, &mint_b);
        msg!("Maker ATA B : {}\n", maker_ata_b);

        // Execute Take instruction
        let take_ix = Instruction {
            program_id: PROGRAM_ID,
            accounts: crate::accounts::Take {
                taker: taker.pubkey(),
                maker: maker,
                mint_a: mint_a,
                mint_b: mint_b,
                taker_ata_a: taker_ata_a,
                taker_ata_b: taker_ata_b,
                maker_ata_b: maker_ata_b,
                escrow: escrow,
                vault: vault,
                associated_token_program: spl_associated_token_account::ID,
                token_program: TOKEN_PROGRAM_ID,
                system_program: SYSTEM_PROGRAM_ID,
            }
            .to_account_metas(None),
            data: crate::instruction::Take {}.data(),
        };

        let take_message = Message::new(&[take_ix], Some(&taker.pubkey()));
        let take_blockhash = program.latest_blockhash();
        let take_transaction = Transaction::new(&[&taker], take_message, take_blockhash);

        let take_tx = program.send_transaction(take_transaction).unwrap();
        msg!("\nTake transaction successful");
        msg!("CUs Consumed: {}", take_tx.compute_units_consumed);
        msg!("Tx Signature: {}", take_tx.signature);

        // Verify the swap completed correctly

        // Check taker received 10 tokens of Mint A from vault
        let taker_ata_a_account = program.get_account(&taker_ata_a).unwrap();
        let taker_ata_a_data =
            spl_token::state::Account::unpack(&taker_ata_a_account.data).unwrap();
        assert_eq!(
            taker_ata_a_data.amount, 10,
            "Taker should receive 10 tokens of mint A"
        );

        // Check maker received 40 tokens of Mint B from taker
        let maker_ata_b_account = program.get_account(&maker_ata_b).unwrap();
        let maker_ata_b_data =
            spl_token::state::Account::unpack(&maker_ata_b_account.data).unwrap();
        assert_eq!(
            maker_ata_b_data.amount, 40,
            "Maker should receive 10 tokens of mint B"
        );

        msg!("\nAll Take assertions passed!");
    }

    #[test]
    fn test_take_with_waiting_time() {
        let waiting_time = 300i64;
        let (mut program, payer, mint_a, mint_b, _maker_ata_a, escrow, vault) =
            setup_with_make(123u64, 40, 90, waiting_time);

        let maker = payer.pubkey();

        // Taker setup
        let taker = Keypair::new();
        program
            .airdrop(&taker.pubkey(), 100 * LAMPORTS_PER_SOL)
            .unwrap();

        // Taker ATA A
        let taker_ata_a = CreateAssociatedTokenAccount::new(&mut program, &taker, &mint_a)
            .owner(&taker.pubkey())
            .send()
            .unwrap();
        
        // Taker ATA B
        let taker_ata_b = CreateAssociatedTokenAccount::new(&mut program, &taker, &mint_b)
            .owner(&taker.pubkey())
            .send()
            .unwrap();
        
        MintTo::new(&mut program, &payer, &mint_b, &taker_ata_b, 1000000000)
            .send()
            .unwrap();


        // Maker ATA B
        let maker_ata_b =
            spl_associated_token_account::get_associated_token_address(&maker, &mint_b);

        let clock: Clock = program.get_sysvar();
        let start_time = clock.unix_timestamp;

        // First attempt (should fail)
        let take_ix_before_waiting = Instruction {
            program_id: PROGRAM_ID,
            accounts: crate::accounts::Take {
                taker: taker.pubkey(),
                maker,
                mint_a,
                mint_b,
                taker_ata_a,
                taker_ata_b,
                maker_ata_b,
                escrow,
                vault,
                associated_token_program: spl_associated_token_account::ID,
                token_program: TOKEN_PROGRAM_ID,
                system_program: SYSTEM_PROGRAM_ID,
            }
            .to_account_metas(None),
            data: crate::instruction::Take {}.data(),
        };

        let msg_before_waiting = Message::new(&[take_ix_before_waiting], Some(&taker.pubkey()));
        let bh_before_waiting = program.latest_blockhash();
        let tx_before_waiting = Transaction::new(&[&taker], msg_before_waiting, bh_before_waiting);

        let result_before_waiting = program.send_transaction(tx_before_waiting);
        assert!(
            result_before_waiting.is_err(),
            "Take should fail before waiting time"
        );

        msg!("✓ Take failed before waiting time");

        let mut new_clock: Clock = program.get_sysvar();
        let current_slot = new_clock.slot;

        // 2. Update the timestamp for your program logic
        new_clock.unix_timestamp += waiting_time;

        // 3. Increment the slot relatively (ensures a new blockhash/signature)
        // This fixes the "AlreadyProcessed" error
        new_clock.slot = current_slot + 100;

        // 4. Push the updated sysvar back to the VM
        program.set_sysvar::<Clock>(&new_clock);

        program.warp_to_slot(new_clock.slot);

        msg!("New slot: {}", new_clock.slot);
        msg!("New timestamp: {}", new_clock.unix_timestamp);

        msg!("Old timestamp: {}", start_time,);
        msg!(
            "New timestamp: {} {}",
            new_clock.unix_timestamp,
            new_clock.slot
        );

        // Second take (should succeed)
        let take_ix = Instruction {
            program_id: PROGRAM_ID,
            accounts: crate::accounts::Take {
                taker: taker.pubkey(),
                maker,
                mint_a,
                mint_b,
                taker_ata_a,
                taker_ata_b,
                maker_ata_b,
                escrow,
                vault,
                associated_token_program: spl_associated_token_account::ID,
                token_program: TOKEN_PROGRAM_ID,
                system_program: SYSTEM_PROGRAM_ID,
            }
            .to_account_metas(None),
            data: crate::instruction::Take {}.data(),
        };

        let msg = Message::new(&[take_ix], Some(&taker.pubkey()));
        program.expire_blockhash();
        let new_bh = program.latest_blockhash();
        let tx = Transaction::new(&[&taker], msg, new_bh);

        program.send_transaction(tx).unwrap();
        msg!("✓ Take succeeded after waiting time");

        // Final assertions
        let taker_a_acc = program.get_account(&taker_ata_a).unwrap();
        let taker_a_data = spl_token::state::Account::unpack(&taker_a_acc.data).unwrap();
        assert_eq!(taker_a_data.amount, 40);

        let maker_b_acc = program.get_account(&maker_ata_b).unwrap();
        let maker_b_data = spl_token::state::Account::unpack(&maker_b_acc.data).unwrap();
        assert_eq!(maker_b_data.amount, 90);

        msg!("✓ All assertions passed");
    }
}
