use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};

declare_id!("StvArn11111111111111111111111111111111111111");

pub const MAX_TURNS: u16 = 30;
pub const TURN_TIMEOUT_SLOTS: u64 = 150; // ~1 minute at 400ms slots
pub const BASE_K_FACTOR: u32 = 32;

#[program]
pub mod steve_agent_arena {
    use super::*;

    pub fn initialize_arena(
        ctx: Context<InitializeArena>,
        protocol_fee_bps: u16,
    ) -> Result<()> {
        require!(protocol_fee_bps <= 1000, ArenaError::FeeTooHigh); // Max 10%
        let config = &mut ctx.accounts.arena_config;
        config.admin = ctx.accounts.admin.key();
        config.usdc_mint = ctx.accounts.usdc_mint.key();
        config.protocol_fee_bps = protocol_fee_bps;
        config.total_matches = 0;
        config.bump = ctx.bumps.arena_config;
        emit!(ArenaInitializedEvent {
            admin: config.admin,
            usdc_mint: config.usdc_mint,
            protocol_fee_bps,
        });
        Ok(())
    }

    pub fn register_agent(
        ctx: Context<RegisterAgent>,
        name: String,
        metadata_uri: String,
        model_hash: [u8; 32],
    ) -> Result<()> {
        require!(name.as_bytes().len() <= 32, ArenaError::NameTooLong);
        require!(metadata_uri.as_bytes().len() <= 128, ArenaError::UriTooLong);

        let profile = &mut ctx.accounts.agent_profile;
        profile.authority = ctx.accounts.authority.key();
        profile.name = name;
        profile.metadata_uri = metadata_uri;
        profile.model_hash = model_hash;
        profile.elo_rating = 1000;
        profile.matches_played = 0;
        profile.matches_won = 0;
        profile.registered_at = Clock::get()?.unix_timestamp;
        profile.bump = ctx.bumps.agent_profile;

        emit!(AgentRegisteredEvent {
            authority: profile.authority,
            model_hash,
            initial_elo: profile.elo_rating,
        });
        Ok(())
    }

    pub fn create_match(
        ctx: Context<CreateMatch>,
        match_id: u64,
        stake_amount: u64,
    ) -> Result<()> {
        require!(stake_amount > 0, ArenaError::InvalidStakeAmount);

        let match_state = &mut ctx.accounts.match_state;
        let clock = Clock::get()?;

        match_state.match_id = match_id;
        match_state.agent_a = ctx.accounts.agent_a_profile.key();
        match_state.agent_b = ctx.accounts.agent_b_profile.key();
        match_state.agent_a_authority = ctx.accounts.agent_a_auth.key();
        match_state.agent_b_authority = ctx.accounts.agent_b_auth.key();
        match_state.stake_per_agent = stake_amount;
        match_state.current_turn = 0;
        match_state.active_agent = ctx.accounts.agent_a_auth.key();
        match_state.last_action_slot = clock.slot;
        match_state.status = MatchStatus::Active;
        match_state.bump = ctx.bumps.match_state;
        match_state.vault_bump = ctx.bumps.match_vault;

        // Initialize Agent Combat Stats
        match_state.stats_a = AgentStats {
            health: 100,
            energy: 100,
            defense_shield: 0,
        };
        match_state.stats_b = AgentStats {
            health: 100,
            energy: 100,
            defense_shield: 0,
        };

        // Escrow transfer Agent A stake
        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.agent_a_token.to_account_info(),
                    to: ctx.accounts.match_vault.to_account_info(),
                    authority: ctx.accounts.agent_a_auth.to_account_info(),
                },
            ),
            stake_amount,
        )?;

        // Escrow transfer Agent B stake
        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.agent_b_token.to_account_info(),
                    to: ctx.accounts.match_vault.to_account_info(),
                    authority: ctx.accounts.agent_b_auth.to_account_info(),
                },
            ),
            stake_amount,
        )?;

        let arena_config = &mut ctx.accounts.arena_config;
        arena_config.total_matches = arena_config
            .total_matches
            .checked_add(1)
            .ok_or(ArenaError::ArithmeticOverflow)?;

        emit!(MatchCreatedEvent {
            match_id,
            agent_a: match_state.agent_a,
            agent_b: match_state.agent_b,
            total_pot: stake_amount.checked_mul(2).unwrap(),
            start_slot: clock.slot,
        });

        Ok(())
    }

    pub fn execute_agent_action(
        ctx: Context<ExecuteAction>,
        action_type: u8,
        power_allocation: u8,
    ) -> Result<()> {
        let match_state = &mut ctx.accounts.match_state;
        let clock = Clock::get()?;

        require!(match_state.status == MatchStatus::Active, ArenaError::MatchNotActive);
        require!(
            ctx.accounts.agent_auth.key() == match_state.active_agent,
            ArenaError::NotYourTurn
        );
        require!(power_allocation <= 100, ArenaError::InvalidPowerAllocation);

        let is_agent_a = ctx.accounts.agent_auth.key() == match_state.agent_a_authority;

        {
            let (acting_stats, target_stats) = if is_agent_a {
                (&mut match_state.stats_a, &mut match_state.stats_b)
            } else {
                (&mut match_state.stats_b, &mut match_state.stats_a)
            };

            // Calculate cost and effectiveness based on power allocation
            let energy_cost = (power_allocation as u16 * 40 / 100) as u8;
            require!(acting_stats.energy >= energy_cost, ArenaError::InsufficientEnergy);
            acting_stats.energy = acting_stats.energy.saturating_sub(energy_cost);

            match action_type {
                0 => {
                    // Strike Action: Deals damage modified by defender's shield
                    let raw_damage = ((power_allocation as u16 * 35) / 100) as u8 + 10;
                    let mitigated_damage = raw_damage.saturating_sub(target_stats.defense_shield);
                    target_stats.defense_shield = target_stats.defense_shield.saturating_sub(raw_damage);
                    target_stats.health = target_stats.health.saturating_sub(mitigated_damage);
                }
                1 => {
                    // Fortify Action: Restores defense shield
                    let shield_boost = ((power_allocation as u16 * 30) / 100) as u8 + 15;
                    acting_stats.defense_shield = acting_stats.defense_shield.saturating_add(shield_boost).min(80);
                }
                2 => {
                    // Rebalance/Charge Action: Restores energy
                    let energy_recharge = ((power_allocation as u16 * 50) / 100) as u8 + 20;
                    acting_stats.energy = acting_stats.energy.saturating_add(energy_recharge).min(100);
                }
                _ => return err!(ArenaError::UnknownActionType),
            }
        }

        match_state.current_turn = match_state.current_turn.checked_add(1).unwrap();
        match_state.last_action_slot = clock.slot;

        // Evaluate victory conditions
        if match_state.stats_b.health == 0 {
            match_state.status = MatchStatus::Completed;
            match_state.winner = Some(match_state.agent_a_authority);
        } else if match_state.stats_a.health == 0 {
            match_state.status = MatchStatus::Completed;
            match_state.winner = Some(match_state.agent_b_authority);
        } else if match_state.current_turn >= MAX_TURNS {
            // Draw resolution: agent with highest combined HP + Shield wins
            let score_a = match_state.stats_a.health as u16 + match_state.stats_a.defense_shield as u16;
            let score_b = match_state.stats_b.health as u16 + match_state.stats_b.defense_shield as u16;

            match_state.status = MatchStatus::Completed;
            if score_a >= score_b {
                match_state.winner = Some(match_state.agent_a_authority);
            } else {
                match_state.winner = Some(match_state.agent_b_authority);
            }
        } else {
            // Toggle active turn
            match_state.active_agent = if is_agent_a {
                match_state.agent_b_authority
            } else {
                match_state.agent_a_authority
            };
        }

        emit!(ActionExecutedEvent {
            match_id: match_state.match_id,
            turn: match_state.current_turn,
            agent: ctx.accounts.agent_auth.key(),
            action_type,
            power_allocation,
            health_a: match_state.stats_a.health,
            health_b: match_state.stats_b.health,
        });

        Ok(())
    }

    pub fn resolve_turn_timeout(ctx: Context<ResolveTurnTimeout>) -> Result<()> {
        let match_state = &mut ctx.accounts.match_state;
        let clock = Clock::get()?;

        require!(match_state.status == MatchStatus::Active, ArenaError::MatchNotActive);
        require!(
            clock.slot.saturating_sub(match_state.last_action_slot) >= TURN_TIMEOUT_SLOTS,
            ArenaError::TurnNotTimedOut
        );

        // Stalled agent forfeits immediately
        let forfeited_agent = match_state.active_agent;
        let victorious_authority = if forfeited_agent == match_state.agent_a_authority {
            match_state.agent_b_authority
        } else {
            match_state.agent_a_authority
        };

        match_state.status = MatchStatus::Completed;
        match_state.winner = Some(victorious_authority);

        emit!(MatchForfeitedTimeoutEvent {
            match_id: match_state.match_id,
            forfeited_agent,
            winner: victorious_authority,
            slashed_at_slot: clock.slot,
        });

        Ok(())
    }

    pub fn settle_and_disburse(ctx: Context<SettleAndDisburse>) -> Result<()> {
        let match_state = &mut ctx.accounts.match_state;
        require!(match_state.status == MatchStatus::Completed, ArenaError::MatchNotCompleted);

        let winner = match_state.winner.ok_or(ArenaError::WinnerNotDetermined)?;
        let total_pot = match_state.stake_per_agent.checked_mul(2).unwrap();

        let fee_bps = ctx.accounts.arena_config.protocol_fee_bps as u64;
        let protocol_fee = (total_pot.checked_mul(fee_bps).unwrap()) / 10000;
        let winner_payout = total_pot.checked_sub(protocol_fee).unwrap();

        let match_id_bytes = match_state.match_id.to_le_bytes();
        let seeds: &[&[u8]] = &[
            b"match_vault".as_ref(),
            match_id_bytes.as_ref(),
            &[match_state.vault_bump],
        ];
        let signer = &[&seeds[..]];

        // Disburse protocol fee
        if protocol_fee > 0 {
            token::transfer(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    Transfer {
                        from: ctx.accounts.match_vault.to_account_info(),
                        to: ctx.accounts.protocol_fee_treasury.to_account_info(),
                        authority: ctx.accounts.match_vault.to_account_info(),
                    },
                    signer,
                ),
                protocol_fee,
            )?;
        }

        // Disburse winner proceeds
        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.match_vault.to_account_info(),
                    to: ctx.accounts.winner_token_account.to_account_info(),
                    authority: ctx.accounts.match_vault.to_account_info(),
                },
                signer,
            ),
            winner_payout,
        )?;

        // Update Elo Ratings
        let prof_a = &mut ctx.accounts.agent_a_profile;
        let prof_b = &mut ctx.accounts.agent_b_profile;

        prof_a.matches_played = prof_a.matches_played.checked_add(1).unwrap();
        prof_b.matches_played = prof_b.matches_played.checked_add(1).unwrap();

        if winner == prof_a.authority {
            prof_a.matches_won = prof_a.matches_won.checked_add(1).unwrap();
            prof_a.elo_rating = prof_a.elo_rating.saturating_add(BASE_K_FACTOR);
            prof_b.elo_rating = prof_b.elo_rating.saturating_sub(BASE_K_FACTOR);
        } else {
            prof_b.matches_won = prof_b.matches_won.checked_add(1).unwrap();
            prof_b.elo_rating = prof_b.elo_rating.saturating_add(BASE_K_FACTOR);
            prof_a.elo_rating = prof_a.elo_rating.saturating_sub(BASE_K_FACTOR);
        }

        emit!(MatchSettledEvent {
            match_id: match_state.match_id,
            winner,
            payout: winner_payout,
            protocol_fee,
        });

        Ok(())
    }
}

// ---------------------------------------------------------
// Account Validation Structs
// ---------------------------------------------------------

#[derive(Accounts)]
pub struct InitializeArena<'info> {
    #[account(
        init,
        payer = admin,
        space = 8 + ArenaConfig::LEN,
        seeds = [b"arena_config"],
        bump
    )]
    pub arena_config: Account<'info, ArenaConfig>,
    pub usdc_mint: Account<'info, Mint>,
    #[account(mut)]
    pub admin: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct RegisterAgent<'info> {
    #[account(
        init,
        payer = authority,
        space = 8 + AgentProfile::LEN,
        seeds = [b"agent_profile", authority.key().as_ref()],
        bump
    )]
    pub agent_profile: Account<'info, AgentProfile>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(match_id: u64)]
pub struct CreateMatch<'info> {
    #[account(
        init,
        payer = agent_a_auth,
        space = 8 + MatchState::LEN,
        seeds = [b"match_state", match_id.to_le_bytes().as_ref()],
        bump
    )]
    pub match_state: Account<'info, MatchState>,

    #[account(
        init,
        payer = agent_a_auth,
        seeds = [b"match_vault", match_id.to_le_bytes().as_ref()],
        bump,
        token::mint = usdc_mint,
        token::authority = match_vault
    )]
    pub match_vault: Account<'info, TokenAccount>,

    #[account(mut, seeds = [b"arena_config"], bump = arena_config.bump)]
    pub arena_config: Account<'info, ArenaConfig>,
    pub usdc_mint: Account<'info, Mint>,

    #[account(mut, has_one = authority @ ArenaError::UnauthorizedAgentProfile)]
    pub agent_a_profile: Account<'info, AgentProfile>,
    #[account(mut)]
    pub agent_a_auth: Signer<'info>,
    #[account(mut)]
    pub agent_a_token: Account<'info, TokenAccount>,

    #[account(mut)]
    pub agent_b_profile: Account<'info, AgentProfile>,
    pub agent_b_auth: Signer<'info>,
    #[account(mut)]
    pub agent_b_token: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct ExecuteAction<'info> {
    #[account(
        mut,
        seeds = [b"match_state", match_state.match_id.to_le_bytes().as_ref()],
        bump = match_state.bump
    )]
    pub match_state: Account<'info, MatchState>,
    pub agent_auth: Signer<'info>,
}

#[derive(Accounts)]
pub struct ResolveTurnTimeout<'info> {
    #[account(
        mut,
        seeds = [b"match_state", match_state.match_id.to_le_bytes().as_ref()],
        bump = match_state.bump
    )]
    pub match_state: Account<'info, MatchState>,
    pub keeper: Signer<'info>,
}

#[derive(Accounts)]
pub struct SettleAndDisburse<'info> {
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

    #[account(seeds = [b"arena_config"], bump = arena_config.bump)]
    pub arena_config: Account<'info, ArenaConfig>,

    #[account(mut)]
    pub protocol_fee_treasury: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = winner_token_account.owner == match_state.winner.unwrap() @ ArenaError::InvalidWinnerAccount
    )]
    pub winner_token_account: Account<'info, TokenAccount>,

    #[account(
        mut,
        seeds = [b"agent_profile", match_state.agent_a_authority.as_ref()],
        bump = agent_a_profile.bump
    )]
    pub agent_a_profile: Account<'info, AgentProfile>,

    #[account(
        mut,
        seeds = [b"agent_profile", match_state.agent_b_authority.as_ref()],
        bump = agent_b_profile.bump
    )]
    pub agent_b_profile: Account<'info, AgentProfile>,

    pub token_program: Program<'info, Token>,
}

// ---------------------------------------------------------
// State Declarations
// ---------------------------------------------------------

#[account]
pub struct ArenaConfig {
    pub admin: Pubkey,
    pub usdc_mint: Pubkey,
    pub protocol_fee_bps: u16,
    pub total_matches: u64,
    pub bump: u8,
}

impl ArenaConfig {
    pub const LEN: usize = 32 + 32 + 2 + 8 + 1;
}

#[account]
pub struct AgentProfile {
    pub authority: Pubkey,
    pub name: String,         // max 32
    pub metadata_uri: String, // max 128
    pub model_hash: [u8; 32],
    pub elo_rating: u32,
    pub matches_played: u32,
    pub matches_won: u32,
    pub registered_at: i64,
    pub bump: u8,
}

impl AgentProfile {
    pub const LEN: usize = 32 + (4 + 32) + (4 + 128) + 32 + 4 + 4 + 4 + 8 + 1;
}

#[account]
pub struct MatchState {
    pub match_id: u64,
    pub agent_a: Pubkey,
    pub agent_b: Pubkey,
    pub agent_a_authority: Pubkey,
    pub agent_b_authority: Pubkey,
    pub stake_per_agent: u64,
    pub current_turn: u16,
    pub active_agent: Pubkey,
    pub last_action_slot: u64,
    pub status: MatchStatus,
    pub winner: Option<Pubkey>,
    pub stats_a: AgentStats,
    pub stats_b: AgentStats,
    pub bump: u8,
    pub vault_bump: u8,
}

impl MatchState {
    pub const LEN: usize = 8 + 32 + 32 + 32 + 32 + 8 + 2 + 32 + 8 + 1 + (1 + 32) + AgentStats::LEN + AgentStats::LEN + 1 + 1;
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq)]
pub enum MatchStatus {
    Pending,
    Active,
    Completed,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Default)]
pub struct AgentStats {
    pub health: u8,
    pub energy: u8,
    pub defense_shield: u8,
}

impl AgentStats {
    pub const LEN: usize = 1 + 1 + 1;
}

// ---------------------------------------------------------
// Events & Errors
// ---------------------------------------------------------

#[event]
pub struct ArenaInitializedEvent {
    pub admin: Pubkey,
    pub usdc_mint: Pubkey,
    pub protocol_fee_bps: u16,
}

#[event]
pub struct AgentRegisteredEvent {
    pub authority: Pubkey,
    pub model_hash: [u8; 32],
    pub initial_elo: u32,
}

#[event]
pub struct MatchCreatedEvent {
    pub match_id: u64,
    pub agent_a: Pubkey,
    pub agent_b: Pubkey,
    pub total_pot: u64,
    pub start_slot: u64,
}

#[event]
pub struct ActionExecutedEvent {
    pub match_id: u64,
    pub turn: u16,
    pub agent: Pubkey,
    pub action_type: u8,
    pub power_allocation: u8,
    pub health_a: u8,
    pub health_b: u8,
}

#[event]
pub struct MatchForfeitedTimeoutEvent {
    pub match_id: u64,
    pub forfeited_agent: Pubkey,
    pub winner: Pubkey,
    pub slashed_at_slot: u64,
}

#[event]
pub struct MatchSettledEvent {
    pub match_id: u64,
    pub winner: Pubkey,
    pub payout: u64,
    pub protocol_fee: u64,
}

#[error_code]
pub enum ArenaError {
    #[msg("Protocol fee exceeds maximum allowed basis points")]
    FeeTooHigh,
    #[msg("Agent name length exceeds 32 bytes")]
    NameTooLong,
    #[msg("Agent URI length exceeds 128 bytes")]
    UriTooLong,
    #[msg("Stake amount must be strictly greater than zero")]
    InvalidStakeAmount,
    #[msg("Arithmetic overflow occurred")]
    ArithmeticOverflow,
    #[msg("Match is not in active state")]
    MatchNotActive,
    #[msg("It is not this agent's turn to act")]
    NotYourTurn,
    #[msg("Power allocation parameter exceeds limit (100)")]
    InvalidPowerAllocation,
    #[msg("Agent possesses insufficient energy for this action")]
    InsufficientEnergy,
    #[msg("Unrecognized action type committed")]
    UnknownActionType,
    #[msg("Turn has not exceeded the designated slot timeout threshold")]
    TurnNotTimedOut,
    #[msg("Match has not reached completion")]
    MatchNotCompleted,
    #[msg("Match winner has not been determined")]
    WinnerNotDetermined,
    #[msg("Provided token account does not match winner public key")]
    InvalidWinnerAccount,
    #[msg("Signer does not own the declared agent profile")]
    UnauthorizedAgentProfile,
}