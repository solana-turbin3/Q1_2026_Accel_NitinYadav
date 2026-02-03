use anchor_lang::prelude::*;

use crate::state::whitelist::Whitelist;

// Add user to whitelist (creates their PDA)
#[derive(Accounts)]
#[instruction(user: Pubkey)]
pub struct AddToWhitelist<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,
    #[account(
        init,
        payer = admin,
        space = 8+ Whitelist::INIT_SPACE,
        seeds = [b"whitelist", user.as_ref()],
        bump
    )]
    pub whitelist: Account<'info, Whitelist>,
    pub system_program: Program<'info, System>,
}

impl<'info> AddToWhitelist<'info> {
    pub fn add_to_whitelist(&mut self, bumps: &AddToWhitelistBumps, _user: Pubkey) -> Result<()> {
        self.whitelist.bump = bumps.whitelist;
        msg!("User added to whitelist");
        Ok(())
    }
}

// Remove user from whitelist (closes their PDA)
#[derive(Accounts)]
#[instruction(user: Pubkey)]
pub struct RemoveFromWhitelist<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,
    #[account(
        mut,
        close = admin,
        seeds = [b"whitelist", user.as_ref()],
        bump = whitelist.bump
    )]
    pub whitelist: Account<'info, Whitelist>,
}

impl<'info> RemoveFromWhitelist<'info> {
    pub fn remove_from_whitelist(&mut self, _user: Pubkey) -> Result<()> {
        msg!("User removed from whitelist");
        Ok(())
    }
}
