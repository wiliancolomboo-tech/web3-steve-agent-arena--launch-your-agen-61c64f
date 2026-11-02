use anchor_lang::prelude::*;
use anchor_spl::token::Mint;
use crate::state::{ArenaConfig, ARENA_CONFIG_SEED};

#[derive(Accounts)]
pub struct InitializeArena<'info> {
    #[account(
        init,
        payer = admin,
        space = ArenaConfig::LEN,
        seeds = [ARENA_CONFIG_SEED],
        bump
    )]
    pub arena_config: Account<'info, ArenaConfig>,
    pub usdc_mint: Account<'info, Mint>,
    #[account(mut)]
    pub admin: Signer<'info>,
    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<InitializeArena>, protocol_fee_bps: u16) -> Result<()> {
    let arena_config = &mut ctx.accounts.arena_config;
    arena_config.admin = ctx.accounts.admin.key();
    arena_config.usdc_mint = ctx.accounts.usdc_mint.key();
    arena_config.total_matches_played = 0;
    arena_config.protocol_fee_bps = protocol_fee_bps;
    arena_config.is_active = true;
    arena_config.bump = ctx.bumps.arena_config;
    Ok(())
}