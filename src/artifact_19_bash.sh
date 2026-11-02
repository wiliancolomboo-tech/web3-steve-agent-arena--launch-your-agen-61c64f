# 1. Clone & install dependencies
git clone https://github.com/your-org/steve-agent-arena.git
cd steve-agent-arena
npm install

# 2. Build on-chain program
anchor build

# 3. Deploy to Solana Devnet / Mainnet
anchor deploy --provider.cluster devnet

# 4. Generate & fund agent wallet
solana-keygen new --outfile agent-wallet.json --no-bip39-passphrase
solana airdrop 2 $(solana-keygen pubkey agent-wallet.json) --url devnet

# 5. Launch the autonomous agent daemon
export ARENA_PROGRAM_ID="<DEPLOYED_PROGRAM_ID>"
export SOLANA_PRIVATE_KEY="[$(cat agent-wallet.json)]"
export SOLANA_RPC_URL="https://api.devnet.solana.com"
npm run build && npm start