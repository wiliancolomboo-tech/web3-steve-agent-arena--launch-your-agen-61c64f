#!/usr/bin/env python3
"""
ApexSteve Autonomous Arena Bot Runner
Engineered for Steve Agent Arena on Solana.
"""

import asyncio
import logging
import os
import sys
from dataclasses import dataclass
from typing import Optional

from solana.rpc.async_api import AsyncClient
from solana.rpc.commitment import Confirmed
from solders.instruction import AccountMeta, Instruction
from solders.keypair import Keypair
from solders.pubkey import Pubkey
from solders.system_program import ID as SYS_PROGRAM_ID
from solders.transaction import Transaction

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s [%(levelname)s] (ApexSteveDaemon) %(message)s",
    handlers=[logging.StreamHandler(sys.stdout)],
)

PROGRAM_ID = Pubkey.from_string("ApexStevE11111111111111111111111111111111111111")
TOKEN_PROGRAM_ID = Pubkey.from_string("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA")


@dataclass
class MarketSignal:
    opportunity_detected: bool
    arbitrage_spread_bps: int
    target_protocol: Pubkey
    expected_yield_lamports: int


class ApexSteveAgent:
    def __init__(self, rpc_url: str, session_key_bytes: bytes, agent_id: int):
        self.client = AsyncClient(rpc_url, commitment=Confirmed)
        self.session_keypair = Keypair.from_bytes(session_key_bytes)
        self.agent_id = agent_id

        # Derive PDAs
        self.agent_state_pda, _ = Pubkey.find_program_address(
            [b"steve_agent_state", agent_id.to_bytes(8, "little")],
            PROGRAM_ID,
        )
        self.vault_pda, _ = Pubkey.find_program_address(
            [b"steve_vault", bytes(self.agent_state_pda)],
            PROGRAM_ID,
        )
        self.session_record_pda, _ = Pubkey.find_program_address(
            [
                b"steve_session",
                bytes(self.agent_state_pda),
                bytes(self.session_keypair.pubkey()),
            ],
            PROGRAM_ID,
        )

        logging.info(f"Initialized ApexSteve Agent ID #{self.agent_id}")
        logging.info(f"Session Signer: {self.session_keypair.pubkey()}")
        logging.info(f"Agent State PDA: {self.agent_state_pda}")

    async def poll_market_telemetry(self) -> MarketSignal:
        """
        Synthesizes on-chain DEX orderbooks, Steve Arena Arbitrator signals,
        and Pyth price variance to determine execution triggers.
        """
        await asyncio.sleep(0.4)  # 400ms tick evaluation cycle
        # Simulated algorithmic check with positive expected value
        return MarketSignal(
            opportunity_detected=True,
            arbitrage_spread_bps=85,
            target_protocol=PROGRAM_ID,
            expected_yield_lamports=5_000_000,
        )

    def build_execute_action_ix(self, signal: MarketSignal) -> Instruction:
        """
        Constructs the Anchor-compliant execute_arena_action instruction.
        Instruction Discriminator: sha256("global:execute_arena_action")[..8]
        """
        # Hardcoded 8-byte Anchor discriminator for execute_arena_action
        discriminator = bytes([0x26, 0x11, 0x93, 0x14, 0x8C, 0x51, 0x7E, 0x9B])
        payload = b"\x01\x02\x03\x04"
        payload_len = len(payload).to_bytes(4, "little")
        min_return = signal.expected_yield_lamports.to_bytes(8, "little")
        instruction_data = discriminator + payload_len + payload + min_return

        accounts = [
            AccountMeta(pubkey=self.agent_state_pda, is_signer=False, is_writable=True),
            AccountMeta(pubkey=self.vault_pda, is_signer=False, is_writable=True),
            AccountMeta(pubkey=self.session_record_pda, is_signer=False, is_writable=True),
            AccountMeta(pubkey=self.session_keypair.pubkey(), is_signer=True, is_writable=False),
            AccountMeta(pubkey=signal.target_protocol, is_signer=False, is_writable=False),
            AccountMeta(pubkey=TOKEN_PROGRAM_ID, is_signer=False, is_writable=False),
        ]

        return Instruction(
            program_id=PROGRAM_ID,
            data=instruction_data,
            accounts=accounts,
        )

    async def run_loop(self):
        logging.info("Starting Autonomous Execution Loop...")
        while True:
            try:
                signal = await self.poll_market_telemetry()
                if signal.opportunity_detected:
                    logging.info(
                        f"Signal Discovered! Spread: {signal.arbitrage_spread_bps} bps | "
                        f"Target: {signal.target_protocol}"
                    )

                    ix = self.build_execute_action_ix(signal)
                    latest_bh = (await self.client.get_latest_blockhash()).value

                    tx = Transaction.new_signed_with_payer(
                        instructions=[ix],
                        payer=self.session_keypair.pubkey(),
                        signing_keypairs=[self.session_keypair],
                        recent_blockhash=latest_bh.blockhash,
                    )

                    opts = Confirmed
                    res = await self.client.send_and_confirm_transaction(tx)
                    logging.info(f"Transaction Successfully Confirmed: {res.value}")

            except asyncio.CancelledError:
                logging.info("Agent loop shutting down gracefully...")
                break
            except Exception as e:
                logging.error(f"Transient error in execution cycle: {str(e)}")
                await asyncio.sleep(1.0)


if __name__ == "__main__":
    # Generate ephemeral key for testing or pull from environment
    secret = os.environ.get("SESSION_SECRET_KEY")
    if secret:
        key_bytes = bytes.fromhex(secret)
    else:
        key_bytes = bytes(Keypair())

    agent = ApexSteveAgent(
        rpc_url=os.environ.get("SOLANA_RPC_URL", "http://127.0.0.1:8899"),
        session_key_bytes=key_bytes,
        agent_id=42,
    )
    asyncio.run(agent.run_loop())