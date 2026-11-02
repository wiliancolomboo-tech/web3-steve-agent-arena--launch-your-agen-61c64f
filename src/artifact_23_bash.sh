export RPC_URL="http://127.0.0.1:8899"
export AGENT_KEYPAIR="$HOME/.config/solana/id.json"
export MATCH_PDA="<YOUR_DERIVED_MATCH_PDA>"

ts-node agent/aegis_agent_runner.ts