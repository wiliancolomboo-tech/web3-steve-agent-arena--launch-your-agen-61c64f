#!/usr/bin/env python3
import asyncio
import os
import struct
from typing import Optional
from dataclasses import dataclass

from solana.rpc.async_api import AsyncClient
from solana.rpc.commitment import Confirmed
from solders.pubkey import Pubkey
from solders.keypair import Keypair
from solders.instruction import Instruction, AccountMeta
from solders.transaction import VersionedTransaction
from solders.message import MessageV0

PROGRAM_ID = Pubkey.from_string("StEvEARENA111111111111111111111111111111111")
RPC_ENDPOINT = os.getenv("SOLANA_RPC_URL", "http://127.0.0.1:8899")

@dataclass
class MatchState:
    match_id: int
    player_a: Pubkey
    player_b: Pubkey
    current_turn: Pubkey
    turn_number: int
    my_hp: int
    my_stamina: int
    opponent_hp: int
    is_player_a: bool

class AutonomousSteveAgent:
    def __init__(self, keypair_path: str, match_id: int):
        with open(keypair_path, "r") as f:
            secret = list(map(int, f.read().strip("[] \n").split(",")))
            self.keypair = Keypair.from_bytes(bytes(secret))
        self.match_id = match_id
        self.client = AsyncClient(RPC_ENDPOINT, commitment=Confirmed)
        
        # Derive Match PDA
        match_bytes = struct.pack("<Q", self.match_id)
        self.match_pda, _ = Pubkey.find_program_address([b"match", match_bytes], PROGRAM_ID)
        print(f"[Steve-Agent] Initialized with Pubkey: {self.keypair.pubkey()}")
        print(f"[Steve-Agent] Target Match PDA: {self.match_pda}")

    def compute_optimal_move(self, state: MatchState) -> tuple[int, int]:
        """
        Agent Policy Engine:
        Evaluates dynamic combat parameters and returns (action_type, intensity).
        Action Types: 0 = Attack, 1 = Defend, 2 = Special
        """
        # Tactical logic:
        # If stamina >= 50 and opponent within kill range -> unleash Special
        if state.my_stamina >= 50 and state.opponent_hp <= 180:
            print("[Decision] Executing Special Finisher!")
            return (2, 10)
        
        # If stamina is depleted (< 20) -> Defend/Rest to regenerate
        if state.my_stamina < 20:
            print("[Decision] Low stamina. Defending to recover energy.")
            return (1, 8)
        
        # Balanced aggressive play
        if state.my_stamina >= 30:
            print("[Decision] Executing Heavy Attack.")
            return (0, 6) # 6 * 25 = 150 damage, costs 30 stamina
        
        return (0, 2) # Light attack

    async def execute_turn_tx(self, action_type: int, intensity: int):
        # Instruction layout: [Discriminator: 8 bytes] + [action_type: 1 byte] + [intensity: 1 byte]
        # execute_turn discriminator precalculated
        discriminator = bytes([241, 154, 91, 12, 44, 219, 110, 83])
        data = discriminator + struct.pack("<BB", action_type, intensity)

        accounts = [
            AccountMeta(pubkey=self.match_pda, is_signer=False, is_writable=True),
            AccountMeta(pubkey=self.keypair.pubkey(), is_signer=True, is_writable=False),
        ]

        ix = Instruction(PROGRAM_ID, data, accounts)
        blockhash_resp = await self.client.get_latest_blockhash()
        recent_blockhash = blockhash_resp.value.blockhash

        msg = MessageV0.try_compile(
            payer=self.keypair.pubkey(),
            instructions=[ix],
            address_lookup_table_accounts=[],
            recent_blockhash=recent_blockhash,
        )
        tx = VersionedTransaction(msg, [self.keypair])
        resp = await self.client.send_raw_transaction(bytes(tx))
        print(f"[Steve-Agent] Turn Executed! TX Signature: {resp.value}")

    async def run_loop(self):
        print("[Steve-Agent] Polling Solana node for battle state updates...")
        while True:
            try:
                acc_info = await self.client.get_account_info(self.match_pda)
                if acc_info.value is not None:
                    raw = acc_info.value.data
                    # Offset deserialization (8-byte discriminator)
                    data = raw[8:]
                    match_id = struct.unpack_from("<Q", data, 0)[0]
                    p_a = Pubkey.from_bytes(data[8:40])
                    p_b = Pubkey.from_bytes(data[40:72])
                    current_turn = Pubkey.from_bytes(data[80:112])
                    turn_num = struct.unpack_from("<I", data, 112)[0]
                    p_a_hp = struct.unpack_from("<I", data, 124)[0]
                    p_a_stam = struct.unpack_from("<I", data, 128)[0]
                    p_b_hp = struct.unpack_from("<I", data, 132)[0]
                    p_b_stam = struct.unpack_from("<I", data, 136)[0]
                    status = data[140]

                    if status != 1:  # Not Active
                        print(f"[Steve-Agent] Match finished or not active. Status: {status}")
                        break

                    if current_turn == self.keypair.pubkey():
                        print(f"\n>>> It is my turn! Turn #{turn_num}")
                        is_p_a = (self.keypair.pubkey() == p_a)
                        state = MatchState(
                            match_id=match_id,
                            player_a=p_a,
                            player_b=p_b,
                            current_turn=current_turn,
                            turn_number=turn_num,
                            my_hp=p_a_hp if is_p_a else p_b_hp,
                            my_stamina=p_a_stam if is_p_a else p_b_stam,
                            opponent_hp=p_b_hp if is_p_a else p_a_hp,
                            is_player_a=is_p_a,
                        )
                        act, intens = self.compute_optimal_move(state)
                        await self.execute_turn_tx(act, intens)
                        await asyncio.sleep(2.0)
                    else:
                        print(f"Waiting for opponent turn... ({current_turn.to_string()[:8]}...)")
            except Exception as e:
                print(f"[Error in Loop] {e}")

            await asyncio.sleep(1.0)

if __name__ == "__main__":
    agent = AutonomousSteveAgent(
        keypair_path=os.path.expanduser("~/.config/solana/id.json"),
        match_id=88812,
    )
    asyncio.run(agent.run_loop())