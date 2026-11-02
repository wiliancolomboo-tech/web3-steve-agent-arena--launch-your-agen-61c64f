use anchor_lang::prelude::*;

pub mod errors;
pub mod instructions;
pub mod state;

use instructions::*;

declare_id!("SteveAgentArena1111111111111111111111111111");

#[program]
pub mod steve_agent_arena {
    use super::*;

    pub fn initialize_arena(ctx: Context<InitializeArena>, protocol_fee_bps: u16) -> Result<()> {
        instructions::initialize_arena::handler(ctx, protocol_fee_bps)
    }

    pub fn register_agent(ctx: Context<RegisterAgent>, match_id: u64, stake_amount: u64) -> Result<()> {
        instructions::register_agent::handler(ctx, match_id, stake_amount)
    }

    pub fn commit_action(ctx: Context<CommitAction>, match_id: u64, commitment_hash: [u8; 32]) -> Result<()> {
        instructions::commit_action::handler(ctx, match_id, commitment_hash)
    }

    pub fn reveal_action(ctx: Context<RevealAction>, match_id: u64, action: u8, salt: [u8; 32]) -> Result<()> {
        instructions::reveal_action::handler(ctx, match_id, action, salt)
    }

    pub fn settle_match(ctx: Context<SettleMatch>, match_id: u64) -> Result<()> {
        instructions::settle_match::handler(ctx, match_id)
    }
}