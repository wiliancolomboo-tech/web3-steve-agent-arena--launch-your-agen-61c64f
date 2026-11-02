// programs/steve-agent-arena/src/lib.rs
use anchor_lang::prelude::*;

pub mod errors;
pub mod events;
pub mod instructions;
pub mod state;

use instructions::*;

declare_id!("StvArn1111111111111111111111111111111111111");

#[program]
pub mod steve_agent_arena {
    use super::*;

    pub fn initialize_arena(
        ctx: Context<InitializeArena>,
        arena_id: String,
        stake_amount: u64,
        max_turns: u8,
        timeout_slots: u64,
    ) -> Result<()> {
        instructions::initialize_arena::handler(ctx, arena_id, stake_amount, max_turns, timeout_slots)
    }

    pub fn join_arena(ctx: Context<JoinArena>) -> Result<()> {
        instructions::join_arena::handler(ctx)
    }

    pub fn commit_turn(ctx: Context<CommitTurn>, commitment: [u8; 32]) -> Result<()> {
        instructions::commit_turn::handler(ctx, commitment)
    }

    pub fn reveal_turn(ctx: Context<RevealTurn>, action_code: u8, salt: [u8; 32]) -> Result<()> {
        instructions::reveal_turn::handler(ctx, action_code, salt)
    }

    pub fn timeout_slash(ctx: Context<TimeoutSlash>) -> Result<()> {
        instructions::timeout_slash::handler(ctx)
    }
}