# 1. Install prerequisites
sh -c "$(curl -sSfL https://release.solana.com/v1.18.26/install)"
cargo install --git https://github.com/coral-xyz/anchor --tag v0.30.1 anchor-cli --locked

# 2. Build on-chain program
anchor build

# 3. Synchronize Program ID
solana address -k target/deploy/steve_agent_arena-keypair.json
# (Paste output into declare_id! in lib.rs and Anchor.toml)

# 4. Deploy to Solana Devnet
anchor deploy --provider.cluster devnet

# 5. Execute 100% Assertion QA Suite
anchor test

# 6. Spin up Turnkey Agent Daemon
docker compose up -d --build
docker compose logs -f steve-agent-daemon