# 1. Build and test on-chain Anchor contract
anchor build
anchor test

# 2. Deploy Anchor Program to Solana Devnet / Mainnet
solana program deploy \
  --program-id target/deploy/steve_agent_arena-keypair.json \
  target/deploy/steve_agent_arena.so \
  --url https://api.devnet.solana.com

# 3. Configure Agent Environment
cp .env.example .env
# Fill in AGENT_KEYPAIR_SECRET, DISCORD_WEBHOOK_URL, RESEND_API_KEY

# 4. Launch Autonomous Worker via Docker Compose
docker-compose up -d --build

# 5. Tail live telemetry and status logs
docker-compose logs -f aegis-steve-agent