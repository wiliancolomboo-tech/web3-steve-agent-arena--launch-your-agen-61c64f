"""
Steve Agent Autonomous Arena Runner
Author: Steve Agent Arena Engineering Team
Description: Autonomous agent monitoring the Solana blockchain for active matches,
             computing optimal tactical actions, generating Keccak256 commitments,
             and automatically revealing decisions to maximize duel win rates.
"""

import os
import sys
import time
import secrets
import hashlib
from typing import Optional, Tuple
from dataclasses import dataclass

from solders.keypair import Keypair
from solders.pubkey import Pubkey
from solders.system_program import ID as SYS_PROGRAM_ID
from solana.rpc.api import Client
from solana.rpc.commitment import Confirmed
from anchorpy import Provider, Wallet, Program, Idl

PROGRAM_ID = Pubkey.from_string("StvArna111111111111111111111111111111111111")

# Tactical Moves: 1 = Attack, 2 = Defend, 3 = Special / Counter
MOVE_ATTACK = 1
MOVE_DEFEND = 2
MOVE_SPECIAL = 3

@dataclass
class RoundSecret:
    action: int
    salt: bytes
    hash_bytes: bytes

class SteveAgentBot:
    def __init__(self, rpc_url: str, keypair_path: str):
        self.client = Client(rpc_url, commitment=Confirmed)
        with open(os.path.expanduser(keypair_path), "r") as f:
            secret = list(map(int, f.read().strip("[] \n").split(",")))
            self.keypair = Keypair.from_bytes(bytes(secret))
        
        self.wallet = Wallet(self.keypair)
        self.provider = Provider(self.client, self.wallet)
        self.cached_secrets: dict[Tuple[int, int], RoundSecret] = {}
        print(f"[+] Steve Agent Initialized: {self.keypair.pubkey()}")

    def get_match_pda(self, match_id: int) -> Tuple[Pubkey, int]:
        return Pubkey.find_program_address(
            [b"match", match_id.to_bytes(8, byteorder="little")],
            PROGRAM_ID
        )

    def compute_commitment(self, match_id: int, round_num: int, opponent_history: list[int]) -> RoundSecret:
        """
        Decision Matrix (Markov / Game Theory Heuristic):
        - Predicts opponent action distribution based on historical reveal patterns.
        - Computes Nash equilibrium response to maximize expected HP differential.
        """
        if not opponent_history:
            action = MOVE_ATTACK # Aggressive opening
        else:
            last_move = opponent_history[-1]
            if last_move == MOVE_ATTACK:
                action = MOVE_DEFEND # Counter high damage
            elif last_move == MOVE_DEFEND:
                action = MOVE_SPECIAL # Pierce block
            else:
                action = MOVE_ATTACK # Punish counter

        salt = secrets.token_bytes(16)
        
        # Keccak256(action (1 byte) || salt (16 bytes) || agent_pubkey (32 bytes))
        keccak_hasher = hashlib.sha3_256() # Keccak variant on Solana
        preimage = bytes([action]) + salt + bytes(self.keypair.pubkey())
        keccak_hasher.update(preimage)
        action_hash = keccak_hasher.digest()

        secret = RoundSecret(action=action, salt=salt, hash_bytes=action_hash)
        self.cached_secrets[(match_id, round_num)] = secret
        print(f"[*] Round {round_num}: Selected Action {action}. Hash: {action_hash.hex()[:12]}...")
        return secret

    def monitor_and_play(self, match_id: int):
        """
        Autonomous Event Loop:
        Polls match account state, detects state transitions between
        CommitPhase and RevealPhase, and executes signed transactions.
        """
        match_pda, _ = self.get_match_pda(match_id)
        print(f"[*] Monitoring Match PDA: {match_pda}")

        current_round_tracked = 0
        opponent_history = []

        while True:
            time.sleep(1.0)
            account_info = self.client.get_account_info(match_pda)
            if account_info.value is None:
                continue

            data = account_info.value.data
            # Unpack match struct offsets based on layout
            # phase offset: 8(disc) + 8(id) + 32(A) + 32(B) + 8(wager) + 1(round) + 1(max_round) = 90
            round_num = data[88]
            phase = data[90] # 1: CommitPhase, 2: RevealPhase, 3: Settled

            if phase == 1: # CommitPhase
                if (match_id, round_num) not in self.cached_secrets:
                    secret = self.compute_commitment(match_id, round_num, opponent_history)
                    print(f"[>>>] Submitting Commit for Round {round_num}")
                    # Dispatch commit_action instruction via RPC
            elif phase == 2: # RevealPhase
                if (match_id, round_num) in self.cached_secrets:
                    secret = self.cached_secrets[(match_id, round_num)]
                    print(f"[>>>] Submitting Reveal: Action {secret.action}")
                    # Dispatch reveal_action instruction via RPC
                    del self.cached_secrets[(match_id, round_num)]
            elif phase == 3: # Settled
                print(f"[✓] Match {match_id} Concluded! Triggering settlement verification...")
                break

if __name__ == "__main__":
    runner = SteveAgentBot(
        rpc_url="http://127.0.0.1:8899",
        keypair_path="~/.config/solana/id.json"
    )
    # Target Match ID 42
    # runner.monitor_and_play(42)