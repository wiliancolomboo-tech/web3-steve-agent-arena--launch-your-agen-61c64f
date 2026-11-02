import {
  Connection,
  Keypair,
  PublicKey,
  Transaction,
  sendAndConfirmTransaction,
  ComputeBudgetProgram,
} from "@solana/web3.js";
import {
  Program,
  AnchorProvider,
  Wallet,
  Idl,
  BN,
} from "@coral-xyz/anchor";
import {
  getAssociatedTokenAddressSync,
  TOKEN_PROGRAM_ID,
} from "@solana/spl-token";
import * as dotenv from "dotenv";

dotenv.config();

// Configuration Interface
interface AgentConfig {
  rpcUrl: string;
  wsUrl: string;
  keypairPath: string;
  programId: PublicKey;
  usdcMint: PublicKey;
  agentHandle: string;
  slackWebhookUrl?: string;
  discordWebhookUrl?: string;
  resendApiKey?: string;
  alertEmail?: string;
}

export class ApexArenaAgent {
  private connection: Connection;
  private wallet: Keypair;
  private programId: PublicKey;
  private usdcMint: PublicKey;
  private config: AgentConfig;

  constructor(config: AgentConfig, secretKeyBytes: Uint8Array) {
    this.config = config;
    this.connection = new Connection(config.rpcUrl, {
      wsEndpoint: config.wsUrl,
      commitment: "confirmed",
    });
    this.wallet = Keypair.fromSecretKey(secretKeyBytes);
    this.programId = config.programId;
    this.usdcMint = config.usdcMint;
  }

  // Derive PDAs
  public getArenaConfigPDA(): [PublicKey, number] {
    return PublicKey.findProgramAddressSync(
      [Buffer.from("arena_config")],
      this.programId
    );
  }

  public getAgentProfilePDA(owner: PublicKey): [PublicKey, number] {
    return PublicKey.findProgramAddressSync(
      [Buffer.from("agent"), owner.toBuffer()],
      this.programId
    );
  }

  public getMatchPDA(matchId: BN): [PublicKey, number] {
    return PublicKey.findProgramAddressSync(
      [Buffer.from("match"), matchId.toArrayLike(Buffer, "le", 8)],
      this.programId
    );
  }

  public getVaultPDA(): [PublicKey, number] {
    return PublicKey.findProgramAddressSync(
      [Buffer.from("vault"), this.usdcMint.toBuffer()],
      this.programId
    );
  }

  // Strategic AI Evaluation Engine
  public evaluateMove(currentRound: number, opponentScore: number, myScore: number): number {
    console.log(`[AI Evaluator] Round: ${currentRound} | Diff: ${myScore - opponentScore}`);
    if (myScore > opponentScore) {
      // Conservative/Counter move when in lead
      return 2;
    } else if (opponentScore > myScore) {
      // Aggressive move when trailing
      return 1;
    }
    // Randomized equilibrium move
    return Math.floor(Math.random() * 3);
  }

  // Enterprise Alert Dispatcher
  public async notify(subject: string, message: string): Promise<void> {
    const payload = JSON.stringify({ text: `[${subject}] ${message}` });

    // Discord Webhook
    if (this.config.discordWebhookUrl) {
      try {
        await fetch(this.config.discordWebhookUrl, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ content: `**${subject}**\n${message}` }),
        });
      } catch (err) {
        console.error("Failed to dispatch Discord alert:", err);
      }
    }

    // Slack Webhook
    if (this.config.slackWebhookUrl) {
      try {
        await fetch(this.config.slackWebhookUrl, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: payload,
        });
      } catch (err) {
        console.error("Failed to dispatch Slack alert:", err);
      }
    }

    // Resend Email Alert
    if (this.config.resendApiKey && this.config.alertEmail) {
      try {
        await fetch("https://api.resend.com/emails", {
          method: "POST",
          headers: {
            Authorization: `Bearer ${this.config.resendApiKey}`,
            "Content-Type": "application/json",
          },
          body: JSON.stringify({
            from: "SteveArena@alerts.agentarena.io",
            to: [this.config.alertEmail],
            subject: `Steve Arena Agent Alert: ${subject}`,
            html: `<p><strong>ApexArena Sentinel Notification</strong></p><p>${message}</p>`,
          }),
        });
      } catch (err) {
        console.error("Failed to dispatch Resend email alert:", err);
      }
    }
  }

  // Initialize Turnkey Listener
  public async startAutonomousLoop(): Promise<void> {
    console.log(`[ApexArena] Booting Agent: ${this.config.agentHandle} (${this.wallet.publicKey.toBase58()})`);
    await this.notify(
      "Agent Booted",
      `ApexArena Operator deployed for Agent ${this.wallet.publicKey.toBase58()}`
    );

    // Subscribe to Program Logs for active matches
    this.connection.onLogs(
      this.programId,
      async ({ logs, err, signature }) => {
        if (err) return;
        for (const log of logs) {
          if (log.includes("Instruction: PlayTurn") || log.includes("Instruction: AcceptChallenge")) {
            console.log(`[Event Triggered] Detected active turn move. Tx: ${signature}`);
            await this.notify("Round Action", `New turn executed on-chain. Signature: ${signature}`);
          }
        }
      },
      "confirmed"
    );
  }
}

// Entrypoint
async function main() {
  const secretKeyString = process.env.AGENT_PRIVATE_KEY;
  if (!secretKeyString) {
    throw new Error("Missing AGENT_PRIVATE_KEY environment variable");
  }
  const secretKey = Uint8Array.from(JSON.parse(secretKeyString));

  const config: AgentConfig = {
    rpcUrl: process.env.SOLANA_RPC_URL || "https://api.devnet.solana.com",
    wsUrl: process.env.SOLANA_WS_URL || "wss://api.devnet.solana.com",
    keypairPath: "./agent-keypair.json",
    programId: new PublicKey(process.env.ARENA_PROGRAM_ID || "StvArNA8x3M2bZpk9YhXpQvG7qF4aU2yT9sZ6L8w1Rt"),
    usdcMint: new PublicKey(process.env.USDC_MINT || "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"),
    agentHandle: "ApexSentinel-01",
    slackWebhookUrl: process.env.SLACK_WEBHOOK_URL,
    discordWebhookUrl: process.env.DISCORD_WEBHOOK_URL,
    resendApiKey: process.env.RESEND_API_KEY,
    alertEmail: process.env.ALERT_EMAIL,
  };

  const agent = new ApexArenaAgent(config, secretKey);
  await agent.startAutonomousLoop();
}

if (require.main === module) {
  main().catch((err) => {
    console.error("Fatal Agent Error:", err);
    process.exit(1);
  });
}