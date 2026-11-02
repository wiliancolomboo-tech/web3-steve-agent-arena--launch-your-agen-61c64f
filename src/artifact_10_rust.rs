use anchor_lang::prelude::*;
use anchor_lang::solana_program::hash::hashv;
use crate::errors::ArenaError;
use crate::state::{MatchSession, MatchStatus, MATCH_SESSION_SEED};

#[derive(Accounts)]
#[instruction(match_id: u64)]
pub struct RevealAction<'info> {
    #[account(
        mut,
        seeds = [MATCH_SESSION_SEED, match_id.to_le_bytes().as_ref()],
        bump = match_session.bump
    )]
    pub match_session: Account<'info, MatchSession>,
    pub agent_authority: Signer<'info>,
}

pub fn handler(ctx: Context<RevealAction>, _match_id: u64, action: u8, salt: [u8; 32]) -> Result<()> {
    let session = &mut ctx.accounts.match_session;
    let clock = Clock::get()?;

    require!(session.status == MatchStatus::RevealPhase, ArenaError::InvalidRevealPhase);
    require!(clock.unix_timestamp <= session.reveal_deadline, ArenaError::InvalidRevealPhase);

    let computed_hash = hashv(&[&[action], &salt]).to_bytes();
    let agent_key = ctx.accounts.agent_authority.key();

    if session.agent_alpha.agent_pubkey == agent_key {
        require!(!session.agent_alpha.has_revealed, ArenaError::ActionAlreadyRevealed);
        require!(session.agent_alpha.action_commitment == computed_hash, ArenaError::ActionHashMismatch);
        session.agent_alpha.revealed_action = action;
        session.agent_alpha.has_revealed = true;
    } else if session.agent_beta.agent_pubkey == agent_key {
        require!(!session.agent_beta.has_revealed, ArenaError::ActionAlreadyRevealed);
        require!(session.agent_beta.action_commitment == computed_hash, ArenaError::ActionHashMismatch);
        session.agent_beta.revealed_action = action;
        session.agent_beta.has_revealed = true;
    } else {
        return err!(ArenaError::UnauthorizedAdmin);
    }

    Ok(())
}