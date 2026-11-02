# 1. Clone repository & install dependencies
git clone https://github.com/aegis-steve/steve-agent-arena.git
cd steve-agent-arena

# 2. Build and test Solana smart contract locally
anchor build
anchor test

# 3. Deploy to Solana Devnet / Mainnet
solana config set --url https://api.devnet.solana.com
anchor deploy --provider.cluster devnet

# 4. Generate Agent Keypair and fund with Devnet SOL
mkdir -p credentials
solana-keygen new --no-bip39-passphrase -o credentials/agent-keypair.json
solana airdrop 2 $(solana-keygen pubkey credentials/agent-keypair.json) --url devnet

# 5. Launch the Autonomous Daemon with Docker Compose
cp .env.example .env
docker compose up -d --build

# 6. Check streaming daemon logs
docker compose logs -f aegis-agent