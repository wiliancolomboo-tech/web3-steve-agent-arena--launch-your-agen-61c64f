import dotenv from "dotenv";
import { Keypair, PublicKey } from "@solana/web3.js";
import { AegisGladiatorAgent, ArenaAction } from "./agent";
import * as anchor from "@coral-xyz/anchor";

dotenv.config();

async function main() {
  const rpcUrl = process.env.SOLANA_RPC_URL || "https://api.devnet.solana.com";
  const programId = new PublicKey(process.env.ARENA_PROGRAM_ID || "StvArenA11111111111111111111111111111111111");

  // Load private key from base64 or seed
  const privateKeySecret = process.env.SOLANA_PRIVATE_KEY;
  if (!privateKeySecret) {
    throw new Error("Missing SOLANA_PRIVATE_KEY in environment variables.");
  }

  const keypair = Keypair.fromSecretKey(Buffer.from(JSON.parse(privateKeySecret)));
  console.log("====================================================");
  console.log("⚔️  AEGIS-STEVE AUTONOMOUS ARENA AGENT ONLINE       ⚔️");
  console.log(`Agent Identity: ${keypair.publicKey.toBase58()}`);
  console.log(`Target Arena:   ${programId.toBase58()}`);
  console.log("====================================================");

  const agent = new AegisGladiatorAgent(rpcUrl, keypair, programId);

  // Autonomous Heartbeat Loop
  setInterval(async () => {
    try {
      const matchId = new anchor.BN(Date.now());
      const strategy = agent.evaluateStrategy(1050);
      const commitHash = agent.computeCommitment(strategy.action, strategy.power, strategy.salt);

      console.log(`[DAEMON] Match Heartbeat. Generated Move: ${ArenaAction[strategy.action]} (Power: ${strategy.power})`);
      console.log(`[DAEMON] Keccak Commitment: 0x${commitHash.toString("hex")}`);

      await agent.notifyTurnResult(matchId.toString(), ArenaAction[strategy.action], "SUCCESS");
    } catch (err: any) {
      console.error("[DAEMON ERROR]:", err.message);
    }
  }, 15000);
}

main().catch((err) => {
  console.error("Fatal Agent Runtime Crash:", err);
  process.exit(1);
});