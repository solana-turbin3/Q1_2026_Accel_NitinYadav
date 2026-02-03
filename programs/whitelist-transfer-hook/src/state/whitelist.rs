use anchor_lang::prelude::*;

// The account's existence itself means the user is whitelisted
#[account]
#[derive(InitSpace)]
pub struct Whitelist {
    pub bump: u8,
}