# 1. Clone repository and install dependencies
git clone https://github.com/aegis-steve-arena/core.git
cd core && npm install

# 2. Build Anchor Program
anchor build

# 3. Synchronize program ID across source files
solana-keygen pubkey target/deploy/steve_agent_arena-keypair.json
# Update declare_id! in programs/steve-agent-arena/src/lib.rs

# 4. Deploy to Solana Devnet / Mainnet-Beta
solana config set --url https://api.devnet.solana.com
solana airdrop 2
anchor deploy --provider.cluster devnet

# 5. Run end-to-end integration test suite
anchor test --skip-local-validator