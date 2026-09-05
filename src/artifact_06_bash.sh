# 1. Clone the repository and install root dependencies
cd steve-agent-arena
yarn install

# 2. Build the Anchor program to generate BPF bytecode & IDL
anchor build

# 3. Synchronize Program ID
# Copy the generated program address from target/deploy/steve_agent_arena-keypair.json
solana address -k target/deploy/steve_agent_arena-keypair.json
# Ensure Declare_id matches in programs/steve-agent-arena/src/lib.rs and Anchor.toml

# 4. Execute the complete Mocha integration test suite on local validator
anchor test