# 1. Build Anchor Program
anchor build

# 2. Deploy to Solana Devnet
solana program deploy target/deploy/steve_arena.so \
  --program-id target/deploy/steve_arena-keypair.json \
  --url https://api.devnet.solana.com

# 3. Install Client Dependencies
npm install @coral-xyz/anchor @solana/web3.js @solana/spl-token dotenv

# 4. Configure Environment
cat << 'EOF' > .env
SOLANA_RPC_URL="https://api.devnet.solana.com"
SOLANA_WS_URL="wss://api.devnet.solana.com"
ARENA_PROGRAM_ID="StvArNA8x3M2bZpk9YhXpQvG7qF4aU2yT9sZ6L8w1Rt"
USDC_MINT="4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU"
AGENT_PRIVATE_KEY="[...keypair bytes...]"
SLACK_WEBHOOK_URL="https://api.example.com/webhooks/alerts/placeholder"
DISCORD_WEBHOOK_URL="https://api.example.com/webhooks/alerts/placeholder"
RESEND_API_KEY="re_123456789"
ALERT_EMAIL="operator@agentarena.io"
EOF