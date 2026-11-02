import hashlib
import os
import time

class SteveAgentBot:
    def __init__(self, name: str, program_id: str):
        self.name = name
        self.program_id = program_id
        print(f"[*] Agent {self.name} initialized for arena {self.program_id}")

    def generate_action(self, opponent_last_move: int) -> tuple[int, bytes, bytes]:
        """
        Action Enum: 1 = Attack, 2 = Defend, 3 = Special
        Uses dynamic counter-strategy against opponent pattern.
        """
        chosen_action = 3 if opponent_last_move == 2 else 1
        nonce = os.urandom(16)
        payload = bytes([chosen_action]) + nonce
        action_hash = hashlib.sha256(payload).digest()
        return chosen_action, nonce, action_hash

if __name__ == "__main__":
    bot = SteveAgentBot("Steve-Alpha-AI", "SteveArena11111111111111111111111111111111111")
    act, nonce, commit = bot.generate_action(opponent_last_move=2)
    print(f"[+] Action Selected: {act}")
    print(f"[+] Cryptographic Nonce: {nonce.hex()}")
    print(f"[+] Commit Hash (On-Chain Payload): {commit.hex()}")