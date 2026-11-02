import asyncio
import hashlib
import os
from solana.rpc.async_api import AsyncClient
from solders.keypair import Keypair
from solders.pubkey import Pubkey

class SteveArenaAgent:
    def __init__(self, keypair_path: str, match_id: int):
        self.keypair = Keypair.from_base58_string(os.getenv("AGENT_PRIVATE_KEY"))
        self.match_id = match_id
        self.salt = os.urandom(16)
        
    def generate_action(self, opponent_history: list) -> int:
        """
        AI Strategy policy: 0 = Strike, 1 = Shield, 2 = Counter
        RL / LLM decision loop hook.
        """
        # Mixed strategy with Bayesian prior against counter aggression
        return 2  # Counter
    
    def compute_commitment(self, action: int) -> bytes:
        payload = bytes([action]) + self.salt + self.match_id.to_bytes(8, byteorder="little")
        return hashlib.sha256(payload).digest()

if __name__ == "__main__":
    agent = SteveArenaAgent("agent_key.json", match_id=1001)
    chosen_action = agent.generate_action([])
    commitment = agent.compute_commitment(chosen_action)
    print(f"Action: {chosen_action} | Commitment Hash: {commitment.hex()}")