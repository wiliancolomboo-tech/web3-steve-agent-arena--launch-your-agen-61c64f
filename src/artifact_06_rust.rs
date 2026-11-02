use anchor_lang::prelude::*;
use anchor_lang::solana_program::hash::hashv;

declare_id!("SteveArena111111111111111111111111111111111");

pub const ARENA_CONFIG_SEED: &[u8] = b"arena_config";
pub const DUEL_MATCH_SEED: &[u8] = b"duel_match";
pub const ESCROW_VAULT_SEED: &[u8] = b"escrow_vault";
pub const AGENT_PROFILE_SEED: &[u8] = b"agent_profile";

pub const REVEAL_TIMEOUT_SLOTS: u64 = 150; // ~60 seconds on Solana
pub const PROTOCOL_FEE_BPS: u64 = 100;    // 1.00% fee

#[program]
pub mod steve_agent_arena {
    use super::*;

    pub fn initialize_arena(
        ctx: Context<InitializeArena>,
        fee_recipient: Pubkey,
    ) -> Result<()> {
        let config = &mut ctx.accounts.arena_config;
        config.admin = ctx.accounts.admin.key();
        config.fee_recipient = fee_recipient;
        config.total_matches = 0;
        config.total_volume = 0;
        config.bump = ctx.bumps.arena_config;
        Ok(())
    }

    pub fn register_agent(
        ctx: Context<RegisterAgent>,
        name: [u8; 32],
    ) -> Result<()> {
        let profile = &mut ctx.accounts.agent_profile;
        profile.owner = ctx.accounts.signer.key();
        profile.name = name;
        profile.elo = 1200;
        profile.wins = 0;
        profile.losses = 0;
        profile.total_earnings = 0;
        profile.bump = ctx.bumps.agent_profile;
        Ok(())
    }

    pub fn create_duel(
        ctx: Context<CreateDuel>,
        match_id: u64,
        stake_amount: u64,
        commitment_hash: [u8; 32],
    ) -> Result<()> {
        require!(stake_amount > 0, ArenaError::InvalidStakeAmount);

        let duel = &mut ctx.accounts.duel_match;
        duel.match_id = match_id;
        duel.player_one = ctx.accounts.player_one.key();
        duel.player_two = Pubkey::default();
        duel.stake_per_player = stake_amount;
        duel.p1_commitment = commitment_hash;
        duel.p2_commitment = [0u8; 32];
        duel.p1_move = 0;
        duel.p2_move = 0;
        duel.p1_revealed = false;
        duel.p2_revealed = false;
        duel.state = MatchState::Initialized;
        duel.created_slot = Clock::get()?.slot;
        duel.deadline_slot = 0;
        duel.winner = Pubkey::default();
        duel.bump = ctx.bumps.duel_match;
        duel.escrow_bump = ctx.bumps.escrow_vault;

        // Escrow transfer from Player One to Match Vault
        let transfer_ix = anchor_lang::solana_program::system_instruction::transfer(
            &ctx.accounts.player_one.key(),
            &ctx.accounts.escrow_vault.key(),
            stake_amount,
        );
        anchor_lang::solana_program::program::invoke(
            &transfer_ix,
            &[
                ctx.accounts.player_one.to_account_info(),
                ctx.accounts.escrow_vault.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
        )?;

        let config = &mut ctx.accounts.arena_config;
        config.total_matches = config.total_matches.checked_add(1).ok_or(ArenaError::ArithmeticOverflow)?;

        emit!(DuelCreatedEvent {
            match_id,
            player_one: ctx.accounts.player_one.key(),
            stake_amount,
            commitment_hash,
        });

        Ok(())
    }

    pub fn join_duel(
        ctx: Context<JoinDuel>,
        commitment_hash: [u8; 32],
    ) -> Result<()> {
        let duel = &mut ctx.accounts.duel_match;
        require!(duel.state == MatchState::Initialized, ArenaError::DuelNotJoinable);
        require!(duel.player_one != ctx.accounts.player_two.key(), ArenaError::CannotPlaySelf);

        duel.player_two = ctx.accounts.player_two.key();
        duel.p2_commitment = commitment_hash;
        duel.state = MatchState::AwaitingReveals;
        duel.deadline_slot = Clock::get()?.slot.checked_add(REVEAL_TIMEOUT_SLOTS).ok_or(ArenaError::ArithmeticOverflow)?;

        // Escrow transfer from Player Two to Match Vault
        let transfer_ix = anchor_lang::solana_program::system_instruction::transfer(
            &ctx.accounts.player_two.key(),
            &ctx.accounts.escrow_vault.key(),
            duel.stake_per_player,
        );
        anchor_lang::solana_program::program::invoke(
            &transfer_ix,
            &[
                ctx.accounts.player_two.to_account_info(),
                ctx.accounts.escrow_vault.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
        )?;

        emit!(DuelJoinedEvent {
            match_id: duel.match_id,
            player_two: ctx.accounts.player_two.key(),
            deadline_slot: duel.deadline_slot,
        });

        Ok(())
    }

    pub fn reveal_move(
        ctx: Context<RevealMove>,
        revealed_move: u8,
        salt: [u8; 32],
    ) -> Result<()> {
        let duel = &mut ctx.accounts.duel_match;
        require!(duel.state == MatchState::AwaitingReveals, ArenaError::NotInRevealState);
        require!(revealed_move >= 1 && revealed_move <= 3, ArenaError::InvalidMoveRange);

        let caller = ctx.accounts.signer.key();

        // Cryptographic preimage check: SHA256(revealed_move || salt)
        let move_byte = [revealed_move];
        let computed_hash = hashv(&[&move_byte, &salt]).to_bytes();

        if caller == duel.player_one {
            require!(!duel.p1_revealed, ArenaError::MoveAlreadyRevealed);
            require!(computed_hash == duel.p1_commitment, ArenaError::InvalidCommitmentPreimage);
            duel.p1_move = revealed_move;
            duel.p1_revealed = true;
        } else if caller == duel.player_two {
            require!(!duel.p2_revealed, ArenaError::MoveAlreadyRevealed);
            require!(computed_hash == duel.p2_commitment, ArenaError::InvalidCommitmentPreimage);
            duel.p2_move = revealed_move;
            duel.p2_revealed = true;
        } else {
            return Err(ArenaError::UnauthorizedPlayer.into());
        }

        emit!(MoveRevealedEvent {
            match_id: duel.match_id,
            player: caller,
            revealed_move,
        });

        // If both revealed, resolve match deterministically
        if duel.p1_revealed && duel.p2_revealed {
            resolve_duel_outcome(
                duel,
                &ctx.accounts.escrow_vault,
                &ctx.accounts.player_one_target,
                &ctx.accounts.player_two_target,
                &ctx.accounts.fee_recipient,
                &mut ctx.accounts.arena_config,
            )?;
        }

        Ok(())
    }

    pub fn claim_timeout_slashing(ctx: Context<ClaimTimeoutSlashing>) -> Result<()> {
        let duel = &mut ctx.accounts.duel_match;
        let current_slot = Clock::get()?.slot;

        require!(duel.state == MatchState::AwaitingReveals, ArenaError::NotInRevealState);
        require!(current_slot > duel.deadline_slot, ArenaError::TimeoutNotElapsed);

        let match_pot = duel.stake_per_player.checked_mul(2).ok_or(ArenaError::ArithmeticOverflow)?;
        let protocol_fee = match_pot.checked_mul(PROTOCOL_FEE_BPS).unwrap() / 10000;
        let winner_payout = match_pot.checked_sub(protocol_fee).unwrap();

        let escrow_bump = duel.escrow_bump;
        let match_id_bytes = duel.match_id.to_le_bytes();
        let signer_seeds: &[&[&[u8]]] = &[&[
            ESCROW_VAULT_SEED,
            match_id_bytes.as_ref(),
            &[escrow_bump],
        ]];

        if duel.p1_revealed && !duel.p2_revealed {
            // Player 1 wins via Player 2 timeout
            duel.winner = duel.player_one;
            duel.state = MatchState::Settled;
            payout_transfer(&ctx.accounts.escrow_vault, &ctx.accounts.player_one_target, winner_payout, signer_seeds)?;
            payout_transfer(&ctx.accounts.escrow_vault, &ctx.accounts.fee_recipient, protocol_fee, signer_seeds)?;
        } else if !duel.p1_revealed && duel.p2_revealed {
            // Player 2 wins via Player 1 timeout
            duel.winner = duel.player_two;
            duel.state = MatchState::Settled;
            payout_transfer(&ctx.accounts.escrow_vault, &ctx.accounts.player_two_target, winner_payout, signer_seeds)?;
            payout_transfer(&ctx.accounts.escrow_vault, &ctx.accounts.fee_recipient, protocol_fee, signer_seeds)?;
        } else if !duel.p1_revealed && !duel.p2_revealed {
            // Both timed out: Split pot minus protocol fee
            duel.state = MatchState::Settled;
            let split_payout = (duel.stake_per_player).checked_sub(protocol_fee / 2).unwrap();
            payout_transfer(&ctx.accounts.escrow_vault, &ctx.accounts.player_one_target, split_payout, signer_seeds)?;
            payout_transfer(&ctx.accounts.escrow_vault, &ctx.accounts.player_two_target, split_payout, signer_seeds)?;
            payout_transfer(&ctx.accounts.escrow_vault, &ctx.accounts.fee_recipient, protocol_fee, signer_seeds)?;
        } else {
            return Err(ArenaError::BothMovesRevealedUseRegularResolve.into());
        }

        emit!(DuelSettledEvent {
            match_id: duel.match_id,
            winner: duel.winner,
            payout: winner_payout,
            draw: duel.winner == Pubkey::default(),
        });

        Ok(())
    }
}

// ---------------- Helper Calculation & Transfer ----------------

fn resolve_duel_outcome<'info>(
    duel: &mut Account<'info, DuelMatch>,
    escrow_vault: &AccountInfo<'info>,
    p1_target: &AccountInfo<'info>,
    p2_target: &AccountInfo<'info>,
    fee_recipient: &AccountInfo<'info>,
    config: &mut Account<'info, ArenaConfig>,
) -> Result<()> {
    duel.state = MatchState::Settled;

    let m1 = duel.p1_move;
    let m2 = duel.p2_move;
    // Moves: 1 = Rock/Attack, 2 = Paper/Counter, 3 = Scissors/Blitz
    // 1 beats 3, 2 beats 1, 3 beats 2. Equal moves = Draw.

    let match_pot = duel.stake_per_player.checked_mul(2).ok_or(ArenaError::ArithmeticOverflow)?;
    let protocol_fee = match_pot.checked_mul(PROTOCOL_FEE_BPS).unwrap() / 10000;
    let winner_prize = match_pot.checked_sub(protocol_fee).unwrap();

    let escrow_bump = duel.escrow_bump;
    let match_id_bytes = duel.match_id.to_le_bytes();
    let signer_seeds: &[&[&[u8]]] = &[&[
        ESCROW_VAULT_SEED,
        match_id_bytes.as_ref(),
        &[escrow_bump],
    ]];

    config.total_volume = config.total_volume.checked_add(match_pot).unwrap();

    if m1 == m2 {
        // Draw: Return original stakes minus proportional fee
        let refund = duel.stake_per_player.checked_sub(protocol_fee / 2).unwrap();
        duel.winner = Pubkey::default();
        payout_transfer(escrow_vault, p1_target, refund, signer_seeds)?;
        payout_transfer(escrow_vault, p2_target, refund, signer_seeds)?;
        payout_transfer(escrow_vault, fee_recipient, protocol_fee, signer_seeds)?;

        emit!(DuelSettledEvent {
            match_id: duel.match_id,
            winner: Pubkey::default(),
            payout: 0,
            draw: true,
        });
    } else if (m1 == 1 && m2 == 3) || (m1 == 2 && m2 == 1) || (m1 == 3 && m2 == 2) {
        // Player 1 Wins
        duel.winner = duel.player_one;
        payout_transfer(escrow_vault, p1_target, winner_prize, signer_seeds)?;
        payout_transfer(escrow_vault, fee_recipient, protocol_fee, signer_seeds)?;

        emit!(DuelSettledEvent {
            match_id: duel.match_id,
            winner: duel.player_one,
            payout: winner_prize,
            draw: false,
        });
    } else {
        // Player 2 Wins
        duel.winner = duel.player_two;
        payout_transfer(escrow_vault, p2_target, winner_prize, signer_seeds)?;
        payout_transfer(escrow_vault, fee_recipient, protocol_fee, signer_seeds)?;

        emit!(DuelSettledEvent {
            match_id: duel.match_id,
            winner: duel.player_two,
            payout: winner_prize,
            draw: false,
        });
    }

    Ok(())
}

fn payout_transfer<'info>(
    from_vault: &AccountInfo<'info>,
    to_account: &AccountInfo<'info>,
    amount: u64,
    signer_seeds: &[&[&[u8]]],
) -> Result<()> {
    if amount == 0 {
        return Ok(());
    }
    let transfer_ix = anchor_lang::solana_program::system_instruction::transfer(
        from_vault.key,
        to_account.key,
        amount,
    );
    anchor_lang::solana_program::program::invoke_signed(
        &transfer_ix,
        &[from_vault.clone(), to_account.clone()],
        signer_seeds,
    )?;
    Ok(())
}

// ---------------- Contexts & State Structures ----------------

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
    #[account(mut)]
    pub admin: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct RegisterAgent<'info> {
    #[account(
        init,
        payer = signer,
        space = 8 + AgentProfile::LEN,
        seeds = [AGENT_PROFILE_SEED, signer.key().as_ref()],
        bump
    )]
    pub agent_profile: Account<'info, AgentProfile>,
    #[account(mut)]
    pub signer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(match_id: u64)]
pub struct CreateDuel<'info> {
    #[account(
        init,
        payer = player_one,
        space = 8 + DuelMatch::LEN,
        seeds = [DUEL_MATCH_SEED, match_id.to_le_bytes().as_ref()],
        bump
    )]
    pub duel_match: Account<'info, DuelMatch>,
    #[account(
        mut,
        seeds = [ESCROW_VAULT_SEED, match_id.to_le_bytes().as_ref()],
        bump
    )]
    pub escrow_vault: SystemAccount<'info>,
    #[account(mut, seeds = [ARENA_CONFIG_SEED], bump = arena_config.bump)]
    pub arena_config: Account<'info, ArenaConfig>,
    #[account(mut)]
    pub player_one: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct JoinDuel<'info> {
    #[account(
        mut,
        seeds = [DUEL_MATCH_SEED, duel_match.match_id.to_le_bytes().as_ref()],
        bump = duel_match.bump
    )]
    pub duel_match: Account<'info, DuelMatch>,
    #[account(
        mut,
        seeds = [ESCROW_VAULT_SEED, duel_match.match_id.to_le_bytes().as_ref()],
        bump = duel_match.escrow_bump
    )]
    pub escrow_vault: SystemAccount<'info>,
    #[account(mut)]
    pub player_two: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct RevealMove<'info> {
    #[account(
        mut,
        seeds = [DUEL_MATCH_SEED, duel_match.match_id.to_le_bytes().as_ref()],
        bump = duel_match.bump
    )]
    pub duel_match: Account<'info, DuelMatch>,
    #[account(
        mut,
        seeds = [ESCROW_VAULT_SEED, duel_match.match_id.to_le_bytes().as_ref()],
        bump = duel_match.escrow_bump
    )]
    pub escrow_vault: SystemAccount<'info>,
    #[account(mut, seeds = [ARENA_CONFIG_SEED], bump = arena_config.bump)]
    pub arena_config: Account<'info, ArenaConfig>,
    #[account(mut)]
    pub signer: Signer<'info>,
    /// CHECK: Validated against duel_match.player_one
    #[account(mut, constraint = player_one_target.key() == duel_match.player_one)]
    pub player_one_target: AccountInfo<'info>,
    /// CHECK: Validated against duel_match.player_two
    #[account(mut, constraint = player_two_target.key() == duel_match.player_two)]
    pub player_two_target: AccountInfo<'info>,
    /// CHECK: Validated against arena_config.fee_recipient
    #[account(mut, constraint = fee_recipient.key() == arena_config.fee_recipient)]
    pub fee_recipient: AccountInfo<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ClaimTimeoutSlashing<'info> {
    #[account(
        mut,
        seeds = [DUEL_MATCH_SEED, duel_match.match_id.to_le_bytes().as_ref()],
        bump = duel_match.bump
    )]
    pub duel_match: Account<'info, DuelMatch>,
    #[account(
        mut,
        seeds = [ESCROW_VAULT_SEED, duel_match.match_id.to_le_bytes().as_ref()],
        bump = duel_match.escrow_bump
    )]
    pub escrow_vault: SystemAccount<'info>,
    #[account(mut, seeds = [ARENA_CONFIG_SEED], bump = arena_config.bump)]
    pub arena_config: Account<'info, ArenaConfig>,
    #[account(mut)]
    pub caller: Signer<'info>,
    /// CHECK: Matches player_one
    #[account(mut, constraint = player_one_target.key() == duel_match.player_one)]
    pub player_one_target: AccountInfo<'info>,
    /// CHECK: Matches player_two
    #[account(mut, constraint = player_two_target.key() == duel_match.player_two)]
    pub player_two_target: AccountInfo<'info>,
    /// CHECK: Matches arena_config.fee_recipient
    #[account(mut, constraint = fee_recipient.key() == arena_config.fee_recipient)]
    pub fee_recipient: AccountInfo<'info>,
    pub system_program: Program<'info, System>,
}

#[account]
pub struct ArenaConfig {
    pub admin: Pubkey,
    pub fee_recipient: Pubkey,
    pub total_matches: u64,
    pub total_volume: u64,
    pub bump: u8,
}

impl ArenaConfig {
    pub const LEN: usize = 32 + 32 + 8 + 8 + 1;
}

#[account]
pub struct AgentProfile {
    pub owner: Pubkey,
    pub name: [u8; 32],
    pub elo: u32,
    pub wins: u32,
    pub losses: u32,
    pub total_earnings: u64,
    pub bump: u8,
}

impl AgentProfile {
    pub const LEN: usize = 32 + 32 + 4 + 4 + 4 + 8 + 1;
}

#[account]
pub struct DuelMatch {
    pub match_id: u64,
    pub player_one: Pubkey,
    pub player_two: Pubkey,
    pub stake_per_player: u64,
    pub p1_commitment: [u8; 32],
    pub p2_commitment: [u8; 32],
    pub p1_move: u8,
    pub p2_move: u8,
    pub p1_revealed: bool,
    pub p2_revealed: bool,
    pub state: MatchState,
    pub created_slot: u64,
    pub deadline_slot: u64,
    pub winner: Pubkey,
    pub bump: u8,
    pub escrow_bump: u8,
}

impl DuelMatch {
    pub const LEN: usize = 8 + 32 + 32 + 8 + 32 + 32 + 1 + 1 + 1 + 1 + 1 + 8 + 8 + 32 + 1 + 1;
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq)]
pub enum MatchState {
    Initialized,
    AwaitingReveals,
    Settled,
    Cancelled,
}

#[error_code]
pub enum ArenaError {
    #[msg("Stake amount must be strictly greater than 0 lamports")]
    InvalidStakeAmount,
    #[msg("Duel is not in an Initialized state")]
    DuelNotJoinable,
    #[msg("An agent cannot play against itself")]
    CannotPlaySelf,
    #[msg("Duel is not in the AwaitingReveals window")]
    NotInRevealState,
    #[msg("Move must be strictly between 1 (Rock), 2 (Paper), or 3 (Scissors)")]
    InvalidMoveRange,
    #[msg("Move for this player has already been verified and revealed")]
    MoveAlreadyRevealed,
    #[msg("Preimage collision failure: hash(move || salt) does not match registered commitment")]
    InvalidCommitmentPreimage,
    #[msg("Caller is not registered as a combatant in this match")]
    UnauthorizedPlayer,
    #[msg("The reveal timeout window has not yet elapsed")]
    TimeoutNotElapsed,
    #[msg("Both moves were revealed; settle via regular reveal flow")]
    BothMovesRevealedUseRegularResolve,
    #[msg("Arithmetic overflow detected during state transition")]
    ArithmeticOverflow,
}

#[event]
pub struct DuelCreatedEvent {
    pub match_id: u64,
    pub player_one: Pubkey,
    pub stake_amount: u64,
    pub commitment_hash: [u8; 32],
}

#[event]
pub struct DuelJoinedEvent {
    pub match_id: u64,
    pub player_two: Pubkey,
    pub deadline_slot: u64,
}

#[event]
pub struct MoveRevealedEvent {
    pub match_id: u64,
    pub player: Pubkey,
    pub revealed_move: u8,
}

#[event]
pub struct DuelSettledEvent {
    pub match_id: u64,
    pub winner: Pubkey,
    pub payout: u64,
    pub draw: bool,
}