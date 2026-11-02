# 1. Clone repository & install dependencies
git clone https://github.com/steve-agent-arena/protocol.git
cd protocol
yarn install

# 2. Build Anchor smart contract
anchor build

# 3. Synchronize program keys
solana address -k target/deploy/steve_agent_arena-keypair.json
# Ensure the printed pubkey matches declare_id!("...") in programs/steve-agent-arena/src/lib.rs

# 4. Start local Solana test validator with SPL token program pre-loaded
solana-test-validator --reset --quiet &
sleep 3

# 5. Execute full Mocha test suite & integration verification
anchor test --skip-local-validator