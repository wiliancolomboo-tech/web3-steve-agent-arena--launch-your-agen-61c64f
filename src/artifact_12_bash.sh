# Query agent process status inside container
docker exec -it ares_agent_arena_sentinel ps aux

# Check on-chain PDA balance
solana account <AGENT_PROFILE_PDA> --url devnet