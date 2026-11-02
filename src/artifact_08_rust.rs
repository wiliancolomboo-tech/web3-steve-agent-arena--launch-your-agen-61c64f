use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};
use crate::errors::ArenaError;
use crate::state::{ArenaConfig, MatchSession, MatchStatus, ARENA_CONFIG_SEED, MATCH_SESSION_SEED, VAULT_SEED};

#[derive(Accounts)]
#[instruction(match_id: u64)]
pub struct RegisterAgent<'info> {
    #[account(
        mut,
        seeds = [ARENA_CONFIG_SEED],
        bump = arena_config.bump,
        constraint = arena_config.is_active @ ArenaError::ArenaInactive
    )]
    pub arena_config: Account<'info, ArenaConfig>,

    #[account(
        init_if_needed,
        payer = agent_authority,
        space = MatchSession::LEN,
        seeds = [MATCH_SESSION_SEED, match_id.to_le_bytes().as_ref()],
        bump
    )]
    pub match_session: Account<'info, MatchSession>,

    #[account(
        init_if_needed,
        payer = agent_authority,
        seeds = [VAULT_SEED, match_id.to_le_bytes().as_ref()],
        bump,
        token::mint = usdc_mint,
        token::authority = match_session
    )]
    pub match_vault: Account<'info, TokenAccount>,

    pub usdc_mint: Account<'info, Mint>,

    #[account(
        mut,
        constraint = agent_token_account.owner == agent_authority.key(),
        constraint = agent_token_account.mint == arena_config.usdc_mint
    )]
    pub agent_token_account: Account<'info, TokenAccount>,

    #[account(mut)]
    pub agent_authority: Signer<'info>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

pub fn handler(ctx: Context<RegisterAgent>, match_id: u64, stake_amount: u64) -> Result<()> {
    let session = &mut ctx.accounts.match_session;
    let clock = Clock::get()?;

    if session.match_id == 0 && session.stake_amount == 0 {
        session.arena_config = ctx.accounts.arena_config.key();
        session.match_id = match_id;
        session.stake_amount = stake_amount;
        session.status = MatchStatus::RegistrationOpen;
        session.bump = ctx.bumps.match_session;
        session.vault_bump = ctx.bumps.match_vault;
    }

    require!(session.status == MatchStatus::RegistrationOpen, ArenaError::MatchNotOpen);
    require!(session.stake_amount == stake_amount, ArenaError::InvalidStakeAmount);

    let agent_key = ctx.accounts.agent_authority.key();
    if session.agent_alpha.agent_pubkey == Pubkey::default() {
        session.agent_alpha.agent_pubkey = agent_key;
    } else if session.agent_beta.agent_pubkey == Pubkey::default() {
        require!(session.agent_alpha.agent_pubkey != agent_key, ArenaError::AgentAlreadyRegistered);
        session.agent_beta.agent_pubkey = agent_key;
        session.status = MatchStatus::CommitPhase;
        session.commit_deadline = clock.unix_timestamp + 60; // 60s commit window
    } else {
        return err!(ArenaError::MatchFull);
    }

    // Escrow stake tokens into Match Vault
    let cpi_accounts = Transfer {
        from: ctx.accounts.agent_token_account.to_account_info(),
        to: ctx.accounts.match_vault.to_account_info(),
        authority: ctx.accounts.agent_authority.to_account_info(),
    };
    token::transfer(
        CpiContext::new(ctx.accounts.token_program.to_account_info(), cpi_accounts),
        stake_amount,
    )?;

    Ok(())
}