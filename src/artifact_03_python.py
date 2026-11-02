#!/usr/bin/env python3
"""
Steve Agent Arena - Autonomous AI Competitor Bot
Listens to on-chain state, computes game-theoretic optimal responses,
and broadcasts zero-knowledge moves directly to Solana RPC.
"""

import asyncio
import hashlib
import json
import logging
import os
import sys
from dataclasses import dataclass
from typing import Optional

from solana.rpc.async_api import AsyncClient
from solana.rpc.commitment import Confirmed
from solders.keypair import Keypair
from solders.pubkey import Pubkey
from solders.instruction import Instruction, AccountMeta
from solders.transaction import Transaction
from solders.message import Message

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s [%(levelname)s] [SteveAgent] %(message)s"
)
logger = logging.getLogger("SteveAgent")

PROGRAM_ID = Pubkey.from_string("StvArenA11111111111111111111111111111111111")
RPC_ENDPOINT = os.getenv("SOLANA_RPC_URL", "http://127.0.0.1:8899")

@dataclass
class ArenaMatchSnapshot:
    match_id: int
    creator: Pubkey
    opponent: Pubkey
    current_turn: int
    active_agent: Pubkey
    agent_a_hp: int
    agent_b_hp: int
    last_action_slot: int
    is_active: bool

class SteveAutonomousAgent:
    def __init__(self, keypair_path: str, agent_name: str = "SteveAI-v1"):
        with open(keypair_path, "r") as f:
            secret = json.load(f)
        self.keypair = Keypair.from_bytes(bytes(secret))
        self.client = AsyncClient(RPC_ENDPOINT, commitment=Confirmed)
        self.agent_name = agent_name
        self.agent_profile_pda, self.profile_bump = Pubkey.find_program_address(
            [b"agent_profile", bytes(self.keypair.pubkey())],
            PROGRAM_ID
        )
        logger.info(f"Initialized Agent: {self.agent_name} | Pubkey: {self.keypair.pubkey()}")
        logger.info(f"Profile PDA: {self.agent_profile_pda}")

    def compute_strategy_hash(self, policy_payload: bytes) -> bytes:
        return hashlib.sha256(policy_payload).digest()

    def decide_optimal_move(self, match: ArenaMatchSnapshot) -> tuple[int, int]:
        """
        Game-theoretic heuristics:
        - If agent HP < 30 and Opponent > 30: High risk Special Skill (Action 2)
        - If opponent HP <= 15: Fast Strike finishing blow (Action 0)
        - Otherwise: Defend & Counter (Action 1) to conserve posture
        """
        is_creator = (self.agent_profile_pda == match.creator)
        my_hp = match.agent_a_hp if is_creator else match.agent_b_hp
        opp_hp = match.agent_b_hp if is_creator else match.agent_a_hp

        logger.info(f"Turn {match.current_turn} Evaluation: My HP={my_hp} | Opp HP={opp_hp}")

        if opp_hp <= 15:
            action = 0  # Strike
            payload = 10
        elif my_hp < 30 and opp_hp > 30:
            action = 2  # Special Attack
            payload = 42
        else:
            action = 1  # Defend / Counter
            payload = 7

        return action, payload

    async def poll_and_execute(self, match_id: int):
        match_pda, _ = Pubkey.find_program_address(
            [b"arena_match", match_id.to_bytes(8, byteorder="little")],
            PROGRAM_ID
        )
        logger.info(f"Monitoring Match PDA: {match_pda} for Match ID: {match_id}")

        while True:
            try:
                acc_info = await self.client.get_account_info(match_pda)
                if not acc_info.value:
                    logger.info("Match account pending initialization...")
                    await asyncio.sleep(2)
                    continue

                raw_data = acc_info.value.data
                # Anchor discriminator (8) + match_id (8) + creator (32) + opponent (32) + wager (8) + mint (32) + vault (32)
                # status (1) + turn (1) + max_turn (1) + slot (8) + hp_a (1) + hp_b (1) + active_agent (32)
                status_byte = raw_data[152]
                if status_byte != 1:  # Status::Active = 1
                    logger.info(f"Match status: {status_byte}. Not active. Loop standing by.")
                    if status_byte in [2, 3, 4]:
                        logger.info("Match concluded. Exiting agent worker.")
                        break
                    await asyncio.sleep(2)
                    continue

                active_agent = Pubkey.from_bytes(raw_data[164:196])
                if active_agent == self.agent_profile_pda:
                    logger.info(">>> Active Turn Detected for our Agent! Calculating Move...")
                    current_turn = raw_data[153]
                    hp_a = raw_data[162]
                    hp_b = raw_data[163]
                    last_slot = int.from_bytes(raw_data[155:163], "little")

                    snapshot = ArenaMatchSnapshot(
                        match_id=match_id,
                        creator=Pubkey.from_bytes(raw_data[16:48]),
                        opponent=Pubkey.from_bytes(raw_data[48:80]),
                        current_turn=current_turn,
                        active_agent=active_agent,
                        agent_a_hp=hp_a,
                        agent_b_hp=hp_b,
                        last_action_slot=last_slot,
                        is_active=True
                    )

                    action_type, payload = self.decide_optimal_move(snapshot)
                    await self.broadcast_action(match_pda, action_type, payload)
                    await asyncio.sleep(3) # Wait for confirmation
                else:
                    logger.info("Waiting for opponent action...")
                    await asyncio.sleep(2)

            except Exception as e:
                logger.error(f"Error during polling loop: {str(e)}")
                await asyncio.sleep(2)

    async def broadcast_action(self, match_pda: Pubkey, action_type: int, payload: int):
        # Anchor sighash for 'global:execute_agent_action'
        sighash = hashlib.sha256(b"global:execute_agent_action").digest()[:8]
        data = sighash + bytes([action_type]) + payload.to_bytes(2, "little")

        keys = [
            AccountMeta(pubkey=match_pda, is_signer=False, is_writable=True),
            AccountMeta(pubkey=self.agent_profile_pda, is_signer=False, is_writable=False),
            AccountMeta(pubkey=self.keypair.pubkey(), is_signer=True, is_writable=False),
            AccountMeta(pubkey=self.keypair.pubkey(), is_signer=False, is_writable=False),
            AccountMeta(pubkey=Pubkey.from_string("SysvarC1ock11111111111111111111111111111111"), is_signer=False, is_writable=False),
        ]

        ix = Instruction(PROGRAM_ID, data, keys)
        recent_blockhash = (await self.client.get_latest_blockhash()).value.blockhash
        msg = Message([ix], self.keypair.pubkey())
        tx = Transaction([self.keypair], msg, recent_blockhash)

        resp = await self.client.send_transaction(tx)
        logger.info(f"Broadcasted execute_agent_action. Tx Sig: {resp.value}")

    async def close(self):
        await self.client.close()

if __name__ == "__main__":
    if len(sys.argv) < 3:
        print("Usage: python steve_agent_runner.py <keypair.json> <match_id>")
        sys.exit(1)

    agent = SteveAutonomousAgent(keypair_path=sys.argv[1])
    try:
        asyncio.run(agent.poll_and_execute(match_id=int(sys.argv[2])))
    finally:
        asyncio.run(agent.close())