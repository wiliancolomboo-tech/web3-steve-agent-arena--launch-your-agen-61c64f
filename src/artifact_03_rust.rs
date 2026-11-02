use anchor_lang::prelude::*;
use anchor_lang::solana_program::clock::Clock;

declare_id!("StvArn1111111111111111111111111111111111111");

#[program]
pub mod steve_agent_arena {
    use super::*;

    /// Initializes the Arena global configuration and Treasury PDA
    pub fn initialize_arena(
        ctx: Context<InitializeArena>,
        protocol_fee_bps: u16,
    ) -> Result<()> {
        require!(protocol_fee_bps <= 1000, ArenaError::ProtocolFeeTooHigh); // Max 10%
        let arena_config = &mut ctx.accounts.arena_config;
        arena_config.authority = ctx.accounts.authority.key();
        arena_config.treasury = ctx.accounts.treasury.key();
        arena_config.protocol_fee_bps = protocol_fee_bps;
        arena_config.total_matches = 0;
        arena_config.is_paused = false;

        emit!(ArenaInitializedEvent {
            authority: arena_config.authority,
            treasury: arena_config.treasury,
            protocol_fee_bps,
        });

        Ok(())
    }

    /// Registers an autonomous agent profile in the arena
    pub fn register_agent(
        ctx: Context<RegisterAgent>,
        agent_id: String,
        metadata_uri: String,
    ) -> Result<()> {
        require!(agent_id.len() <= 32, ArenaError::AgentIdTooLong);
        require!(metadata_uri.len() <= 128, ArenaError::MetadataUriTooLong);

        let agent = &mut ctx.accounts.agent_profile;
        agent.owner = ctx.accounts.owner.key();
        agent.agent_id = agent_id.clone();
        agent.metadata_uri = metadata_uri;
        agent.elo_rating = 1200; // Base ELO rating
        agent.matches_won = 0;
        agent.matches_lost = 0;
        agent.registered_at = Clock::get()?.unix_timestamp;

        emit!(AgentRegisteredEvent {
            agent: agent.key(),
            owner: agent.owner,
            agent_id,
        });

        Ok(())
    }

    /// Creates a 1v1 match escrow. Player 1 deposits the bounty amount.
    pub fn create_match(
        ctx: Context<CreateMatch>,
        match_id: u64,
        wager_amount: u64,
        round_duration_slots: u64,
    ) -> Result<()> {
        require!(!ctx.accounts.arena_config.is_paused, ArenaError::ArenaPaused);
        require!(wager_amount > 0, ArenaError::InvalidWagerAmount);
        require!(round_duration_slots >= 10, ArenaError::DurationTooShort);

        let match_state = &mut ctx.accounts.match_state;
        let clock = Clock::get()?;

        match_state.match_id = match_id;
        match_state.player_one = ctx.accounts.player_one.key();
        match_state.player_two = Pubkey::default();
        match_state.wager_amount = wager_amount;
        match_state.status = MatchStatus::WaitingForOpponent;
        match_state.round_duration_slots = round_duration_slots;
        match_state.round_start_slot = clock.slot;
        match_state.player_one_commit = [0u8; 32];
        match_state.player_two_commit = [0u8; 32];
        match_state.player_one_move = 0;
        match_state.player_two_move = 0;
        match_state.winner = Pubkey::default();

        // Escrow wager from Player 1 into match PDA
        anchor_lang::solana_program::program::invoke(
            &anchor_lang::solana_program::system_instruction::transfer(
                &ctx.accounts.player_one.key(),
                &match_state.key(),
                wager_amount,
            ),
            &[
                ctx.accounts.player_one.to_account_info(),
                match_state.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
        )?;

        let config = &mut ctx.accounts.arena_config;
        config.total_matches = config.total_matches.checked_add(1).unwrap();

        emit!(MatchCreatedEvent {
            match_id,
            creator: ctx.accounts.player_one.key(),
            wager_amount,
        });

        Ok(())
    }

    /// Player 2 joins the match, matching the wager
    pub fn join_match(ctx: Context<JoinMatch>) -> Result<()> {
        require!(!ctx.accounts.arena_config.is_paused, ArenaError::ArenaPaused);
        let match_state = &mut ctx.accounts.match_state;

        require!(
            match_state.status == MatchStatus::WaitingForOpponent,
            ArenaError::InvalidMatchStatus
        );
        require!(
            ctx.accounts.player_two.key() != match_state.player_one,
            ArenaError::CannotPlaySelf
        );

        match_state.player_two = ctx.accounts.player_two.key();
        match_state.status = MatchStatus::CommitPhase;
        match_state.round_start_slot = Clock::get()?.slot;

        // Escrow wager from Player 2 into match PDA
        anchor_lang::solana_program::program::invoke(
            &anchor_lang::solana_program::system_instruction::transfer(
                &ctx.accounts.player_two.key(),
                &match_state.key(),
                match_state.wager_amount,
            ),
            &[
                ctx.accounts.player_two.to_account_info(),
                match_state.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
        )?;

        emit!(MatchJoinedEvent {
            match_id: match_state.match_id,
            opponent: match_state.player_two,
        });

        Ok(())
    }

    /// Submits a hashed move commitment (SHA-256 of move + secret nonce)
    pub fn commit_move(ctx: Context<CommitMove>, action_hash: [u8; 32]) -> Result<()> {
        let match_state = &mut ctx.accounts.match_state;
        let caller = ctx.accounts.player.key();
        let clock = Clock::get()?;

        require!(
            match_state.status == MatchStatus::CommitPhase,
            ArenaError::NotInCommitPhase
        );
        require!(
            clock.slot <= match_state.round_start_slot.checked_add(match_state.round_duration_slots).unwrap(),
            ArenaError::CommitTimeout
        );

        if caller == match_state.player_one {
            require!(match_state.player_one_commit == [0u8; 32], ArenaError::AlreadyCommitted);
            match_state.player_one_commit = action_hash;
        } else if caller == match_state.player_two {
            require!(match_state.player_two_commit == [0u8; 32], ArenaError::AlreadyCommitted);
            match_state.player_two_commit = action_hash;
        } else {
            return Err(ArenaError::UnauthorizedPlayer.into());
        }

        // If both have committed, advance to RevealPhase
        if match_state.player_one_commit != [0u8; 32] && match_state.player_two_commit != [0u8; 32] {
            match_state.status = MatchStatus::RevealPhase;
            match_state.round_start_slot = clock.slot;
        }

        emit!(MoveCommittedEvent {
            match_id: match_state.match_id,
            player: caller,
        });

        Ok(())
    }

    /// Reveals action and provides salt to prove hash match
    pub fn reveal_move(
        ctx: Context<RevealMove>,
        action: u8,
        salt: [u8; 16],
    ) -> Result<()> {
        let match_state = &mut ctx.accounts.match_state;
        let caller = ctx.accounts.player.key();

        require!(
            match_state.status == MatchStatus::RevealPhase,
            ArenaError::NotInRevealPhase
        );
        require!(action >= 1 && action <= 3, ArenaError::InvalidMove); // 1 = Attack, 2 = Defend, 3 = Counter

        // Verify SHA256(action || salt) matches commit
        let mut hasher = anchor_lang::solana_program::hash::Hasher::default();
        hasher.hash(&[action]);
        hasher.hash(&salt);
        let computed_hash = hasher.result().to_bytes();

        if caller == match_state.player_one {
            require!(match_state.player_one_commit == computed_hash, ArenaError::InvalidRevealHash);
            match_state.player_one_move = action;
        } else if caller == match_state.player_two {
            require!(match_state.player_two_commit == computed_hash, ArenaError::InvalidRevealHash);
            match_state.player_two_move = action;
        } else {
            return Err(ArenaError::UnauthorizedPlayer.into());
        }

        // If both moves revealed, resolve match
        if match_state.player_one_move != 0 && match_state.player_two_move != 0 {
            resolve_match_internal(
                match_state,
                &ctx.accounts.player_one_account,
                &ctx.accounts.player_two_account,
                &ctx.accounts.treasury,
                ctx.accounts.arena_config.protocol_fee_bps,
            )?;
        }

        emit!(MoveRevealedEvent {
            match_id: match_state.match_id,
            player: caller,
            action,
        });

        Ok(())
    }

    /// Timeout penalty if opponent stalls during reveal phase
    pub fn claim_timeout_victory(ctx: Context<ClaimTimeoutVictory>) -> Result<()> {
        let match_state = &mut ctx.accounts.match_state;
        let caller = ctx.accounts.player.key();
        let clock = Clock::get()?;

        require!(
            match_state.status == MatchStatus::RevealPhase,
            ArenaError::NotInRevealPhase
        );
        require!(
            clock.slot > match_state.round_start_slot.checked_add(match_state.round_duration_slots).unwrap(),
            ArenaError::TimeoutNotReached
        );

        let winner: Pubkey;
        if caller == match_state.player_one && match_state.player_one_move != 0 && match_state.player_two_move == 0 {
            winner = match_state.player_one;
        } else if caller == match_state.player_two && match_state.player_two_move != 0 && match_state.player_one_move == 0 {
            winner = match_state.player_two;
        } else {
            return Err(ArenaError::InvalidTimeoutClaim.into());
        }

        match_state.winner = winner;
        match_state.status = MatchStatus::Completed;

        // Disburse entire pot to the compliant player
        let total_pot = match_state.wager_amount.checked_mul(2).unwrap();
        **match_state.to_account_info().try_borrow_mut_lamports()? -= total_pot;
        **ctx.accounts.player.to_account_info().try_borrow_mut_lamports()? += total_pot;

        emit!(MatchFinalizedEvent {
            match_id: match_state.match_id,
            winner,
            payout: total_pot,
            is_draw: false,
        });

        Ok(())
    }
}

/// Game Resolution Engine:
/// 1 = Strike, 2 = Block, 3 = Feint
/// Rules:
/// Strike beats Feint (1 beats 3)
/// Feint beats Block (3 beats 2)
/// Block beats Strike (2 beats 1)
/// Same move = Draw
fn resolve_match_internal(
    match_state: &mut Account<MatchState>,
    p1_info: &AccountInfo,
    p2_info: &AccountInfo,
    treasury_info: &AccountInfo,
    protocol_fee_bps: u16,
) -> Result<()> {
    let m1 = match_state.player_one_move;
    let m2 = match_state.player_two_move;
    let total_pot = match_state.wager_amount.checked_mul(2).unwrap();

    let mut is_draw = false;
    let winner: Pubkey;

    if m1 == m2 {
        is_draw = true;
        winner = Pubkey::default();
        match_state.status = MatchStatus::Completed;
        match_state.winner = winner;

        // Refund both players
        **match_state.to_account_info().try_borrow_mut_lamports()? -= total_pot;
        **p1_info.try_borrow_mut_lamports()? += match_state.wager_amount;
        **p2_info.try_borrow_mut_lamports()? += match_state.wager_amount;
    } else {
        let p1_wins = (m1 == 1 && m2 == 3) || (m1 == 3 && m2 == 2) || (m1 == 2 && m2 == 1);
        winner = if p1_wins { match_state.player_one } else { match_state.player_two };

        match_state.status = MatchStatus::Completed;
        match_state.winner = winner;

        let fee = (total_pot as u128)
            .checked_mul(protocol_fee_bps as u128)
            .unwrap()
            .checked_div(10000)
            .unwrap() as u64;

        let winner_payout = total_pot.checked_sub(fee).unwrap();

        **match_state.to_account_info().try_borrow_mut_lamports()? -= total_pot;
        **treasury_info.try_borrow_mut_lamports()? += fee;

        if p1_wins {
            **p1_info.try_borrow_mut_lamports()? += winner_payout;
        } else {
            **p2_info.try_borrow_mut_lamports()? += winner_payout;
        }
    }

    emit!(MatchFinalizedEvent {
        match_id: match_state.match_id,
        winner,
        payout: if is_draw { 0 } else { total_pot },
        is_draw,
    });

    Ok(())
}

// -----------------------------------------------------------------------------
// ACCOUNT CONTEXTS
// -----------------------------------------------------------------------------

#[derive(Accounts)]
pub fn InitializeArena<'info>(mut ctx: Context<InitializeArena>) -> Result<()> {
    Ok(())
}

#[derive(Accounts)]
#[instruction(protocol_fee_bps: u16)]
pub struct InitializeArena<'info> {
    #[account(
        init,
        payer = authority,
        space = 8 + ArenaConfig::LEN,
        seeds = [b"arena_config"],
        bump
    )]
    pub arena_config: Account<'info, ArenaConfig>,
    #[account(mut)]
    pub authority: Signer<'info>,
    /// CHECK: Protocol Treasury recipient account
    pub treasury: AccountInfo<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(agent_id: String, metadata_uri: String)]
pub struct RegisterAgent<'info> {
    #[account(
        init,
        payer = owner,
        space = 8 + AgentProfile::LEN,
        seeds = [b"agent_profile", owner.key().as_ref(), agent_id.as_bytes()],
        bump
    )]
    pub agent_profile: Account<'info, AgentProfile>,
    #[account(mut)]
    pub owner: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(match_id: u64)]
pub struct CreateMatch<'info> {
    #[account(
        init,
        payer = player_one,
        space = 8 + MatchState::LEN,
        seeds = [b"arena_match", match_id.to_le_bytes().as_ref()],
        bump
    )]
    pub match_state: Account<'info, MatchState>,
    #[account(mut, seeds = [b"arena_config"], bump)]
    pub arena_config: Account<'info, ArenaConfig>,
    #[account(mut)]
    pub player_one: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct JoinMatch<'info> {
    #[account(
        mut,
        seeds = [b"arena_match", match_state.match_id.to_le_bytes().as_ref()],
        bump
    )]
    pub match_state: Account<'info, MatchState>,
    #[account(seeds = [b"arena_config"], bump)]
    pub arena_config: Account<'info, ArenaConfig>,
    #[account(mut)]
    pub player_two: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct CommitMove<'info> {
    #[account(
        mut,
        seeds = [b"arena_match", match_state.match_id.to_le_bytes().as_ref()],
        bump
    )]
    pub match_state: Account<'info, MatchState>,
    pub player: Signer<'info>,
}

#[derive(Accounts)]
pub struct RevealMove<'info> {
    #[account(
        mut,
        seeds = [b"arena_match", match_state.match_id.to_le_bytes().as_ref()],
        bump
    )]
    pub match_state: Account<'info, MatchState>,
    #[account(seeds = [b"arena_config"], bump)]
    pub arena_config: Account<'info, ArenaConfig>,
    pub player: Signer<'info>,
    /// CHECK: Verified against match_state.player_one
    #[account(mut, address = match_state.player_one)]
    pub player_one_account: AccountInfo<'info>,
    /// CHECK: Verified against match_state.player_two
    #[account(mut, address = match_state.player_two)]
    pub player_two_account: AccountInfo<'info>,
    /// CHECK: Verified against arena_config.treasury
    #[account(mut, address = arena_config.treasury)]
    pub treasury: AccountInfo<'info>,
}

#[derive(Accounts)]
pub struct ClaimTimeoutVictory<'info> {
    #[account(
        mut,
        seeds = [b"arena_match", match_state.match_id.to_le_bytes().as_ref()],
        bump
    )]
    pub match_state: Account<'info, MatchState>,
    #[account(mut)]
    pub player: Signer<'info>,
}

// -----------------------------------------------------------------------------
// STATE STRUCTS
// -----------------------------------------------------------------------------

#[account]
pub struct ArenaConfig {
    pub authority: Pubkey,
    pub treasury: Pubkey,
    pub protocol_fee_bps: u16,
    pub total_matches: u64,
    pub is_paused: bool,
}

impl ArenaConfig {
    pub const LEN: usize = 32 + 32 + 2 + 8 + 1;
}

#[account]
pub struct AgentProfile {
    pub owner: Pubkey,
    pub agent_id: String,
    pub metadata_uri: String,
    pub elo_rating: u32,
    pub matches_won: u32,
    pub matches_lost: u32,
    pub registered_at: i64,
}

impl AgentProfile {
    pub const LEN: usize = 32 + 36 + 132 + 4 + 4 + 4 + 8;
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq)]
pub enum MatchStatus {
    WaitingForOpponent,
    CommitPhase,
    RevealPhase,
    Completed,
    Cancelled,
}

#[account]
pub struct MatchState {
    pub match_id: u64,
    pub player_one: Pubkey,
    pub player_two: Pubkey,
    pub wager_amount: u64,
    pub status: MatchStatus,
    pub round_duration_slots: u64,
    pub round_start_slot: u64,
    pub player_one_commit: [u8; 32],
    pub player_two_commit: [u8; 32],
    pub player_one_move: u8,
    pub player_two_move: u8,
    pub winner: Pubkey,
}

impl MatchState {
    pub const LEN: usize = 8 + 32 + 32 + 8 + 1 + 8 + 8 + 32 + 32 + 1 + 1 + 32;
}

// -----------------------------------------------------------------------------
// EVENTS & ERRORS
// -----------------------------------------------------------------------------

#[event]
pub struct ArenaInitializedEvent {
    pub authority: Pubkey,
    pub treasury: Pubkey,
    pub protocol_fee_bps: u16,
}

#[event]
pub struct AgentRegisteredEvent {
    pub agent: Pubkey,
    pub owner: Pubkey,
    pub agent_id: String,
}

#[event]
pub struct MatchCreatedEvent {
    pub match_id: u64,
    pub creator: Pubkey,
    pub wager_amount: u64,
}

#[event]
pub struct MatchJoinedEvent {
    pub match_id: u64,
    pub opponent: Pubkey,
}

#[event]
pub struct MoveCommittedEvent {
    pub match_id: u64,
    pub player: Pubkey,
}

#[event]
pub struct MoveRevealedEvent {
    pub match_id: u64,
    pub player: Pubkey,
    pub action: u8,
}

#[event]
pub struct MatchFinalizedEvent {
    pub match_id: u64,
    pub winner: Pubkey,
    pub payout: u64,
    pub is_draw: bool,
}

#[error_code]
pub enum ArenaError {
    #[msg("Protocol fee exceeds maximum allowed threshold of 10%")]
    ProtocolFeeTooHigh,
    #[msg("Agent ID string length exceeds 32 characters")]
    AgentIdTooLong,
    #[msg("Metadata URI string length exceeds 128 characters")]
    MetadataUriTooLong,
    #[msg("Arena is currently paused for maintenance")]
    ArenaPaused,
    #[msg("Wager amount must be strictly greater than 0 lamports")]
    InvalidWagerAmount,
    #[msg("Round duration is below minimum threshold of 10 slots")]
    DurationTooShort,
    #[msg("Match is not in waiting state for an opponent")]
    InvalidMatchStatus,
    #[msg("Cannot initiate battle against own agent profile")]
    CannotPlaySelf,
    #[msg("Match is not currently in the Commit Phase")]
    NotInCommitPhase,
    #[msg("Commitment slot window expired")]
    CommitTimeout,
    #[msg("Move hash already committed for this round")]
    AlreadyCommitted,
    #[msg("Signer is not an authorized combatant in this match")]
    UnauthorizedPlayer,
    #[msg("Match is not currently in Reveal Phase")]
    NotInRevealPhase,
    #[msg("Invalid move action code. Allowed: 1 (Strike), 2 (Block), 3 (Feint)")]
    InvalidMove,
    #[msg("Provided action and salt do not hash to the committed value")]
    InvalidRevealHash,
    #[msg("Timeout duration has not yet expired")]
    TimeoutNotReached,
    #[msg("Invalid conditions for timeout claim")]
    InvalidTimeoutClaim,
}