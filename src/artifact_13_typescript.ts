import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { SteveAgentArena } from "../target/types/steve_agent_arena";
import { Keypair, LAMPORTS_PER_SOL, PublicKey, SystemProgram } from "@solana/web3.js";
import { expect } from "chai";
import * as crypto from "crypto";

describe("Steve Agent Arena - Autonomous Dual Contract Tests", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.SteveAgentArena as Program<SteveAgentArena>;

  const arenaAuthority = Keypair.generate();
  const challenger = Keypair.generate();
  const opponent = Keypair.generate();

  const arenaId = new anchor.BN(999);
  const matchId = new anchor.BN(55555);
  const entryFee = new anchor.BN(0.1 * LAMPORTS_PER_SOL);
  const durationSlots = new anchor.BN(100);

  let arenaPda: PublicKey;
  let challengerAgentPda: PublicKey;
  let opponentAgentPda: PublicKey;
  let matchPda: PublicKey;
  let vaultPda: PublicKey;

  before(async () => {
    // Airdrop funds to all actors
    const airdropSigners = [arenaAuthority, challenger, opponent];
    for (const actor of airdropSigners) {
      const sig = await provider.connection.requestAirdrop(actor.publicKey, 5 * LAMPORTS_PER_SOL);
      await provider.connection.confirmTransaction(sig, "confirmed");
    }

    [arenaPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("arena"), arenaId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );

    [challengerAgentPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("agent"), arenaPda.toBuffer(), challenger.publicKey.toBuffer()],
      program.programId
    );

    [opponentAgentPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("agent"), arenaPda.toBuffer(), opponent.publicKey.toBuffer()],
      program.programId
    );

    [matchPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("match"), matchId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );

    [vaultPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("vault"), matchId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );
  });

  it("1. Initializes Arena with valid fee structure and parameter boundaries", async () => {
    await program.methods
      .initializeArena(arenaId, entryFee, durationSlots)
      .accounts({
        arena: arenaPda,
        authority: arenaAuthority.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([arenaAuthority])
      .rpc();

    const arenaAccount = await program.account.arenaState.fetch(arenaPda);
    expect(arenaAccount.authority.toBase58()).to.equal(arenaAuthority.publicKey.toBase58());
    expect(arenaAccount.entryFee.toNumber()).to.equal(entryFee.toNumber());
    expect(arenaAccount.roundDurationSlots.toNumber()).to.equal(durationSlots.toNumber());
    expect(arenaAccount.paused).to.be.false;
  });

  it("2. Registers Challenger and Opponent Autonomous Agents", async () => {
    const hashChallenger = crypto.createHash("sha256").update("AlphaAegis").digest();
    const hashOpponent = crypto.createHash("sha256").update("BetaSteve").digest();

    await program.methods
      .registerAgent("AlphaAegis", Array.from(hashChallenger))
      .accounts({
        agent: challengerAgentPda,
        arena: arenaPda,
        authority: challenger.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([challenger])
      .rpc();

    await program.methods
      .registerAgent("BetaSteve", Array.from(hashOpponent))
      .accounts({
        agent: opponentAgentPda,
        arena: arenaPda,
        authority: opponent.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([opponent])
      .rpc();

    const cAgent = await program.account.agentAccount.fetch(challengerAgentPda);
    expect(cAgent.name).to.equal("AlphaAegis");
    expect(cAgent.wins.toNumber()).to.equal(0);
  });

  it("3. Creates a Match and locks escrow funds into PDA Vault", async () => {
    const vaultBalBefore = await provider.connection.getBalance(vaultPda);
    expect(vaultBalBefore).to.equal(0);

    await program.methods
      .createMatch(matchId)
      .accounts({
        matchAccount: matchPda,
        vault: vaultPda,
        arena: arenaPda,
        challenger: challenger.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([challenger])
      .rpc();

    const matchAcc = await program.account.matchSession.fetch(matchPda);
    expect(matchAcc.challenger.toBase58()).to.equal(challenger.publicKey.toBase58());
    expect(matchAcc.wagerAmount.toNumber()).to.equal(entryFee.toNumber());

    const vaultBalAfter = await provider.connection.getBalance(vaultPda);
    expect(vaultBalAfter).to.equal(entryFee.toNumber());
  });

  it("4. Prevents an agent from dueling against itself", async () => {
    try {
      await program.methods
        .joinMatch()
        .accounts({
          matchAccount: matchPda,
          vault: vaultPda,
          arena: arenaPda,
          opponent: challenger.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([challenger])
        .rpc();
      expect.fail("Should have thrown CannotDuelSelf error");
    } catch (err: any) {
      expect(err.error.errorCode.code).to.equal("CannotDuelSelf");
    }
  });

  it("5. Opponent joins match, locking matching entry fee", async () => {
    await program.methods
      .joinMatch()
      .accounts({
        matchAccount: matchPda,
        vault: vaultPda,
        arena: arenaPda,
        opponent: opponent.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([opponent])
      .rpc();

    const vaultBal = await provider.connection.getBalance(vaultPda);
    expect(vaultBal).to.equal(entryFee.toNumber() * 2);
  });

  it("6. Submits actions deterministically (1: Attack vs 2: Counter => Challenger Wins)", async () => {
    const salt1 = crypto.randomBytes(16);
    const salt2 = crypto.randomBytes(16);

    // Challenger plays 1 (Attack)
    await program.methods
      .submitAction(1, Array.from(salt1))
      .accounts({
        matchAccount: matchPda,
        agentAuthority: challenger.publicKey,
      })
      .signers([challenger])
      .rpc();

    // Opponent plays 2 (Counter)
    await program.methods
      .submitAction(2, Array.from(salt2))
      .accounts({
        matchAccount: matchPda,
        agentAuthority: opponent.publicKey,
      })
      .signers([opponent])
      .rpc();

    const matchAcc = await program.account.matchSession.fetch(matchPda);
    expect(matchAcc.challengerCommitted).to.be.true;
    expect(matchAcc.opponentCommitted).to.be.true;
  });

  it("7. Settles duel, disburses prize to winner, and allocates protocol fee", async () => {
    const challengerBalBefore = await provider.connection.getBalance(challenger.publicKey);
    const treasuryBalBefore = await provider.connection.getBalance(arenaAuthority.publicKey);

    await program.methods
      .settleDuel()
      .accounts({
        matchAccount: matchPda,
        vault: vaultPda,
        arenaAuthority: arenaAuthority.publicKey,
        arena: arenaPda,
        challengerAgent: challengerAgentPda,
        opponentAgent: opponentAgentPda,
        challenger: challenger.publicKey,
        opponent: opponent.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .rpc();

    const challengerBalAfter = await provider.connection.getBalance(challenger.publicKey);
    const treasuryBalAfter = await provider.connection.getBalance(arenaAuthority.publicKey);

    // 0.2 SOL total pool - 2.5% (0.005 SOL) = 0.195 SOL prize
    expect(challengerBalAfter).to.be.greaterThan(challengerBalBefore);
    expect(treasuryBalAfter).to.be.greaterThan(treasuryBalBefore);

    const cAgent = await program.account.agentAccount.fetch(challengerAgentPda);
    expect(cAgent.wins.toNumber()).to.equal(1);
    expect(cAgent.totalScore.toNumber()).to.equal(100);

    const vaultBalAfter = await provider.connection.getBalance(vaultPda);
    expect(vaultBalAfter).to.equal(0);
  });
});