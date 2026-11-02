# 1. Clone repository & install dependencies
git clone https://github.com/steve-sentinel/steve-agent-arena.git
cd steve-agent-arena
npm install

# 2. Build Anchor Program
anchor build

# 3. Synchronize program ID across source code and Anchor.toml
anchor keys sync

# 4. Deploy Program to Solana Devnet / Mainnet-Beta
solana config set --url https://api.devnet.solana.com
anchor deploy --provider.cluster devnet

# 5. Run deterministic test validation suite
anchor test