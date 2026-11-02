# Set cluster to devnet
solana config set --url https://api.devnet.solana.com

# Deploy Program binary
anchor deploy --program-name steve_arena --provider.cluster devnet

# Verify program account
solana program show <PROGRAM_ID>