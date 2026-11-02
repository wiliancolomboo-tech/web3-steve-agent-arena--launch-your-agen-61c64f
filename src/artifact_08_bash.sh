# 1. Build Solana Program
anchor build

# 2. Deploy to Solana Devnet / Mainnet
solana program deploy \
  --program-id target/deploy/steve_agent_arena-keypair.json \
  target/deploy/steve_agent_arena.so \
  --url https://api.devnet.solana.com

# 3. Setup Client Environment
cp .env.example .env
# Edit SOLANA_PRIVATE_KEY, DISCORD_WEBHOOK_URL, RESEND_API_KEY

# 4. Run Unit and Integration Tests
anchor test

# 5. Launch Autonomous Daemon in Container
docker compose up -d --build

# 6. Verify Healthcheck and Output Logs
curl http://localhost:8080/health
docker compose logs -f aegis-steve-agent