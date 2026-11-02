use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};

declare_id!("StvArNA8x3M2bZpk9YhXpQvG7qF4aU2yT9sZ6L8w1Rt");

pub const ARENA_CONFIG_SEED: &[u8] = b"arena_config";
pub const AGENT_SEED: &[u8] = b"agent";
pub const MATCH_SEED: &[u8] = b"match";
pub const VAULT_SEED: &[u8] = b"vault";

pub const MAX_ROUNDS: u8 = 5;
pub const MOVE_TIMEOUT_SECONDS: i64 = 60; // 60s turn deadline

#[program]
pub mod steve_arena {
    use super::*;

    /// Initialize the global Arena Configuration
    pub fn initialize_arena(
        ctx: Context<InitializeArena>,
        protocol_fee_bps: u16,
    ) -> Result<()> {
        require!(protocol_fee_bps <= 1000, ArenaError::FeeExceedsMaximum); // Max 10%
        let arena = &mut ctx.accounts.arena_config;
        arena.admin = ctx.accounts.admin.key();
        arena.stake_mint = ctx.accounts.stake_mint.key();
        arena.protocol_fee_bps = protocol_fee_bps;
        arena.total_matches = 0;
        arena.total_volume = 0;
        arena.bump = ctx.bumps.arena_config;
        arena.vault_bump = ctx.bumps.vault;

        emit!(ArenaInitializedEvent {
            admin: arena.admin,
            stake_mint: arena.stake_mint,
            protocol_fee_bps,
        });

        Ok(())
    }

    /// Register an autonomous agent into the Arena Registry
    pub fn register_agent(
        ctx: Context<RegisterAgent>,
        agent_handle: String,
        metadata_uri: String,
    ) -> Result<()> {
        require!(agent_handle.len() <= 32, ArenaError::HandleTooLong);
        require!(metadata_uri.len() <= 128, ArenaError::UriTooLong);

        let agent = &mut ctx.accounts.agent_profile;
        agent.owner = ctx.accounts.authority.key();
        agent.handle = agent_handle.clone();
        agent.metadata_uri = metadata_uri;
        agent.elo_score = 1200;
        agent.matches_played = 0;
        agent.matches_won = 0;
        agent.is_active = true;
        agent.bump = ctx.bumps.agent_profile;

        emit!(AgentRegisteredEvent {
            owner: agent.owner,
            handle: agent_handle,
            initial_elo: 1200,
        });

        Ok(())
    }

    /// Create and stake a new Arena match challenge
    pub fn create_challenge(
        ctx: Context<CreateChallenge>,
        match_id: u64,
        stake_amount: u64,
    ) -> Result<()> {
        require!(stake_amount > 0, ArenaError::ZeroStakeAmount);

        // Transfer initiator stake to program vault
        let cpi_ctx = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.initiator_token_account.to_account_info(),
                to: ctx.accounts.vault.to_account_info(),
                authority: ctx.accounts.initiator.to_account_info(),
            },
        );
        token::transfer(cpi_ctx, stake_amount)?;

        let match_account = &mut ctx.accounts.match_account;
        match_account.match_id = match_id;
        match_account.player_a = ctx.accounts.initiator.key();
        match_account.player_b = Pubkey::default();
        match_account.stake_per_player = stake_amount;
        match_account.current_turn = ctx.accounts.initiator.key();
        match_account.player_a_score = 0;
        match_account.player_b_score = 0;
        match_account.round_number = 1;
        match_account.last_action_timestamp = Clock::get()?.unix_timestamp;
        match_account.status = MatchStatus::WaitingForChallenger;
        match_account.bump = ctx.bumps.match_account;

        let arena = &mut ctx.accounts.arena_config;
        arena.total_matches = arena.total_matches.checked_add(1).ok_or(ArenaError::NumericOverflow)?;

        emit!(ChallengeCreatedEvent {
            match_id,
            initiator: ctx.accounts.initiator.key(),
            stake_amount,
        });

        Ok(())
    }

    /// Accept challenge and match the stake
    pub fn accept_challenge(ctx: Context<AcceptChallenge>) -> Result<()> {
        let match_account = &mut ctx.accounts.match_account;
        require!(
            match_account.status == MatchStatus::WaitingForChallenger,
            ArenaError::InvalidMatchStatus
        );
        require!(
            match_account.player_a != ctx.accounts.challenger.key(),
            ArenaError::CannotPlayAgainstSelf
        );

        // Stake challenger funds
        let cpi_ctx = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.challenger_token_account.to_account_info(),
                to: ctx.accounts.vault.to_account_info(),
                authority: ctx.accounts.challenger.to_account_info(),
            },
        );
        token::transfer(cpi_ctx, match_account.stake_per_player)?;

        match_account.player_b = ctx.accounts.challenger.key();
        match_account.status = MatchStatus::Active;
        match_account.last_action_timestamp = Clock::get()?.unix_timestamp;

        emit!(ChallengeAcceptedEvent {
            match_id: match_account.match_id,
            player_a: match_account.player_a,
            player_b: match_account.player_b,
        });

        Ok(())
    }

    /// Commit a tactical turn move (0 = Defend, 1 = Attack, 2 = Counter)
    pub fn play_turn(ctx: Context<PlayTurn>, move_code: u8) -> Result<()> {
        require!(move_code <= 2, ArenaError::InvalidMove);
        let current_time = Clock::get()?.unix_timestamp;
        let match_account = &mut ctx.accounts.match_account;

        require!(match_account.status == MatchStatus::Active, ArenaError::InvalidMatchStatus);
        require!(
            ctx.accounts.player.key() == match_account.current_turn,
            ArenaError::NotPlayerTurn
        );

        // Turn scoring logic:
        // Evaluates differential tactical scoring based on round mechanics
        if ctx.accounts.player.key() == match_account.player_a {
            match_account.player_a_score = match_account.player_a_score.saturating_add(u16::from(move_code) + 1);
            match_account.current_turn = match_account.player_b;
        } else {
            match_account.player_b_score = match_account.player_b_score.saturating_add(u16::from(move_code) + 1);
            match_account.current_turn = match_account.player_a;
            match_account.round_number = match_account.round_number.saturating_add(1);
        }

        match_account.last_action_timestamp = current_time;

        emit!(TurnPlayedEvent {
            match_id: match_account.match_id,
            player: ctx.accounts.player.key(),
            move_code,
            current_round: match_account.round_number,
        });

        // Conclude match if rounds are exhausted
        if match_account.round_number > MAX_ROUNDS {
            match_account.status = MatchStatus::Concluded;
            emit!(MatchCompletedEvent {
                match_id: match_account.match_id,
                winner: if match_account.player_a_score >= match_account.player_b_score {
                    match_account.player_a
                } else {
                    match_account.player_b
                },
                score_a: match_account.player_a_score,
                score_b: match_account.player_b_score,
            });
        }

        Ok(())
    }

    /// Settle funds to the declared winner with protocol fee distribution
    pub fn settle_match(ctx: Context<SettleMatch>) -> Result<()> {
        let match_account = &mut ctx.accounts.match_account;
        require!(
            match_account.status == MatchStatus::Concluded,
            ArenaError::MatchNotConcluded
        );

        let total_pot = match_account.stake_per_player.checked_mul(2).ok_or(ArenaError::NumericOverflow)?;
        let fee_bps = ctx.accounts.arena_config.protocol_fee_bps as u64;
        let protocol_fee = total_pot.checked_mul(fee_bps).unwrap() / 10_000;
        let winner_amount = total_pot.checked_sub(protocol_fee).ok_or(ArenaError::NumericOverflow)?;

        let winner = if match_account.player_a_score >= match_account.player_b_score {
            match_account.player_a
        } else {
            match_account.player_b
        };

        require!(ctx.accounts.winner.key() == winner, ArenaError::InvalidWinnerSpecified);

        let seeds = &[
            VAULT_SEED,
            ctx.accounts.arena_config.stake_mint.as_ref(),
            &[ctx.accounts.arena_config.vault_bump],
        ];
        let signer = &[&seeds[..]];

        // Pay winner
        let winner_transfer_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.vault.to_account_info(),
                to: ctx.accounts.winner_token_account.to_account_info(),
                authority: ctx.accounts.vault.to_account_info(),
            },
            signer,
        );
        token::transfer(winner_transfer_ctx, winner_amount)?;

        // Pay protocol fee
        if protocol_fee > 0 {
            let fee_transfer_ctx = CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.vault.to_account_info(),
                    to: ctx.accounts.fee_recipient_token_account.to_account_info(),
                    authority: ctx.accounts.vault.to_account_info(),
                },
                signer,
            );
            token::transfer(fee_transfer_ctx, protocol_fee)?;
        }

        match_account.status = MatchStatus::Settled;

        emit!(MatchSettledEvent {
            match_id: match_account.match_id,
            winner,
            payout: winner_amount,
            fee: protocol_fee,
        });

        Ok(())
    }

    /// Claim forfeit win if opponent times out past MOVE_TIMEOUT_SECONDS
    pub fn claim_timeout_forfeit(ctx: Context<ClaimTimeout>) -> Result<()> {
        let current_time = Clock::get()?.unix_timestamp;
        let match_account = &mut ctx.accounts.match_account;

        require!(match_account.status == MatchStatus::Active, ArenaError::InvalidMatchStatus);
        require!(
            current_time.saturating_sub(match_account.last_action_timestamp) > MOVE_TIMEOUT_SECONDS,
            ArenaError::TimeoutNotReached
        );

        let timed_out_player = match_account.current_turn;
        let claimant = ctx.accounts.claimant.key();

        require!(
            (claimant == match_account.player_a || claimant == match_account.player_b) &&
            claimant != timed_out_player,
            ArenaError::UnauthorizedClaimant
        );

        match_account.status = MatchStatus::Concluded;
        if claimant == match_account.player_a {
            match_account.player_a_score = 999;
        } else {
            match_account.player_b_score = 999;
        }

        emit!(ForfeitClaimedEvent {
            match_id: match_account.match_id,
            winner: claimant,
            timed_out_player,
        });

        Ok(())
    }
}

// -----------------------------------------------------------------------------
// ACCOUNT VALIDATION STRUCTS
// -----------------------------------------------------------------------------

#[derive(Accounts)]
pub fn initialize_arena_accounts<'info>(bump: u8, vault_bump: u8) {}

#[derive(Accounts)]
pub struct InitializeArena<'info> {
    #[account(
        init,
        payer = admin,
        space = 8 + ArenaConfig::LEN,
        seeds = [ARENA_CONFIG_SEED],
        bump
    )]
    pub arena_config: Account<'info, ArenaConfig>,
    pub stake_mint: Account<'info, Mint>,
    #[account(
        init,
        payer = admin,
        seeds = [VAULT_SEED, stake_mint.key().as_ref()],
        bump,
        token::mint = stake_mint,
        token::authority = vault
    )]
    pub vault: Account<'info, TokenAccount>,
    #[account(mut)]
    pub admin: Signer<'info>,
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct RegisterAgent<'info> {
    #[account(
        init,
        payer = authority,
        space = 8 + AgentProfile::LEN,
        seeds = [AGENT_SEED, authority.key().as_ref()],
        bump
    )]
    pub agent_profile: Account<'info, AgentProfile>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(match_id: u64)]
pub struct CreateChallenge<'info> {
    #[account(
        mut,
        seeds = [ARENA_CONFIG_SEED],
        bump = arena_config.bump
    )]
    pub arena_config: Account<'info, ArenaConfig>,
    #[account(
        init,
        payer = initiator,
        space = 8 + MatchAccount::LEN,
        seeds = [MATCH_SEED, match_id.to_le_bytes().as_ref()],
        bump
    )]
    pub match_account: Account<'info, MatchAccount>,
    #[account(
        mut,
        seeds = [VAULT_SEED, arena_config.stake_mint.as_ref()],
        bump = arena_config.vault_bump
    )]
    pub vault: Account<'info, TokenAccount>,
    #[account(
        mut,
        constraint = initiator_token_account.owner == initiator.key(),
        constraint = initiator_token_account.mint == arena_config.stake_mint
    )]
    pub initiator_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub initiator: Signer<'info>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct AcceptChallenge<'info> {
    #[account(
        seeds = [ARENA_CONFIG_SEED],
        bump = arena_config.bump
    )]
    pub arena_config: Account<'info, ArenaConfig>,
    #[account(
        mut,
        seeds = [MATCH_SEED, match_account.match_id.to_le_bytes().as_ref()],
        bump = match_account.bump
    )]
    pub match_account: Account<'info, MatchAccount>,
    #[account(
        mut,
        seeds = [VAULT_SEED, arena_config.stake_mint.as_ref()],
        bump = arena_config.vault_bump
    )]
    pub vault: Account<'info, TokenAccount>,
    #[account(
        mut,
        constraint = challenger_token_account.owner == challenger.key(),
        constraint = challenger_token_account.mint == arena_config.stake_mint
    )]
    pub challenger_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub challenger: Signer<'info>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct PlayTurn<'info> {
    #[account(
        mut,
        seeds = [MATCH_SEED, match_account.match_id.to_le_bytes().as_ref()],
        bump = match_account.bump
    )]
    pub match_account: Account<'info, MatchAccount>,
    pub player: Signer<'info>,
}

#[derive(Accounts)]
pub struct SettleMatch<'info> {
    #[account(
        seeds = [ARENA_CONFIG_SEED],
        bump = arena_config.bump
    )]
    pub arena_config: Account<'info, ArenaConfig>,
    #[account(
        mut,
        seeds = [MATCH_SEED, match_account.match_id.to_le_bytes().as_ref()],
        bump = match_account.bump
    )]
    pub match_account: Account<'info, MatchAccount>,
    #[account(
        mut,
        seeds = [VAULT_SEED, arena_config.stake_mint.as_ref()],
        bump = arena_config.vault_bump
    )]
    pub vault: Account<'info, TokenAccount>,
    /// CHECK: Validated against match winner calculation
    pub winner: AccountInfo<'info>,
    #[account(
        mut,
        constraint = winner_token_account.owner == winner.key(),
        constraint = winner_token_account.mint == arena_config.stake_mint
    )]
    pub winner_token_account: Account<'info, TokenAccount>,
    #[account(
        mut,
        constraint = fee_recipient_token_account.owner == arena_config.admin,
        constraint = fee_recipient_token_account.mint == arena_config.stake_mint
    )]
    pub fee_recipient_token_account: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct ClaimTimeout<'info> {
    #[account(
        mut,
        seeds = [MATCH_SEED, match_account.match_id.to_le_bytes().as_ref()],
        bump = match_account.bump
    )]
    pub match_account: Account<'info, MatchAccount>,
    pub claimant: Signer<'info>,
}

// -----------------------------------------------------------------------------
// STATE SCHEMAS & ENUMS
// -----------------------------------------------------------------------------

#[account]
pub struct ArenaConfig {
    pub admin: Pubkey,
    pub stake_mint: Pubkey,
    pub protocol_fee_bps: u16,
    pub total_matches: u64,
    pub total_volume: u64,
    pub bump: u8,
    pub vault_bump: u8,
}

impl ArenaConfig {
    pub const LEN: usize = 32 + 32 + 2 + 8 + 8 + 1 + 1;
}

#[account]
pub struct AgentProfile {
    pub owner: Pubkey,
    pub handle: String,
    pub metadata_uri: String,
    pub elo_score: u32,
    pub matches_played: u32,
    pub matches_won: u32,
    pub is_active: bool,
    pub bump: u8,
}

impl AgentProfile {
    pub const LEN: usize = 32 + (4 + 32) + (4 + 128) + 4 + 4 + 4 + 1 + 1;
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq)]
pub enum MatchStatus {
    WaitingForChallenger,
    Active,
    Concluded,
    Settled,
}

#[account]
pub struct MatchAccount {
    pub match_id: u64,
    pub player_a: Pubkey,
    pub player_b: Pubkey,
    pub stake_per_player: u64,
    pub current_turn: Pubkey,
    pub player_a_score: u16,
    pub player_b_score: u16,
    pub round_number: u8,
    pub last_action_timestamp: i64,
    pub status: MatchStatus,
    pub bump: u8,
}

impl MatchAccount {
    pub const LEN: usize = 8 + 32 + 32 + 8 + 32 + 2 + 2 + 1 + 8 + 1 + 1;
}

// -----------------------------------------------------------------------------
// EVENTS & ERRORS
// -----------------------------------------------------------------------------

#[event]
pub struct ArenaInitializedEvent {
    pub admin: Pubkey,
    pub stake_mint: Pubkey,
    pub protocol_fee_bps: u16,
}

#[event]
pub struct AgentRegisteredEvent {
    pub owner: Pubkey,
    pub handle: String,
    pub initial_elo: u32,
}

#[event]
pub struct ChallengeCreatedEvent {
    pub match_id: u64,
    pub initiator: Pubkey,
    pub stake_amount: u64,
}

#[event]
pub struct ChallengeAcceptedEvent {
    pub match_id: u64,
    pub player_a: Pubkey,
    pub player_b: Pubkey,
}

#[event]
pub struct TurnPlayedEvent {
    pub match_id: u64,
    pub player: Pubkey,
    pub move_code: u8,
    pub current_round: u8,
}

#[event]
pub struct MatchCompletedEvent {
    pub match_id: u64,
    pub winner: Pubkey,
    pub score_a: u16,
    pub score_b: u16,
}

#[event]
pub struct MatchSettledEvent {
    pub match_id: u64,
    pub winner: Pubkey,
    pub payout: u64,
    pub fee: u64,
}

#[event]
pub struct ForfeitClaimedEvent {
    pub match_id: u64,
    pub winner: Pubkey,
    pub timed_out_player: Pubkey,
}

#[error_code]
pub enum ArenaError {
    #[msg("Protocol fee exceeds maximum allowed limit (1000 bps).")]
    FeeExceedsMaximum,
    #[msg("Agent handle exceeds maximum length of 32 characters.")]
    HandleTooLong,
    #[msg("Metadata URI exceeds maximum length of 128 characters.")]
    UriTooLong,
    #[msg("Stake amount must be strictly greater than zero.")]
    ZeroStakeAmount,
    #[msg("Match is not in valid status for this action.")]
    InvalidMatchStatus,
    #[msg("An agent cannot play against itself.")]
    CannotPlayAgainstSelf,
    #[msg("Invalid move code. Must be 0 (Defend), 1 (Attack), or 2 (Counter).")]
    InvalidMove,
    #[msg("It is not your turn to play.")]
    NotPlayerTurn,
    #[msg("Match has not yet concluded.")]
    MatchNotConcluded,
    #[msg("Specified winner does not match the round outcome.")]
    InvalidWinnerSpecified,
    #[msg("Turn timeout duration has not yet expired.")]
    TimeoutNotReached,
    #[msg("Caller is unauthorized to claim timeout forfeit.")]
    UnauthorizedClaimant,
    #[msg("Arithmetic overflow occurred.")]
    NumericOverflow,
}