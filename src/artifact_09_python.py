"""
Steve Autonomous Agent Bot: Executes competitive strategy loops on Solana.
Calculates game actions, computes cryptographic salt commitments, and posts transactions.
"""

import hashlib
import json
import os
import secrets
import sys
import time
from typing import Tuple

from solana.rpc.api import Client
from solders.keypair import Keypair
from solders.pubkey import Pubkey

RPC_ENDPOINT = os.getenv("SOLANA_RPC_URL", "http://127.0.0.1:8899")
PROGRAM_ID = Pubkey.from_string("SteveArena111111111111111111111111111111111")


class SteveAgent:
    def __init__(self, keypair_path: str):
        with open(keypair_path, "r") as f:
            secret = json.load(f)
        self.keypair = Keypair.from_bytes(bytes(secret))
        self.client = Client(RPC_ENDPOINT)
        self.salt_cache = {}
        print(f"[+] Steve Autonomous Bot Loaded. Public Key: {self.keypair.pubkey()}")

    def generate_strategy(self, opponent_history: list) -> Tuple[int, bytes, bytes]:
        """
        Game-Theoretic action selection:
        1: Rock, 2: Paper, 3: Scissors.
        Uses counter-frequency weighting against opponent history with random entropy.
        """
        if not opponent_history:
            action = secrets.choice([1, 2, 3])
        else:
            most_frequent = max(set(opponent_history), key=opponent_history.count)
            # Counter the opponent's modal move
            counter_map = {1: 2, 2: 3, 3: 1}
            action = counter_map.get(most_frequent, secrets.choice([1, 2, 3]))

        salt = secrets.token_bytes(32)
        hasher = hashlib.sha256()
        hasher.update(bytes([action]))
        hasher.update(salt)
        commit_hash = hasher.digest()

        return action, salt, commit_hash

    def monitor_and_act(self, match_id: int):
        print(f"[*] Monitoring arena state for Match ID: {match_id}...")
        action, salt, commit_hash = self.generate_strategy(opponent_history=[1, 1, 2])
        self.salt_cache[match_id] = (action, salt)

        print(f"[!] Action Chosen: {action} | Commit Hash: {commit_hash.hex()[:16]}...")
        # Transaction submission logic uses anchor client or raw solders instructions
        print("[+] Action commitment prepared for on-chain submission.")


if __name__ == "__main__":
    dummy_keypair_path = "/tmp/steve_bot_keypair.json"
    if not os.path.exists(dummy_keypair_path):
        with open(dummy_keypair_path, "w") as f:
            json.dump(list(Keypair().to_bytes()), f)

    bot = SteveAgent(dummy_keypair_path)
    bot.monitor_and_act(match_id=1001)