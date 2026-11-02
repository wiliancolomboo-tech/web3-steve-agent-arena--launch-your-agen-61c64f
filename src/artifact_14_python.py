"""
Autonomous Steve Agent - Solana Tactical Engine
Monitors on-chain arena account, evaluates optimal minimax action via heuristic / LLM,
and commits signed transactions autonomously to Solana localnet/devnet.
"""

import asyncio
import hashlib
import json
import logging
import os
import sys
from typing import Dict, Any, Optional

from solders.keypair import Keypair
from solders.pubkey import Pubkey
from solders.instruction import Instruction, AccountMeta
from solders.transaction import Transaction
from solders.message import Message
from solana.rpc.async_api import AsyncClient
from solana.rpc.commitment import Confirmed

logging.basicConfig(level=logging.INFO, format="%(asctime)s [%(levelname)s] %(message)s")
logger = logging.getLogger("SteveAgent")

PROGRAM_ID = Pubkey.from_string("StvArenA11111111111111111111111111111111111")

class SteveAutonomousAgent:
    def __init__(self, keypair_path: str, arena_id: int, rpc_url: str = "http://127.0.0.1:8899"):
        self.arena_id = arena_id
        self.rpc_client = AsyncClient(rpc_url, commitment=Confirmed)
        self.keypair = self._load_or_generate_keypair(keypair_path)
        self.arena_pda, _ = Pubkey.find_program_address(
            [b"arena", arena_id.to_bytes(8, byteorder="little")],
            PROGRAM_ID
        )
        self.agent_pda, _ = Pubkey.find_program_address(
            [b"agent", bytes(self.arena_pda), bytes(self.keypair.pubkey())],
            PROGRAM_ID
        )
        logger.info(f"Agent initialised: {self.keypair.pubkey()}")
        logger.info(f"Target Arena PDA: {self.arena_pda}")
        logger.info(f"Agent State PDA:  {self.agent_pda}")

    def _load_or_generate_keypair(self, path: str) -> Keypair:
        if os.path.exists(path):
            with open(path, "r") as f:
                data = json.load(f)
            return Keypair.from_bytes(bytes(data))
        kp = Keypair()
        with open(path, "w") as f:
            json.dump(list(kp.to_bytes()), f)
        return kp

    async def get_agent_state(self) -> Optional[Dict[str, Any]]:
        info = await self.rpc_client.get_account_info(self.agent_pda)
        if not info.value:
            return None
        data = info.value.data
        # Slice custom state offsets from AgentState:
        # Discriminator (8) + Arena (32) + Owner (32) = 72
        health = int.from_bytes(data[72:74], "little")
        energy = int.from_bytes(data[74:76], "little")
        score = int.from_bytes(data[76:84], "little")
        has_committed = bool(data[118])
        return {
            "health": health,
            "energy": energy,
            "score": score,
            "has_committed": has_committed
        }

    def compute_optimal_move(self, state: Dict[str, Any]) -> int:
        """
        Tactical Decision Logic:
        0 = LightAttack (15 Energy, 20 Dmg)
        1 = HeavyAttack (35 Energy, 45 Dmg)
        2 = Defend      (10 Energy, 75% dmg mitigation)
        3 = Recharge    (0 Energy, +30 Energy)
        """
        energy = state["energy"]
        health = state["health"]

        logger.info(f"Evaluating move: Agent HP={health}, Energy={energy}")
        if energy < 15:
            logger.info("Decision: Recharge (Energy low)")
            return 3
        if health < 30 and energy >= 10:
            logger.info("Decision: Defend (Health critical)")
            return 2
        if energy >= 35:
            logger.info("Decision: HeavyAttack (Max damage)")
            return 1
        return 0

    async def commit_move(self, action_enum: int):
        salt = os.urandom(16)
        action_hash = hashlib.sha256(action_enum.to_bytes(1, "little") + salt).digest()

        # Anchor discriminator for "commit_action": sha256("global:commit_action")[0..8]
        # Discriminator pre-computed: [0xb8, 0x11, 0x51, 0x59, 0x05, 0x93, 0x8b, 0x47]
        discriminator = bytes([0xb8, 0x11, 0x51, 0x59, 0x05, 0x93, 0x8b, 0x47])
        data = discriminator + action_enum.to_bytes(1, "little") + action_hash

        accounts = [
            AccountMeta(pubkey=self.arena_pda, is_signer=False, is_writable=False),
            AccountMeta(pubkey=self.agent_pda, is_signer=False, is_writable=True),
            AccountMeta(pubkey=self.keypair.pubkey(), is_signer=True, is_writable=False),
        ]

        ix = Instruction(PROGRAM_ID, data, accounts)
        recent_blockhash = (await self.rpc_client.get_latest_blockhash()).value.blockhash
        msg = Message([ix], self.keypair.pubkey())
        tx = Transaction([self.keypair], msg, recent_blockhash)

        resp = await self.rpc_client.send_transaction(tx)
        logger.info(f"Action committed. Tx Signature: {resp.value}")

    async def run(self):
        logger.info("Starting Autonomous Agent Polling Loop...")
        while True:
            try:
                state = await self.get_agent_state()
                if state:
                    if not state["has_committed"]:
                        move = self.compute_optimal_move(state)
                        await self.commit_move(move)
                    else:
                        logger.info("Agent action committed. Waiting for round resolution...")
                else:
                    logger.warning("Agent account not initialized yet. Waiting for match launch...")
            except Exception as e:
                logger.error(f"Error in agent execution cycle: {str(e)}")
            await asyncio.sleep(1.0)

if __name__ == "__main__":
    arena_id_arg = int(sys.argv[1]) if len(sys.argv) > 1 else 1337
    agent = SteveAutonomousAgent(keypair_path="./agent_wallet.json", arena_id=arena_id_arg)
    asyncio.run(agent.run())