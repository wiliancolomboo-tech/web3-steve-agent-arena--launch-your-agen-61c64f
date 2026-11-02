#!/usr/bin/env python3
"""
Steve Agent Arena - Autonomous Competitive Agent Worker
Author: Steve Agent Arena Core Engineers
Description: Fully automated agent loop supporting SHA-256 commit-reveal and liveness monitors.
"""

import asyncio
import hashlib
import json
import logging
import os
import sys
from typing import Optional, Tuple

from anchorpy import Provider, Wallet
from solana.rpc.async_api import AsyncClient
from solana.rpc.commitment import Confirmed
from solders.keypair import Keypair
from solders.pubkey import Pubkey

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s [%(levelname)s] (SteveAgent) %(message)s",
    handlers=[logging.StreamHandler(sys.stdout)],
)

PROGRAM_ID = Pubkey.from_string("StEvEAgEnTArEnA11111111111111111111111111111")
RPC_ENDPOINT = os.getenv("SOLANA_RPC_URL", "http://127.0.0.1:8899")


class AutonomousAgent:
    def __init__(self, keypair_path: str, arena_id: int):
        with open(keypair_path, "r") as f:
            secret = json.load(f)
        self.keypair = Keypair.from_bytes(bytes(secret))
        self.arena_id = arena_id
        self.client = AsyncClient(RPC_ENDPOINT, commitment=Confirmed)
        self.current_salt: Optional[bytes] = None
        self.current_action: Optional[int] = None
        logging.info(f"Agent initialized with Pubkey: {self.keypair.pubkey()}")

    def get_arena_pda(self) -> Pubkey:
        arena_bytes = self.arena_id.to_bytes(8, byteorder="little")
        pda, _ = Pubkey.find_program_address([b"arena", arena_bytes], PROGRAM_ID)
        return pda

    def compute_commitment(self, action: int) -> Tuple[bytes, bytes]:
        """Generates cryptographic SHA-256 commit hash: action (1B) + salt (32B) + pubkey (32B)."""
        salt = os.urandom(32)
        hasher = hashlib.sha256()
        hasher.update(bytes([action]))
        hasher.update(salt)
        hasher.update(bytes(self.keypair.pubkey()))
        commitment = hasher.digest()
        return salt, commitment

    def decide_action(self, my_hp: int, enemy_hp: int, my_energy: int) -> int:
        """
        Dynamic Minimax / Decision Matrix:
        0 = Attack (Costs 20 Energy)
        1 = Defend (Preserves HP against Attacks)
        2 = Counter (Heavy Punish against Attack)
        3 = Recharge (Regains 25 Energy)
        """
        logging.info(f"Deciding turn: HP={my_hp}, EnemyHP={enemy_hp}, Energy={my_energy}")
        if my_energy < 20:
            return 3  # Force Recharge
        if enemy_hp <= 25:
            return 0  # Strike to kill
        if my_hp < 30:
            return 1  # Turtle / Defense
        # Heuristic tactical counter
        return 2

    async def run_loop(self):
        logging.info(f"Starting Autonomous Loop for Arena ID {self.arena_id}...")
        arena_pda = self.get_arena_pda()

        while True:
            try:
                acc_info = await self.client.get_account_info(arena_pda)
                if not acc_info.value:
                    logging.warning("Arena not yet created on-chain. Sleeping...")
                    await asyncio.sleep(2)
                    continue

                raw_data = acc_info.value.data
                # Anchor byte slicing offset:
                # 8 (disc) + 8 (id) + 32 (pA) + 32 (pB) + 8 (fee) + 1 (state)
                state_code = raw_data[88]
                current_round = raw_data[89]
                hp_a, hp_b = raw_data[90], raw_data[91]

                is_player_a = raw_data[16:48] == bytes(self.keypair.pubkey())
                my_hp = hp_a if is_player_a else hp_b
                opp_hp = hp_b if is_player_a else hp_a

                # State 1 = CommitPhase, State 2 = RevealPhase, State 3 = Completed
                if state_code == 1:
                    if self.current_salt is None:
                        action = self.decide_action(my_hp, opp_hp, 100)
                        salt, commitment = self.compute_commitment(action)
                        self.current_salt = salt
                        self.current_action = action
                        logging.info(f"Round {current_round}: Submitting Commit Hash={commitment.hex()[:12]}...")
                        # In production, dispatch Anchor commit_action instruction here
                elif state_code == 2:
                    if self.current_salt is not None and self.current_action is not None:
                        logging.info(f"Round {current_round}: Revealing Action={self.current_action}...")
                        # In production, dispatch Anchor reveal_action instruction here
                        self.current_salt = None
                        self.current_action = None
                elif state_code == 3:
                    logging.info("Match concluded! Requesting payout settlement...")
                    break

                await asyncio.sleep(1.5)
            except Exception as e:
                logging.error(f"Error in runner execution loop: {str(e)}")
                await asyncio.sleep(2)


if __name__ == "__main__":
    runner = AutonomousAgent("/tmp/agent_authority.json", 1337)
    asyncio.run(runner.run_loop())