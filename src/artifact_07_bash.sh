# Set Solana CLI config to Devnet
solana config set --url https://api.devnet.solana.com

# Fund deployer authority with Devnet SOL
solana airdrop 2

# Deploy program binary to Devnet
anchor deploy --provider.cluster devnet

# Run automated Python Agent Bot runner against the deployment
python3 -m venv venv
source venv/bin/activate
pip install anchorpy solana solders
SOLANA_RPC_URL="https://api.devnet.solana.com" python3 bot/agent_runner.py