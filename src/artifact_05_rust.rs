use anchor_lang::prelude::*;
use anchor_spl::token::{self, CloseAccount, Token, TokenAccount, Transfer};

declare_id!("Vangd87wM2UvKzF5V64sXN9FpA3gJ7V1qWbL8aC9zXm");

#[program]
pub mod vanguard_arena {
    use super::*;

    /// Initializes a new agent profile inside the Steve Agent Arena.
    pub fn register_agent(
        ctx: Context<RegisterAgent>,
        agent_id: String,
        metadata_uri: String,
    ) -> Result<()> {
        require!(agent_id.len() <= 32, ArenaError::StringTooLong);
        require!(metadata_uri.len() <= 128, ArenaError::StringTooLong);

        let agent = &mut ctx.accounts.agent_profile;
        agent.authority = ctx.accounts.authority.key();
        agent.agent_id = agent_id;
        agent.metadata_uri = metadata_uri;
        agent.matches_played = 0;
        agent.matches_won = 0;
        agent.elo_rating = 1000;
        agent.is_active = true;
        agent.bump = ctx.bumps.agent_profile;

        emit!(AgentRegistered {
            authority: agent.authority,
            agent_id: agent.agent_id.clone(),
            timestamp: Clock::get()?.unix_timestamp,
        });

        Ok(())
    }

    /// Creates and escrows a new match challenge.
    pub fn initiate_match(
        ctx: Context<InitiateMatch>,
        match_id: u64,
        stake_amount: u64,
        action_hash: [u8; 32],
    ) -> Result<()> {
        require!(stake_amount > 0, ArenaError::InvalidStake);

        let match_state = &mut ctx.accounts.match_state;
        match_state.match_id = match_id;
        match_state.initiator = ctx.accounts.initiator.key();
        match_state.opponent = Pubkey::default();
        match_state.stake_amount = stake_amount;
        match_state.initiator_action_hash = action_hash;
        match_state.opponent_action_hash = [0u8; 32];
        match_state.status = MatchStatus::Initialized;
        match_state.created_at = Clock::get()?.unix_timestamp;
        match_state.bump = ctx.bumps.match_state;
        match_state.vault_bump = ctx.bumps.match_vault;

        // Escrow funds from initiator to match vault
        let cpi_accounts = Transfer {
            from: ctx.accounts.initiator_token_account.to_account_info(),
            to: ctx.accounts.match_vault.to_account_info(),
            authority: ctx.accounts.initiator.to_account_info(),
        };
        let cpi_ctx = CpiContext::new(ctx.accounts.token_program.to_account_info(), cpi_accounts);
        token::transfer(cpi_ctx, stake_amount)?;

        emit!(MatchInitiated {
            match_id,
            initiator: ctx.accounts.initiator.key(),
            stake_amount,
            timestamp: match_state.created_at,
        });

        Ok(())
    }

    /// Challenger joins the match and deposits matching stake.
    pub fn accept_match(
        ctx: Context<AcceptMatch>,
        action_hash: [u8; 32],
    ) -> Result<()> {
        let match_state = &mut ctx.accounts.match_state;
        require!(
            match_state.status == MatchStatus::Initialized,
            ArenaError::InvalidMatchStatus
        );
        require!(
            ctx.accounts.opponent.key() != match_state.initiator,
            ArenaError::SelfMatchProhibited
        );

        match_state.opponent = ctx.accounts.opponent.key();
        match_state.opponent_action_hash = action_hash;
        match_state.status = MatchStatus::Active;

        // Escrow matching funds
        let cpi_accounts = Transfer {
            from: ctx.accounts.opponent_token_account.to_account_info(),
            to: ctx.accounts.match_vault.to_account_info(),
            authority: ctx.accounts.opponent.to_account_info(),
        };
        let cpi_ctx = CpiContext::new(ctx.accounts.token_program.to_account_info(), cpi_accounts);
        token::transfer(cpi_ctx, match_state.stake_amount)?;

        emit!(MatchAccepted {
            match_id: match_state.match_id,
            opponent: ctx.accounts.opponent.key(),
            timestamp: Clock::get()?.unix_timestamp,
        });

        Ok(())
    }

    /// Resolves the match through reveal and deterministic scoring.
    pub fn settle_match(
        ctx: Context<SettleMatch>,
        initiator_secret: u64,
        initiator_action: u8,
        opponent_secret: u64,
        opponent_action: u8,
    ) -> Result<()> {
        let match_state = &mut ctx.accounts.match_state;
        require!(
            match_state.status == MatchStatus::Active,
            ArenaError::InvalidMatchStatus
        );

        // Verify initiator hash commitment
        let mut init_preimage = Vec::new();
        init_preimage.extend_from_slice(&initiator_secret.to_le_bytes());
        init_preimage.push(initiator_action);
        let calculated_init_hash = anchor_lang::solana_program::hash::hash(&init_preimage).to_bytes();
        require!(
            calculated_init_hash == match_state.initiator_action_hash,
            ArenaError::HashCommitmentMismatch
        );

        // Verify opponent hash commitment
        let mut opp_preimage = Vec::new();
        opp_preimage.extend_from_slice(&opponent_secret.to_le_bytes());
        opp_preimage.push(opponent_action);
        let calculated_opp_hash = anchor_lang::solana_program::hash::hash(&opp_preimage).to_bytes();
        require!(
            calculated_opp_hash == match_state.opponent_action_hash,
            ArenaError::HashCommitmentMismatch
        );

        // Deterministic winner resolution (Rock-Paper-Scissors Strategy Matrix: 0=Attack, 1=Defend, 2=Counter)
        // 0 beats 2, 2 beats 1, 1 beats 0
        let winner_pubkey: Option<Pubkey> = if initiator_action == opponent_action {
            None // Draw
        } else if (initiator_action == 0 && opponent_action == 2)
            || (initiator_action == 2 && opponent_action == 1)
            || (initiator_action == 1 && opponent_action == 0)
        {
            Some(match_state.initiator)
        } else {
            Some(match_state.opponent)
        };

        let match_id_bytes = match_state.match_id.to_le_bytes();
        let seeds = &[
            b"match_vault".as_ref(),
            match_id_bytes.as_ref(),
            &[match_state.vault_bump],
        ];
        let signer_seeds = &[&seeds[..]];

        let total_payout = match_state.stake_amount.checked_mul(2).ok_or(ArenaError::MathOverflow)?;

        if let Some(winner) = winner_pubkey {
            let destination = if winner == match_state.initiator {
                ctx.accounts.initiator_profile.matches_won += 1;
                ctx.accounts.initiator_profile.elo_rating += 25;
                if ctx.accounts.opponent_profile.elo_rating > 25 {
                    ctx.accounts.opponent_profile.elo_rating -= 20;
                }
                ctx.accounts.initiator_token_account.to_account_info()
            } else {
                ctx.accounts.opponent_profile.matches_won += 1;
                ctx.accounts.opponent_profile.elo_rating += 25;
                if ctx.accounts.initiator_profile.elo_rating > 25 {
                    ctx.accounts.initiator_profile.elo_rating -= 20;
                }
                ctx.accounts.opponent_token_account.to_account_info()
            };

            let cpi_accounts = Transfer {
                from: ctx.accounts.match_vault.to_account_info(),
                to: destination,
                authority: ctx.accounts.match_vault.to_account_info(),
            };
            let cpi_ctx = CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                cpi_accounts,
                signer_seeds,
            );
            token::transfer(cpi_ctx, total_payout)?;
            match_state.status = MatchStatus::Completed;
        } else {
            // Draw: Refund equal parts
            let cpi_init = Transfer {
                from: ctx.accounts.match_vault.to_account_info(),
                to: ctx.accounts.initiator_token_account.to_account_info(),
                authority: ctx.accounts.match_vault.to_account_info(),
            };
            token::transfer(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    cpi_init,
                    signer_seeds,
                ),
                match_state.stake_amount,
            )?;

            let cpi_opp = Transfer {
                from: ctx.accounts.match_vault.to_account_info(),
                to: ctx.accounts.opponent_token_account.to_account_info(),
                authority: ctx.accounts.match_vault.to_account_info(),
            };
            token::transfer(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    cpi_opp,
                    signer_seeds,
                ),
                match_state.stake_amount,
            )?;
            match_state.status = MatchStatus::Draw;
        }

        ctx.accounts.initiator_profile.matches_played += 1;
        ctx.accounts.opponent_profile.matches_played += 1;

        // Close match vault account to return rent
        let close_accounts = CloseAccount {
            account: ctx.accounts.match_vault.to_account_info(),
            destination: ctx.accounts.settler.to_account_info(),
            authority: ctx.accounts.match_vault.to_account_info(),
        };
        token::close_account(CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            close_accounts,
            signer_seeds,
        ))?;

        emit!(MatchSettled {
            match_id: match_state.match_id,
            winner: winner_pubkey,
            total_pot: total_payout,
            timestamp: Clock::get()?.unix_timestamp,
        });

        Ok(())
    }
}

#[derive(Accounts)]
#[instruction(agent_id: String)]
pub struct RegisterAgent<'info> {
    #[account(
        init,
        payer = authority,
        space = 8 + AgentProfile::MAX_SIZE,
        seeds = [b"agent", authority.key().as_ref(), agent_id.as_bytes()],
        bump
    )]
    pub agent_profile: Account<'info, AgentProfile>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(match_id: u64)]
pub struct InitiateMatch<'info> {
    #[account(
        init,
        payer = initiator,
        space = 8 + MatchState::MAX_SIZE,
        seeds = [b"match_state", match_id.to_le_bytes().as_ref()],
        bump
    )]
    pub match_state: Account<'info, MatchState>,
    #[account(
        init,
        payer = initiator,
        seeds = [b"match_vault", match_id.to_le_bytes().as_ref()],
        bump,
        token::mint = token_mint,
        token::authority = match_vault
    )]
    pub match_vault: Account<'info, TokenAccount>,
    pub token_mint: Account<'info, anchor_spl::token::Mint>,
    #[account(
        mut,
        constraint = initiator_token_account.owner == initiator.key(),
        constraint = initiator_token_account.mint == token_mint.key()
    )]
    pub initiator_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub initiator: Signer<'info>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct AcceptMatch<'info> {
    #[account(
        mut,
        seeds = [b"match_state", match_state.match_id.to_le_bytes().as_ref()],
        bump = match_state.bump
    )]
    pub match_state: Account<'info, MatchState>,
    #[account(
        mut,
        seeds = [b"match_vault", match_state.match_id.to_le_bytes().as_ref()],
        bump = match_state.vault_bump
    )]
    pub match_vault: Account<'info, TokenAccount>,
    #[account(
        mut,
        constraint = opponent_token_account.owner == opponent.key()
    )]
    pub opponent_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub opponent: Signer<'info>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct SettleMatch<'info> {
    #[account(
        mut,
        seeds = [b"match_state", match_state.match_id.to_le_bytes().as_ref()],
        bump = match_state.bump
    )]
    pub match_state: Account<'info, MatchState>,
    #[account(
        mut,
        seeds = [b"match_vault", match_state.match_id.to_le_bytes().as_ref()],
        bump = match_state.vault_bump
    )]
    pub match_vault: Account<'info, TokenAccount>,
    #[account(
        mut,
        seeds = [b"agent", match_state.initiator.as_ref(), initiator_profile.agent_id.as_bytes()],
        bump = initiator_profile.bump
    )]
    pub initiator_profile: Account<'info, AgentProfile>,
    #[account(
        mut,
        seeds = [b"agent", match_state.opponent.as_ref(), opponent_profile.agent_id.as_bytes()],
        bump = opponent_profile.bump
    )]
    pub opponent_profile: Account<'info, AgentProfile>,
    #[account(mut)]
    pub initiator_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub opponent_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub settler: Signer<'info>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[account]
pub struct AgentProfile {
    pub authority: Pubkey,
    pub agent_id: String,
    pub metadata_uri: String,
    pub matches_played: u64,
    pub matches_won: u64,
    pub elo_rating: u32,
    pub is_active: bool,
    pub bump: u8,
}

impl AgentProfile {
    pub const MAX_SIZE: usize = 32 + (4 + 32) + (4 + 128) + 8 + 8 + 4 + 1 + 1;
}

#[account]
pub struct MatchState {
    pub match_id: u64,
    pub initiator: Pubkey,
    pub opponent: Pubkey,
    pub stake_amount: u64,
    pub initiator_action_hash: [u8; 32],
    pub opponent_action_hash: [u8; 32],
    pub status: MatchStatus,
    pub created_at: i64,
    pub bump: u8,
    pub vault_bump: u8,
}

impl MatchState {
    pub const MAX_SIZE: usize = 8 + 32 + 32 + 8 + 32 + 32 + 1 + 8 + 1 + 1;
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq)]
pub enum MatchStatus {
    Initialized,
    Active,
    Completed,
    Draw,
    Cancelled,
}

#[event]
pub struct AgentRegistered {
    pub authority: Pubkey,
    pub agent_id: String,
    pub timestamp: i64,
}

#[event]
pub struct MatchInitiated {
    pub match_id: u64,
    pub initiator: Pubkey,
    pub stake_amount: u64,
    pub timestamp: i64,
}

#[event]
pub struct MatchAccepted {
    pub match_id: u64,
    pub opponent: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct MatchSettled {
    pub match_id: u64,
    pub winner: Option<Pubkey>,
    pub total_pot: u64,
    pub timestamp: i64,
}

#[error_code]
pub enum ArenaError {
    #[msg("Input string exceeds maximum allowed length")]
    StringTooLong,
    #[msg("Stake amount must be strictly greater than zero")]
    InvalidStake,
    #[msg("Invalid match status for this instruction")]
    InvalidMatchStatus,
    #[msg("An agent cannot initiate a challenge against itself")]
    SelfMatchProhibited,
    #[msg("Committed hash does not match revealed secret and action")]
    HashCommitmentMismatch,
    #[msg("Arithmetic overflow detected in pot distribution calculation")]
    MathOverflow,
}