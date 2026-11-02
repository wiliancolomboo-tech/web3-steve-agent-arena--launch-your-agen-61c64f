use anchor_lang::prelude::*;
use crate::errors::ArenaError;
use crate::state::{MatchSession, MatchStatus, MATCH_SESSION_SEED};

#[derive(Accounts)]
#[instruction(match_id: u64)]
pub struct CommitAction<'info> {
    #[account(
        mut,
        seeds = [MATCH_SESSION_SEED, match_id.to_le_bytes().as_ref()],
        bump = match_session.bump
    )]
    pub match_session: Account<'info, MatchSession>,
    pub agent_authority: Signer<'info>,
}

pub fn handler(ctx: Context<CommitAction>, _match_id: u64, commitment_hash: [u8; 32]) -> Result<()> {
    let session = &mut ctx.accounts.match_session;
    let clock = Clock::get()?;

    require!(session.status == MatchStatus::CommitPhase, ArenaError::InvalidCommitPhase);
    require!(clock.unix_timestamp <= session.commit_deadline, ArenaError::InvalidCommitPhase);

    let agent_key = ctx.accounts.agent_authority.key();
    if session.agent_alpha.agent_pubkey == agent_key {
        session.agent_alpha.action_commitment = commitment_hash;
        session.agent_alpha.has_committed = true;
    } else if session.agent_beta.agent_pubkey == agent_key {
        session.agent_beta.action_commitment = commitment_hash;
        session.agent_beta.has_committed = true;
    } else {
        return err!(ArenaError::UnauthorizedAdmin);
    }

    if session.agent_alpha.has_committed && session.agent_beta.has_committed {
        session.status = MatchStatus::RevealPhase;
        session.reveal_deadline = clock.unix_timestamp + 60; // 60s reveal window
    }

    Ok(())
}