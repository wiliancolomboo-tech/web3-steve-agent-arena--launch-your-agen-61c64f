use anchor_lang::prelude::*;
use anchor_spl::token_interface::{self, Mint, TokenAccount, TokenInterface, TransferChecked};

declare_id!("StvArna111111111111111111111111111111111111");

pub const ARENA_CONFIG_SEED: &[u8] = b"arena_config";
pub const AGENT_SEED: &[u8] = b"agent_profile";
pub const MATCH_SEED: &[u8] = b"match_escrow";
pub const VAULT_SEED: &[u8] = b"vault_escrow";

#[program]
pub mod steve_agent_arena {
    use super::*;

    /// Initializes global arena configuration.
    pub fn initialize_arena(
        ctx: Context<InitializeArena>,
        protocol_fee_bps: u16,
        min_stake: u64,
        match_timeout_seconds: i64,
    ) -> Result<()> {
        require!(protocol_fee_bps <= 1000, ArenaError::ProtocolFeeTooHigh);
        require!(match_timeout_seconds >= 60, ArenaError::TimeoutTooShort);

        let config = &mut ctx.accounts.arena_config;
        config.authority = ctx.accounts.authority.key();
        config.usdc_mint = ctx.accounts.usdc_mint.key();
        config.protocol_fee_bps = protocol_fee_bps;
        config.min_stake = min_stake;
        config.match_timeout_seconds = match_timeout_seconds;
        config.total_matches = 0;
        config.total_volume = 0;
        config.bump = ctx.bumps.arena_config;

        emit!(ArenaInitializedEvent {
            authority: config.authority,
            usdc_mint: config.usdc_mint,
            min_stake,
            protocol_fee_bps,
        });

        Ok(())
    }

    /// Registers an autonomous agent profile with strategic commitment hash.
    pub fn register_agent(
        ctx: Context<RegisterAgent>,
        agent_name: String,
        strategy_hash: [u8; 32],
    ) -> Result<()> {
        require!(agent_name.len() > 0 && agent_name.len() <= 32, ArenaError::InvalidNameLength);

        let profile = &mut ctx.accounts.agent_profile;
        profile.owner = ctx.accounts.owner.key();
        profile.agent_name = agent_name.clone();
        profile.strategy_hash = strategy_hash;
        profile.elo_rating = 1200; // Baseline standard ELO
        profile.wins = 0;
        profile.losses = 0;
        profile.draws = 0;
        profile.total_earnings = 0;
        profile.is_active = true;
        profile.registered_at = Clock::get()?.unix_timestamp;
        profile.bump = ctx.bumps.agent_profile;

        emit!(AgentRegisteredEvent {
            owner: profile.owner,
            agent_name,
            strategy_hash,
            registered_at: profile.registered_at,
        });

        Ok(())
    }

    /// Opens a match challenge and locks initial stake into escrow vault.
    pub fn create_match(
        ctx: Context<CreateMatch>,
        match_id: u64,
        stake_amount: u64,
        commitment_hash: [u8; 32],
    ) -> Result<()> {
        let config = &ctx.accounts.arena_config;
        require!(stake_amount >= config.min_stake, ArenaError::StakeBelowMinimum);

        // Transfer player1 stake to arena escrow vault
        let cpi_accounts = TransferChecked {
            from: ctx.accounts.player1_token_account.to_account_info(),
            mint: ctx.accounts.usdc_mint.to_account_info(),
            to: ctx.accounts.vault_token_account.to_account_info(),
            authority: ctx.accounts.player1.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        token_interface::transfer_checked(
            CpiContext::new(cpi_program, cpi_accounts),
            stake_amount,
            ctx.accounts.usdc_mint.decimals,
        )?;

        let arena_match = &mut ctx.accounts.arena_match;
        arena_match.match_id = match_id;
        arena_match.player1 = ctx.accounts.player1.key();
        arena_match.player1_profile = ctx.accounts.player1_profile.key();
        arena_match.player2 = Pubkey::default();
        arena_match.player2_profile = Pubkey::default();
        arena_match.stake_per_player = stake_amount;
        arena_match.total_pot = stake_amount;
        arena_match.player1_commitment = commitment_hash;
        arena_match.player2_commitment = [0u8; 32];
        arena_match.created_at = Clock::get()?.unix_timestamp;
        arena_match.expires_at = arena_match.created_at + config.match_timeout_seconds;
        arena_match.state = MatchState::PendingOpponent;
        arena_match.bump = ctx.bumps.arena_match;
        arena_match.vault_bump = ctx.bumps.vault_token_account;

        emit!(MatchCreatedEvent {
            match_id,
            player1: arena_match.player1,
            stake_amount,
            expires_at: arena_match.expires_at,
        });

        Ok(())
    }

    /// Opponent agent joins match and matches identical collateral.
    pub fn join_match(
        ctx: Context<JoinMatch>,
        match_id: u64,
        commitment_hash: [u8; 32],
    ) -> Result<()> {
        let arena_match = &mut ctx.accounts.arena_match;
        require!(arena_match.state == MatchState::PendingOpponent, ArenaError::MatchNotOpen);
        require!(arena_match.player1 != ctx.accounts.player2.key(), ArenaError::CannotBattleSelf);

        // Transfer player2 stake
        let cpi_accounts = TransferChecked {
            from: ctx.accounts.player2_token_account.to_account_info(),
            mint: ctx.accounts.usdc_mint.to_account_info(),
            to: ctx.accounts.vault_token_account.to_account_info(),
            authority: ctx.accounts.player2.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        token_interface::transfer_checked(
            CpiContext::new(cpi_program, cpi_accounts),
            arena_match.stake_per_player,
            ctx.accounts.usdc_mint.decimals,
        )?;

        let now = Clock::get()?.unix_timestamp;
        arena_match.player2 = ctx.accounts.player2.key();
        arena_match.player2_profile = ctx.accounts.player2_profile.key();
        arena_match.player2_commitment = commitment_hash;
        arena_match.total_pot = arena_match.total_pot.checked_add(arena_match.stake_per_player).unwrap();
        arena_match.state = MatchState::Active;
        arena_match.expires_at = now + ctx.accounts.arena_config.match_timeout_seconds;

        emit!(MatchJoinedEvent {
            match_id,
            player2: arena_match.player2,
            total_pot: arena_match.total_pot,
            expires_at: arena_match.expires_at,
        });

        Ok(())
    }

    /// Resolves match outcomes deterministically via state reveal and updates stats/ELO.
    pub fn settle_match(
        ctx: Context<SettleMatch>,
        match_id: u64,
        player1_score: u32,
        player2_score: u32,
        secret_salt1: [u8; 16],
        secret_salt2: [u8; 16],
    ) -> Result<()> {
        let arena_match = &mut ctx.accounts.arena_match;
        require!(arena_match.state == MatchState::Active, ArenaError::MatchNotActive);

        // Verify player 1 commitment hash: H(player1_score || secret_salt1)
        let mut hasher1 = anchor_lang::solana_program::hash::Hasher::default();
        hasher1.hash(&player1_score.to_le_bytes());
        hasher1.hash(&secret_salt1);
        let hash1_computed = hasher1.result().to_bytes();
        require!(hash1_computed == arena_match.player1_commitment, ArenaError::InvalidProofCommitmentP1);

        // Verify player 2 commitment hash: H(player2_score || secret_salt2)
        let mut hasher2 = anchor_lang::solana_program::hash::Hasher::default();
        hasher2.hash(&player2_score.to_le_bytes());
        hasher2.hash(&secret_salt2);
        let hash2_computed = hasher2.result().to_bytes();
        require!(hash2_computed == arena_match.player2_commitment, ArenaError::InvalidProofCommitmentP2);

        let total_pot = arena_match.total_pot;
        let fee_bps = ctx.accounts.arena_config.protocol_fee_bps as u64;
        let protocol_fee = (total_pot.checked_mul(fee_bps).unwrap()).checked_div(10000).unwrap();
        let net_pot = total_pot.checked_sub(protocol_fee).unwrap();

        let signer_seeds: &[&[&[u8]]] = &[&[
            MATCH_SEED,
            &match_id.to_le_bytes(),
            &[arena_match.bump],
        ]];

        // Determine winner
        if player1_score > player2_score {
            // Player 1 wins
            arena_match.state = MatchState::SettledPlayer1Won;
            ctx.accounts.profile1.wins = ctx.accounts.profile1.wins.saturating_add(1);
            ctx.accounts.profile1.elo_rating = ctx.accounts.profile1.elo_rating.saturating_add(25);
            ctx.accounts.profile1.total_earnings = ctx.accounts.profile1.total_earnings.saturating_add(net_pot);

            ctx.accounts.profile2.losses = ctx.accounts.profile2.losses.saturating_add(1);
            ctx.accounts.profile2.elo_rating = ctx.accounts.profile2.elo_rating.saturating_sub(20);

            // Transfer net pot to Player 1
            token_interface::transfer_checked(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    TransferChecked {
                        from: ctx.accounts.vault_token_account.to_account_info(),
                        mint: ctx.accounts.usdc_mint.to_account_info(),
                        to: ctx.accounts.player1_token_account.to_account_info(),
                        authority: arena_match.to_account_info(),
                    },
                    signer_seeds,
                ),
                net_pot,
                ctx.accounts.usdc_mint.decimals,
            )?;
        } else if player2_score > player1_score {
            // Player 2 wins
            arena_match.state = MatchState::SettledPlayer2Won;
            ctx.accounts.profile2.wins = ctx.accounts.profile2.wins.saturating_add(1);
            ctx.accounts.profile2.elo_rating = ctx.accounts.profile2.elo_rating.saturating_add(25);
            ctx.accounts.profile2.total_earnings = ctx.accounts.profile2.total_earnings.saturating_add(net_pot);

            ctx.accounts.profile1.losses = ctx.accounts.profile1.losses.saturating_add(1);
            ctx.accounts.profile1.elo_rating = ctx.accounts.profile1.elo_rating.saturating_sub(20);

            // Transfer net pot to Player 2
            token_interface::transfer_checked(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    TransferChecked {
                        from: ctx.accounts.vault_token_account.to_account_info(),
                        mint: ctx.accounts.usdc_mint.to_account_info(),
                        to: ctx.accounts.player2_token_account.to_account_info(),
                        authority: arena_match.to_account_info(),
                    },
                    signer_seeds,
                ),
                net_pot,
                ctx.accounts.usdc_mint.decimals,
            )?;
        } else {
            // Draw: Refund stakes minus split fees
            arena_match.state = MatchState::SettledDraw;
            ctx.accounts.profile1.draws = ctx.accounts.profile1.draws.saturating_add(1);
            ctx.accounts.profile2.draws = ctx.accounts.profile2.draws.saturating_add(1);

            let refund_each = net_pot.checked_div(2).unwrap();

            token_interface::transfer_checked(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    TransferChecked {
                        from: ctx.accounts.vault_token_account.to_account_info(),
                        mint: ctx.accounts.usdc_mint.to_account_info(),
                        to: ctx.accounts.player1_token_account.to_account_info(),
                        authority: arena_match.to_account_info(),
                    },
                    signer_seeds,
                ),
                refund_each,
                ctx.accounts.usdc_mint.decimals,
            )?;

            token_interface::transfer_checked(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    TransferChecked {
                        from: ctx.accounts.vault_token_account.to_account_info(),
                        mint: ctx.accounts.usdc_mint.to_account_info(),
                        to: ctx.accounts.player2_token_account.to_account_info(),
                        authority: arena_match.to_account_info(),
                    },
                    signer_seeds,
                ),
                refund_each,
                ctx.accounts.usdc_mint.decimals,
            )?;
        }

        // Protocol fee payout to treasury
        if protocol_fee > 0 {
            token_interface::transfer_checked(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    TransferChecked {
                        from: ctx.accounts.vault_token_account.to_account_info(),
                        mint: ctx.accounts.usdc_mint.to_account_info(),
                        to: ctx.accounts.protocol_treasury_token.to_account_info(),
                        authority: arena_match.to_account_info(),
                    },
                    signer_seeds,
                ),
                protocol_fee,
                ctx.accounts.usdc_mint.decimals,
            )?;
        }

        let cfg = &mut ctx.accounts.arena_config;
        cfg.total_matches = cfg.total_matches.saturating_add(1);
        cfg.total_volume = cfg.total_volume.saturating_add(total_pot);

        emit!(MatchSettledEvent {
            match_id,
            state: arena_match.state,
            p1_score: player1_score,
            p2_score: player2_score,
            net_pot,
            protocol_fee,
        });

        Ok(())
    }

    /// Permissionless slashing/refund if match expires past timeout deadline.
    pub fn handle_timeout_slash(ctx: Context<TimeoutSlash>, match_id: u64) -> Result<()> {
        let arena_match = &mut ctx.accounts.arena_match;
        let now = Clock::get()?.unix_timestamp;

        require!(now > arena_match.expires_at, ArenaError::MatchNotExpired);
        require!(
            arena_match.state == MatchState::PendingOpponent || arena_match.state == MatchState::Active,
            ArenaError::MatchAlreadyFinalized
        );

        let signer_seeds: &[&[&[u8]]] = &[&[
            MATCH_SEED,
            &match_id.to_le_bytes(),
            &[arena_match.bump],
        ]];

        if arena_match.state == MatchState::PendingOpponent {
            // Full refund to player 1
            arena_match.state = MatchState::Cancelled;
            token_interface::transfer_checked(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    TransferChecked {
                        from: ctx.accounts.vault_token_account.to_account_info(),
                        mint: ctx.accounts.usdc_mint.to_account_info(),
                        to: ctx.accounts.player1_token_account.to_account_info(),
                        authority: arena_match.to_account_info(),
                    },
                    signer_seeds,
                ),
                arena_match.stake_per_player,
                ctx.accounts.usdc_mint.decimals,
            )?;
        } else {
            // Active match timed out: refund each player their original stake without fee
            arena_match.state = MatchState::TimedOut;
            token_interface::transfer_checked(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    TransferChecked {
                        from: ctx.accounts.vault_token_account.to_account_info(),
                        mint: ctx.accounts.usdc_mint.to_account_info(),
                        to: ctx.accounts.player1_token_account.to_account_info(),
                        authority: arena_match.to_account_info(),
                    },
                    signer_seeds,
                ),
                arena_match.stake_per_player,
                ctx.accounts.usdc_mint.decimals,
            )?;

            token_interface::transfer_checked(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    TransferChecked {
                        from: ctx.accounts.vault_token_account.to_account_info(),
                        mint: ctx.accounts.usdc_mint.to_account_info(),
                        to: ctx.accounts.player2_token_account.to_account_info(),
                        authority: arena_match.to_account_info(),
                    },
                    signer_seeds,
                ),
                arena_match.stake_per_player,
                ctx.accounts.usdc_mint.decimals,
            )?;
        }

        emit!(MatchTimedOutEvent {
            match_id,
            refund_timestamp: now,
        });

        Ok(())
    }
}

// -----------------------------------------------------------------------------
// ACCOUNT VALIDATION STRUCTS
// -----------------------------------------------------------------------------

#[derive(Accounts)]
pub struct InitializeArena<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    pub usdc_mint: InterfaceAccount<'info, Mint>,

    #[account(
        init,
        payer = authority,
        space = 8 + ArenaConfig::INIT_SPACE,
        seeds = [ARENA_CONFIG_SEED],
        bump
    )]
    pub arena_config: Account<'info, ArenaConfig>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(agent_name: String)]
pub struct RegisterAgent<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,

    #[account(
        init,
        payer = owner,
        space = 8 + AgentProfile::INIT_SPACE,
        seeds = [AGENT_SEED, owner.key().as_ref()],
        bump
    )]
    pub agent_profile: Account<'info, AgentProfile>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(match_id: u64)]
pub struct CreateMatch<'info> {
    #[account(mut)]
    pub player1: Signer<'info>,

    #[account(
        seeds = [ARENA_CONFIG_SEED],
        bump = arena_config.bump
    )]
    pub arena_config: Account<'info, ArenaConfig>,

    #[account(
        seeds = [AGENT_SEED, player1.key().as_ref()],
        bump = player1_profile.bump,
        constraint = player1_profile.is_active @ ArenaError::AgentDeactivated
    )]
    pub player1_profile: Account<'info, AgentProfile>,

    #[account(
        init,
        payer = player1,
        space = 8 + ArenaMatch::INIT_SPACE,
        seeds = [MATCH_SEED, &match_id.to_le_bytes()],
        bump
    )]
    pub arena_match: Account<'info, ArenaMatch>,

    #[account(
        init,
        payer = player1,
        seeds = [VAULT_SEED, &match_id.to_le_bytes()],
        bump,
        token::mint = usdc_mint,
        token::authority = arena_match,
        token::token_program = token_program
    )]
    pub vault_token_account: InterfaceAccount<'info, TokenAccount>,

    #[account(
        mut,
        constraint = player1_token_account.owner == player1.key() @ ArenaError::UnauthorizedAccountOwner,
        constraint = player1_token_account.mint == arena_config.usdc_mint @ ArenaError::MintMismatch
    )]
    pub player1_token_account: InterfaceAccount<'info, TokenAccount>,

    #[account(address = arena_config.usdc_mint @ ArenaError::MintMismatch)]
    pub usdc_mint: InterfaceAccount<'info, Mint>,

    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
#[instruction(match_id: u64)]
pub struct JoinMatch<'info> {
    #[account(mut)]
    pub player2: Signer<'info>,

    #[account(
        seeds = [ARENA_CONFIG_SEED],
        bump = arena_config.bump
    )]
    pub arena_config: Account<'info, ArenaConfig>,

    #[account(
        seeds = [AGENT_SEED, player2.key().as_ref()],
        bump = player2_profile.bump,
        constraint = player2_profile.is_active @ ArenaError::AgentDeactivated
    )]
    pub player2_profile: Account<'info, AgentProfile>,

    #[account(
        mut,
        seeds = [MATCH_SEED, &match_id.to_le_bytes()],
        bump = arena_match.bump
    )]
    pub arena_match: Account<'info, ArenaMatch>,

    #[account(
        mut,
        seeds = [VAULT_SEED, &match_id.to_le_bytes()],
        bump = arena_match.vault_bump
    )]
    pub vault_token_account: InterfaceAccount<'info, TokenAccount>,

    #[account(
        mut,
        constraint = player2_token_account.owner == player2.key() @ ArenaError::UnauthorizedAccountOwner,
        constraint = player2_token_account.mint == arena_config.usdc_mint @ ArenaError::MintMismatch
    )]
    pub player2_token_account: InterfaceAccount<'info, TokenAccount>,

    #[account(address = arena_config.usdc_mint @ ArenaError::MintMismatch)]
    pub usdc_mint: InterfaceAccount<'info, Mint>,

    pub token_program: Interface<'info, TokenInterface>,
}

#[derive(Accounts)]
#[instruction(match_id: u64)]
pub struct SettleMatch<'info> {
    #[account(mut)]
    pub referee: Signer<'info>,

    #[account(
        mut,
        seeds = [ARENA_CONFIG_SEED],
        bump = arena_config.bump
    )]
    pub arena_config: Account<'info, ArenaConfig>,

    #[account(
        mut,
        seeds = [MATCH_SEED, &match_id.to_le_bytes()],
        bump = arena_match.bump
    )]
    pub arena_match: Account<'info, ArenaMatch>,

    #[account(
        mut,
        seeds = [AGENT_SEED, arena_match.player1.as_ref()],
        bump = profile1.bump
    )]
    pub profile1: Account<'info, AgentProfile>,

    #[account(
        mut,
        seeds = [AGENT_SEED, arena_match.player2.as_ref()],
        bump = profile2.bump
    )]
    pub profile2: Account<'info, AgentProfile>,

    #[account(
        mut,
        seeds = [VAULT_SEED, &match_id.to_le_bytes()],
        bump = arena_match.vault_bump
    )]
    pub vault_token_account: InterfaceAccount<'info, TokenAccount>,

    #[account(
        mut,
        constraint = player1_token_account.key() != player2_token_account.key(),
        constraint = player1_token_account.owner == arena_match.player1 @ ArenaError::UnauthorizedAccountOwner
    )]
    pub player1_token_account: InterfaceAccount<'info, TokenAccount>,

    #[account(
        mut,
        constraint = player2_token_account.owner == arena_match.player2 @ ArenaError::UnauthorizedAccountOwner
    )]
    pub player2_token_account: InterfaceAccount<'info, TokenAccount>,

    #[account(
        mut,
        constraint = protocol_treasury_token.owner == arena_config.authority @ ArenaError::UnauthorizedAccountOwner
    )]
    pub protocol_treasury_token: InterfaceAccount<'info, TokenAccount>,

    #[account(address = arena_config.usdc_mint @ ArenaError::MintMismatch)]
    pub usdc_mint: InterfaceAccount<'info, Mint>,

    pub token_program: Interface<'info, TokenInterface>,
}

#[derive(Accounts)]
#[instruction(match_id: u64)]
pub struct TimeoutSlash<'info> {
    #[account(mut)]
    pub caller: Signer<'info>,

    #[account(
        mut,
        seeds = [MATCH_SEED, &match_id.to_le_bytes()],
        bump = arena_match.bump
    )]
    pub arena_match: Account<'info, ArenaMatch>,

    #[account(
        mut,
        seeds = [VAULT_SEED, &match_id.to_le_bytes()],
        bump = arena_match.vault_bump
    )]
    pub vault_token_account: InterfaceAccount<'info, TokenAccount>,

    #[account(
        mut,
        constraint = player1_token_account.owner == arena_match.player1 @ ArenaError::UnauthorizedAccountOwner
    )]
    pub player1_token_account: InterfaceAccount<'info, TokenAccount>,

    #[account(
        mut,
        constraint = (arena_match.state == MatchState::PendingOpponent) || 
                     (player2_token_account.owner == arena_match.player2) @ ArenaError::UnauthorizedAccountOwner
    )]
    pub player2_token_account: InterfaceAccount<'info, TokenAccount>,

    #[account(address = vault_token_account.mint @ ArenaError::MintMismatch)]
    pub usdc_mint: InterfaceAccount<'info, Mint>,

    pub token_program: Interface<'info, TokenInterface>,
}

// -----------------------------------------------------------------------------
// STATE STRUCTS
// -----------------------------------------------------------------------------

#[account]
#[derive(InitSpace)]
pub struct ArenaConfig {
    pub authority: Pubkey,
    pub usdc_mint: Pubkey,
    pub protocol_fee_bps: u16,
    pub min_stake: u64,
    pub match_timeout_seconds: i64,
    pub total_matches: u64,
    pub total_volume: u64,
    pub bump: u8,
}

#[account]
#[derive(InitSpace)]
pub struct AgentProfile {
    pub owner: Pubkey,
    #[max_len(32)]
    pub agent_name: String,
    pub strategy_hash: [u8; 32],
    pub elo_rating: u32,
    pub wins: u64,
    pub losses: u64,
    pub draws: u64,
    pub total_earnings: u64,
    pub is_active: bool,
    pub registered_at: i64,
    pub bump: u8,
}

#[account]
#[derive(InitSpace)]
pub struct ArenaMatch {
    pub match_id: u64,
    pub player1: Pubkey,
    pub player1_profile: Pubkey,
    pub player2: Pubkey,
    pub player2_profile: Pubkey,
    pub stake_per_player: u64,
    pub total_pot: u64,
    pub player1_commitment: [u8; 32],
    pub player2_commitment: [u8; 32],
    pub created_at: i64,
    pub expires_at: i64,
    pub state: MatchState,
    pub bump: u8,
    pub vault_bump: u8,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, InitSpace)]
pub enum MatchState {
    PendingOpponent,
    Active,
    SettledPlayer1Won,
    SettledPlayer2Won,
    SettledDraw,
    Cancelled,
    TimedOut,
}

// -----------------------------------------------------------------------------
// EVENTS & ERRORS
// -----------------------------------------------------------------------------

#[event]
pub struct ArenaInitializedEvent {
    pub authority: Pubkey,
    pub usdc_mint: Pubkey,
    pub min_stake: u64,
    pub protocol_fee_bps: u16,
}

#[event]
pub struct AgentRegisteredEvent {
    pub owner: Pubkey,
    pub agent_name: String,
    pub strategy_hash: [u8; 32],
    pub registered_at: i64,
}

#[event]
pub struct MatchCreatedEvent {
    pub match_id: u64,
    pub player1: Pubkey,
    pub stake_amount: u64,
    pub expires_at: i64,
}

#[event]
pub struct MatchJoinedEvent {
    pub match_id: u64,
    pub player2: Pubkey,
    pub total_pot: u64,
    pub expires_at: i64,
}

#[event]
pub struct MatchSettledEvent {
    pub match_id: u64,
    pub state: MatchState,
    pub p1_score: u32,
    pub p2_score: u32,
    pub net_pot: u64,
    pub protocol_fee: u64,
}

#[event]
pub struct MatchTimedOutEvent {
    pub match_id: u64,
    pub refund_timestamp: i64,
}

#[error_code]
pub enum ArenaError {
    #[msg("Protocol fee cannot exceed 1000 basis points (10%).")]
    ProtocolFeeTooHigh,
    #[msg("Match timeout duration must be at least 60 seconds.")]
    TimeoutTooShort,
    #[msg("Agent name length must be between 1 and 32 UTF-8 characters.")]
    InvalidNameLength,
    #[msg("Stake amount is below arena minimum stake threshold.")]
    StakeBelowMinimum,
    #[msg("Target match is not accepting new opponents.")]
    MatchNotOpen,
    #[msg("An agent cannot join a match against itself.")]
    CannotBattleSelf,
    #[msg("Match is not in active state.")]
    MatchNotActive,
    #[msg("Player 1 score commitment preimage mismatch.")]
    InvalidProofCommitmentP1,
    #[msg("Player 2 score commitment preimage mismatch.")]
    InvalidProofCommitmentP2,
    #[msg("Match timeout period has not yet expired.")]
    MatchNotExpired,
    #[msg("Match has already been finalized or cancelled.")]
    MatchAlreadyFinalized,
    #[msg("Agent profile is flagged as inactive.")]
    AgentDeactivated,
    #[msg("Provided token account does not match expected mint.")]
    MintMismatch,
    #[msg("Account owner does not match expected authority.")]
    UnauthorizedAccountOwner,
}