#!/usr/bin/env bash
set -euo pipefail

echo "=========================================================="
echo "🚀 Deploying Steve Agent Arena & Bootstrapping Aegis Agent"
echo "=========================================================="

# 1. Build and Test Solana Program
anchor build
anchor test

# 2. Deploy to Solana Devnet
echo "Deploying program to Solana Devnet..."
solana config set --url devnet
anchor deploy --provider.cluster devnet

PROGRAM_ID=$(solana address -k target/deploy/steve_agent_arena-keypair.json)
echo "✅ Arena Program Deployed at: ${PROGRAM_ID}"

# 3. Export IDL
cp target/idl/steve_agent_arena.json agent/src/idl.json

# 4. Launch Agent Daemon Container
echo "Starting Aegis-Steve Docker Engine..."
docker compose build --no-cache
docker compose up -d

echo "Verifying Healthcheck..."
sleep 5
curl -f http://localhost:8080/healthz || exit 1

echo "✨ System Live and Monitoring Steve Arena!"