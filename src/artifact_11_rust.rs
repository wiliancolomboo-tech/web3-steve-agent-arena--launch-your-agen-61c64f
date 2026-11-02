use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Transfer};
use crate::errors::ArenaError;
use crate::state::{ArenaConfig, MatchSession, MatchStatus, ARENA_CONFIG_SEED, MATCH_SESSION_SEED, VAULT_SEED};

#[derive(Accounts)]
#[instruction(match_id: u64)]
pub struct SettleMatch<'info> {
    #[account(
        mut,
        seeds = [ARENA_CONFIG_SEED],
        bump = arena_config.bump
    )]
    pub arena_config: Account<'info, ArenaConfig>,

    #[account(
        mut,
        seeds = [MATCH_SESSION_SEED, match_id.to_le_bytes().as_ref()],
        bump = match_session.bump
    )]
    pub match_session: Account<'info, MatchSession>,

    #[account(
        mut,
        seeds = [VAULT_SEED, match_id.to_le_bytes().as_ref()],
        bump = match_session.vault_bump
    )]
    pub match_vault: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = winner_token_account.owner == match_session.winner
    )]
    pub winner_token_account: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = treasury_token_account.owner == arena_config.admin
    )]
    pub treasury_token_account: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}

pub fn handler(ctx: Context<SettleMatch>, match_id: u64) -> Result<()> {
    let session = &mut ctx.accounts.match_session;
    let config = &mut ctx.accounts.arena_config;

    require!(session.status == MatchStatus::RevealPhase, ArenaError::MatchNotReadyForSettlement);

    // Dynamic Game Theory Rule Matrix (Rock-Paper-Scissors / Matrix Payoff)
    // 0: Cooperative Arb, 1: Aggressive Front-Run, 2: Liquidity Defense
    let a_act = session.agent_alpha.revealed_action;
    let b_act = session.agent_beta.revealed_action;

    let winner_pubkey = if session.agent_alpha.has_revealed && !session.agent_beta.has_revealed {
        session.agent_alpha.agent_pubkey
    } else if !session.agent_alpha.has_revealed && session.agent_beta.has_revealed {
        session.agent_beta.agent_pubkey
    } else if session.agent_alpha.has_revealed && session.agent_beta.has_revealed {
        if a_act == b_act {
            // Draw splits or defaults to alpha priority in tie-break
            session.agent_alpha.agent_pubkey
        } else if (a_act == 0 && b_act == 1) || (a_act == 1 && b_act == 2) || (a_act == 2 && b_act == 0) {
            session.agent_alpha.agent_pubkey
        } else {
            session.agent_beta.agent_pubkey
        }
    } else {
        return err!(ArenaError::MatchNotReadyForSettlement);
    };

    session.winner = winner_pubkey;
    session.status = MatchStatus::Settled;
    config.total_matches_played = config.total_matches_played.checked_add(1).unwrap();

    let total_pool = ctx.accounts.match_vault.amount;
    let fee = (total_pool * config.protocol_fee_bps as u64) / 10000;
    let prize = total_pool.checked_sub(fee).unwrap();

    let match_id_bytes = match_id.to_le_bytes();
    let signer_seeds: &[&[&[u8]]] = &[&[
        MATCH_SESSION_SEED,
        match_id_bytes.as_ref(),
        &[session.bump],
    ]];

    // Transfer Protocol Fee
    if fee > 0 {
        let fee_transfer = Transfer {
            from: ctx.accounts.match_vault.to_account_info(),
            to: ctx.accounts.treasury_token_account.to_account_info(),
            authority: session.to_account_info(),
        };
        token::transfer(
            CpiContext::new_with_signer(ctx.accounts.token_program.to_account_info(), fee_transfer, signer_seeds),
            fee,
        )?;
    }

    // Transfer Winner Prize
    let prize_transfer = Transfer {
        from: ctx.accounts.match_vault.to_account_info(),
        to: ctx.accounts.winner_token_account.to_account_info(),
        authority: session.to_account_info(),
    };
    token::transfer(
        CpiContext::new_with_signer(ctx.accounts.token_program.to_account_info(), prize_transfer, signer_seeds),
        prize,
    )?;

    Ok(())
}