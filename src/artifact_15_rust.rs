use anchor_lang::prelude::*;

pub mod errors;
pub mod instructions;
pub mod state;

use instructions::*;
use state::*;

declare_id!("StvArenA11111111111111111111111111111111111");

#[program]
pub mod steve_agent_arena {
    use super::*;

    pub fn initialize_arena(
        ctx: Context<InitializeArena>,
        arena_id: u64,
        entry_fee: u64,
        turn_timeout_slots: u64,
        protocol_fee_bps: u16,
    ) -> Result<()> {
        instructions::initialize_arena::handler(ctx, arena_id, entry_fee, turn_timeout_slots, protocol_fee_bps)
    }

    pub fn register_agent(
        ctx: Context<RegisterAgent>,
        agent_id: u64,
        name: String,
        metadata_hash: [u8; 32],
    ) -> Result<()> {
        instructions::register_agent::handler(ctx, agent_id, name, metadata_hash)
    }

    pub fn join_match(ctx: Context<JoinMatch>, match_id: u64) -> Result<()> {
        instructions::join_match::handler(ctx, match_id)
    }

    pub fn commit_action(ctx: Context<CommitAction>, action_hash: [u8; 32]) -> Result<()> {
        instructions::commit_action::handler(ctx, action_hash)
    }

    pub fn resolve_turn(
        ctx: Context<ResolveTurn>,
        act_one: DecodedAction,
        act_two: DecodedAction,
    ) -> Result<()> {
        instructions::resolve_turn::handler(ctx, act_one, act_two)
    }

    pub fn timeout_forfeit(ctx: Context<TimeoutForfeit>) -> Result<()> {
        instructions::timeout_forfeit::handler(ctx)
    }

    pub fn settle_match(ctx: Context<SettleMatch>) -> Result<()> {
        instructions::settle_match::handler(ctx)
    }
}