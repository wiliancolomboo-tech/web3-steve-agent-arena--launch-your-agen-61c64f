import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { Keypair, SystemProgram, LAMPORTS_PER_SOL, PublicKey } from "@solana/web3.js";
import { expect } from "chai";
import crypto from "crypto";

describe("Steve Agent Arena - Full Test Suite", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  // Load IDL dynamically for testing
  const idl = require("../target/idl/steve_agent_arena.json");
  const programId = new PublicKey("StvArn1111111111111111111111111111111111111");
  const program = new Program(idl, provider);

  const playerOne = Keypair.generate();
  const playerTwo = Keypair.generate();
  const wager = new anchor.BN(0.05 * LAMPORTS_PER_SOL);

  before(async () => {
    // Airdrop testing SOL to participants
    const sig1 = await provider.connection.requestAirdrop(playerOne.publicKey, 2 * LAMPORTS_PER_SOL);
    const sig2 = await provider.connection.requestAirdrop(playerTwo.publicKey, 2 * LAMPORTS_PER_SOL);
    await provider.connection.confirmTransaction(sig1);
    await provider.connection.confirmTransaction(sig2);
  });

  it("1. Successfully registers Agent Profile", async () => {
    const [agentProfilePda] = PublicKey.findProgramAddressSync(
      [Buffer.from("agent_profile"), playerOne.publicKey.toBuffer()],
      program.programId
    );

    await program.methods
      .registerAgent("Aegis-Steve-01", "https://metadata.aegis.io/agent.json")
      .accounts({
        agentProfile: agentProfilePda,
        authority: playerOne.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([playerOne])
      .rpc();

    const profile = await (program.account as any).agentProfile.fetch(agentProfilePda);
    expect(profile.name).to.equal("Aegis-Steve-01");
    expect(profile.eloRating).to.equal(1000);
    expect(profile.wins).to.equal(0);
  });

  it("2. Creates a Match and locks escrow funds", async () => {
    const matchId = new anchor.BN(1001);
    const [matchPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("match"), matchId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );
    const [escrowPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("arena_escrow"), matchId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );

    const initialP1Bal = await provider.connection.getBalance(playerOne.publicKey);

    await program.methods
      .createMatch(matchId, wager)
      .accounts({
        matchAccount: matchPda,
        arenaEscrow: escrowPda,
        playerOne: playerOne.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([playerOne])
      .rpc();

    const matchAcc = await (program.account as any).matchAccount.fetch(matchPda);
    expect(matchAcc.playerOne.toBase58()).to.equal(playerOne.publicKey.toBase58());
    expect(matchAcc.wagerAmount.toNumber()).to.equal(wager.toNumber());

    const escrowBal = await provider.connection.getBalance(escrowPda);
    expect(escrowBal).to.equal(wager.toNumber());
  });

  it("3. Opponent Joins Match and deposits matching escrow", async () => {
    const matchId = new anchor.BN(1001);
    const [matchPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("match"), matchId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );
    const [escrowPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("arena_escrow"), matchId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );

    await program.methods
      .joinMatch(matchId)
      .accounts({
        matchAccount: matchPda,
        arenaEscrow: escrowPda,
        playerTwo: playerTwo.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([playerTwo])
      .rpc();

    const matchAcc = await (program.account as any).matchAccount.fetch(matchPda);
    expect(matchAcc.playerTwo.toBase58()).to.equal(playerTwo.publicKey.toBase58());
    expect(matchAcc.round).to.equal(1);

    const escrowBal = await provider.connection.getBalance(escrowPda);
    expect(escrowBal).to.equal(wager.toNumber() * 2);
  });

  it("4. Enforces Hash Commitments and verifies Reveal Integrity", async () => {
    const matchId = new anchor.BN(1001);
    const [matchPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("match"), matchId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );

    // P1 chooses Strike (1); P2 chooses Defend (2) -> Defend beats Strike
    const p1Action = 1;
    const p1Salt = crypto.randomBytes(32);
    const p1Preimage = Buffer.concat([Buffer.from([p1Action]), p1Salt]);
    const p1Hash = crypto.createHash("sha256").update(p1Preimage).digest();

    const p2Action = 2;
    const p2Salt = crypto.randomBytes(32);
    const p2Preimage = Buffer.concat([Buffer.from([p2Action]), p2Salt]);
    const p2Hash = crypto.createHash("sha256").update(p2Preimage).digest();

    // Commit P1
    await program.methods
      .commitAction(matchId, Array.from(p1Hash))
      .accounts({ matchAccount: matchPda, signer: playerOne.publicKey })
      .signers([playerOne])
      .rpc();

    // Commit P2
    await program.methods
      .commitAction(matchId, Array.from(p2Hash))
      .accounts({ matchAccount: matchPda, signer: playerTwo.publicKey })
      .signers([playerTwo])
      .rpc();

    let matchAcc = await (program.account as any).matchAccount.fetch(matchPda);
    expect(matchAcc.state.revealPhase).to.be.true;

    // Reveal P1
    await program.methods
      .revealAction(matchId, p1Action, Array.from(p1Salt))
      .accounts({ matchAccount: matchPda, signer: playerOne.publicKey })
      .signers([playerOne])
      .rpc();

    // Reveal P2
    await program.methods
      .revealAction(matchId, p2Action, Array.from(p2Salt))
      .accounts({ matchAccount: matchPda, signer: playerTwo.publicKey })
      .signers([playerTwo])
      .rpc();

    matchAcc = await (program.account as any).matchAccount.fetch(matchPda);
    expect(matchAcc.state.roundResolved).to.be.true;
  });

  it("5. Prevents Forged Reveals with commitment mismatch error", async () => {
    const matchId = new anchor.BN(9999);
    const [matchPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("match"), matchId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );
    const [escrowPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("arena_escrow"), matchId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );

    // Setup dummy match
    await program.methods
      .createMatch(matchId, wager)
      .accounts({
        matchAccount: matchPda,
        arenaEscrow: escrowPda,
        playerOne: playerOne.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([playerOne])
      .rpc();

    await program.methods
      .joinMatch(matchId)
      .accounts({
        matchAccount: matchPda,
        arenaEscrow: escrowPda,
        playerTwo: playerTwo.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([playerTwo])
      .rpc();

    const legitimateAction = 1;
    const salt = crypto.randomBytes(32);
    const hash = crypto.createHash("sha256").update(Buffer.concat([Buffer.from([legitimateAction]), salt])).digest();

    await program.methods
      .commitAction(matchId, Array.from(hash))
      .accounts({ matchAccount: matchPda, signer: playerOne.publicKey })
      .signers([playerOne])
      .rpc();

    // Try revealing an altered action (e.g. 2 instead of 1)
    try {
      await program.methods
        .revealAction(matchId, 2, Array.from(salt))
        .accounts({ matchAccount: matchPda, signer: playerOne.publicKey })
        .signers([playerOne])
        .rpc();
      expect.fail("Expected CommitmentMismatch error was not thrown");
    } catch (err: any) {
      expect(err.toString()).to.include("CommitmentMismatch");
    }
  });
});