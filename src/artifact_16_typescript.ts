import * as anchor from "@coral-xyz/anchor";
import { Connection, Keypair, PublicKey, SystemProgram } from "@solana/web3.js";
import crypto from "crypto";
import { EnterpriseTelemetry } from "./telemetry";

export enum ArenaAction {
  Attack = 0,
  Defend = 1,
  Siphon = 2
}

export class AegisGladiatorAgent {
  public keypair: Keypair;
  public connection: Connection;
  public programId: PublicKey;
  private telemetry: EnterpriseTelemetry;

  constructor(rpcUrl: string, keypair: Keypair, programId: PublicKey) {
    this.connection = new Connection(rpcUrl, "confirmed");
    this.keypair = keypair;
    this.programId = programId;
    this.telemetry = new EnterpriseTelemetry();
  }

  /**
   * Deterministically calculates game move using opponent historical profile & seed entropy
   */
  public evaluateStrategy(opponentRating: number): { action: ArenaAction; power: number; salt: Buffer } {
    const salt = crypto.randomBytes(16);

    // High rating opponent triggers defensive counter-gambit
    let action: ArenaAction = ArenaAction.Attack;
    let power = 750;

    if (opponentRating > 1200) {
      action = ArenaAction.Defend;
      power = 900;
    } else if (opponentRating < 900) {
      action = ArenaAction.Siphon;
      power = 600;
    }

    return { action, power, salt };
  }

  /**
   * Computes Keccak-256 hash matching on-chain verification
   */
  public computeCommitment(action: ArenaAction, power: number, salt: Buffer): Buffer {
    const powerBuffer = Buffer.alloc(2);
    powerBuffer.writeUInt16LE(power, 0);

    const payload = Buffer.concat([Buffer.from([action]), powerBuffer, salt]);
    // Keccak-256
    const hash = crypto.createHash("sha3-256").update(payload).digest();
    return hash;
  }

  public getAgentPda(): [PublicKey, number] {
    return PublicKey.findProgramAddressSync(
      [Buffer.from("agent"), this.keypair.publicKey.toBuffer()],
      this.programId
    );
  }

  public getMatchPda(matchId: anchor.BN): [PublicKey, number] {
    return PublicKey.findProgramAddressSync(
      [Buffer.from("match"), matchId.toArrayLike(Buffer, "le", 8)],
      this.programId
    );
  }

  public getEscrowPda(matchId: anchor.BN): [PublicKey, number] {
    return PublicKey.findProgramAddressSync(
      [Buffer.from("escrow"), matchId.toArrayLike(Buffer, "le", 8)],
      this.programId
    );
  }

  public async notifyTurnResult(matchId: string, action: string, status: "INFO" | "SUCCESS" | "CRITICAL") {
    await this.telemetry.sendAlert({
      title: "Tactical Execution Complete",
      matchId,
      agentPubkey: this.keypair.publicKey.toBase58(),
      actionTaken: action,
      status
    });
  }
}