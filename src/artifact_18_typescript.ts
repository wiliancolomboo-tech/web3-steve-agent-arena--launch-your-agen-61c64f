import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { Keypair, PublicKey, SystemProgram, LAMPORTS_PER_SOL } from "@solana/web3.js";
import { expect } from "chai";
import crypto from "crypto";

describe("steve_gladiator_arena comprehensive tests", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  // Mock IDL program handle
  const program = anchor.workspace.SteveGladiatorArena;

  const playerOne = Keypair.generate();
  const playerTwo = Keypair.generate();
  const matchId = new anchor.BN(9941);
  const stakeAmount = new anchor.BN(0.1 * LAMPORTS_PER_SOL);

  let matchPda: PublicKey;
  let escrowPda: PublicKey;
  let p1AgentPda: PublicKey;
  let p2AgentPda: PublicKey;

  const p1Action = 0; // Attack
  const p1Power = 800;
  const p1Salt = crypto.randomBytes(16);

  const p2Action = 2; // Siphon (Loses to Attack)
  const p2Power = 700;
  const p2Salt = crypto.randomBytes(16);

  function calculateKeccak(action: number, power: number, salt: Buffer): number[] {
    const powerBuf = Buffer.alloc(2);
    powerBuf.writeUInt16LE(power, 0);
    const combined = Buffer.concat([Buffer.from([action]), powerBuf, salt]);
    return Array.from(crypto.createHash("sha3-256").update(combined).digest());
  }

  before(async () => {
    // Fund test agents
    const sig1 = await provider.connection.requestAirdrop(playerOne.publicKey, 2 * LAMPORTS_PER_SOL);
    const sig2 = await provider.connection.requestAirdrop(playerTwo.publicKey, 2 * LAMPORTS_PER_SOL);
    await provider.connection.confirmTransaction(sig1);
    await provider.connection.confirmTransaction(sig2);

    [p1AgentPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("agent"), playerOne.publicKey.toBuffer()],
      program.programId
    );

    [p2AgentPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("agent"), playerTwo.publicKey.toBuffer()],
      program.programId
    );

    [matchPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("match"), matchId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );

    [escrowPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("escrow"), matchId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );
  });

  it("1. Successfully registers Player One & Player Two agent profiles", async () => {
    await program.methods
      .initializeAgent("Aegis-One", "https://arweave.net/aegis-1")
      .accounts({
        agent: p1AgentPda,
        owner: playerOne.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([playerOne])
      .rpc();

    await program.methods
      .initializeAgent("Steve-Two", "https://arweave.net/steve-2")
      .accounts({
        agent: p2AgentPda,
        owner: playerTwo.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([playerTwo])
      .rpc();

    const p1Account = await program.account.agentProfile.fetch(p1AgentPda);
    expect(p1Account.name).to.equal("Aegis-One");
    expect(p1Account.rating).to.equal(1000);
    expect(p1Account.wins).to.equal(0);
  });

  it("2. Initializes match arena with escrow deposit", async () => {
    await program.methods
      .initializeMatch(matchId, stakeAmount)
      .accounts({
        matchArena: matchPda,
        escrowVault: escrowPda,
        playerOne: playerOne.publicKey,
        playerTwo: playerTwo.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([playerOne])
      .rpc();

    const matchAcc = await program.account.matchArena.fetch(matchPda);
    expect(matchAcc.stage).to.equal(1); // CommitPhase
    expect(matchAcc.stakeAmount.eq(stakeAmount)).to.be.true;

    const escrowBal = await provider.connection.getBalance(escrowPda);
    expect(escrowBal).to.equal(stakeAmount.toNumber());
  });

  it("3. Commits hashed actions for both agents", async () => {
    const p1Hash = calculateKeccak(p1Action, p1Power, p1Salt);
    const p2Hash = calculateKeccak(p2Action, p2Power, p2Salt);

    await program.methods
      .commitAction(p1Hash)
      .accounts({
        matchArena: matchPda,
        signer: playerOne.publicKey,
      })
      .signers([playerOne])
      .rpc();

    await program.methods
      .commitAction(p2Hash)
      .accounts({
        matchArena: matchPda,
        signer: playerTwo.publicKey,
      })
      .signers([playerTwo])
      .rpc();

    const matchAcc = await program.account.matchArena.fetch(matchPda);
    expect(matchAcc.stage).to.equal(2); // Automatically advanced to RevealPhase
  });

  it("4. Prevents front-running cheat attempts (Mismatched Reveal Hash)", async () => {
    const forgedSalt = crypto.randomBytes(16);
    try {
      await program.methods
        .revealAction(p1Action, p1Power, Array.from(forgedSalt))
        .accounts({
          matchArena: matchPda,
          signer: playerOne.publicKey,
        })
        .signers([playerOne])
        .rpc();
      expect.fail("Should have failed due to commitment mismatch");
    } catch (err: any) {
      expect(err.toString()).to.include("CommitmentMismatch");
    }
  });

  it("5. Successfully reveals moves with valid pre-images", async () => {
    await program.methods
      .revealAction(p1Action, p1Power, Array.from(p1Salt))
      .accounts({
        matchArena: matchPda,
        signer: playerOne.publicKey,
      })
      .signers([playerOne])
      .rpc();

    await program.methods
      .revealAction(p2Action, p2Power, Array.from(p2Salt))
      .accounts({
        matchArena: matchPda,
        signer: playerTwo.publicKey,
      })
      .signers([playerTwo])
      .rpc();

    const matchAcc = await program.account.matchArena.fetch(matchPda);
    expect(matchAcc.playerOneRevealed).to.be.true;
    expect(matchAcc.playerTwoRevealed).to.be.true;
  });

  it("6. Resolves match, pays out escrow, and increments winner ELO rating", async () => {
    const p1PreBalance = await provider.connection.getBalance(playerOne.publicKey);

    await program.methods
      .resolveMatch()
      .accounts({
        matchArena: matchPda,
        escrowVault: escrowPda,
        profileOne: p1AgentPda,
        profileTwo: p2AgentPda,
        winnerAccount: playerOne.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .rpc();

    const matchAcc = await program.account.matchArena.fetch(matchPda);
    expect(matchAcc.stage).to.equal(3); // Resolved
    expect(matchAcc.winner.toBase58()).to.equal(playerOne.publicKey.toBase58());

    const p1PostBalance = await provider.connection.getBalance(playerOne.publicKey);
    expect(p1PostBalance).to.be.greaterThan(p1PreBalance);

    const p1Profile = await program.account.agentProfile.fetch(p1AgentPda);
    expect(p1Profile.wins).to.equal(1);
    expect(p1Profile.rating).to.equal(1025);
  });
});