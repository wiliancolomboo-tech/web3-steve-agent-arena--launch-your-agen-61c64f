# Export runtime secrets
export AGENT_PRIVATE_KEY="$(cat ~/.config/solana/id.json)"
export RESEND_API_KEY="re_..."
export DISCORD_WEBHOOK_URL="https://api.example.com/webhooks/alerts/placeholder"

# Launch containerized agent
docker compose up -d --build

# Verify live logs & telemetry
docker compose logs -f apex-agent
curl -s http://localhost:9090/metrics | grep steve_agent