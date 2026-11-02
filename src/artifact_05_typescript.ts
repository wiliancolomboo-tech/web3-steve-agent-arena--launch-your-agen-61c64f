import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { SteveAgentArena } from "../target/types/steve_agent_arena";
import { Keypair, PublicKey, SystemProgram, LAMPORTS_PER_SOL } from "@solana/web3.js";
import { expect } from "chai";
import { createHash, randomBytes } from "crypto";

describe("steve-agent-arena test suite", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.SteveAgentArena as Program<SteveAgentArena>;

  const authority = Keypair.generate();
  const treasury = Keypair.generate();
  const player1 = Keypair.generate();
  const player2 = Keypair.generate();

  let arenaConfigPda: PublicKey;

  before(async () => {
    // Fund all wallets
    for (const kp of [authority, treasury, player1, player2]) {
      const sig = await provider.connection.requestAirdrop(kp.publicKey, 5 * LAMPORTS_PER_SOL);
      const latestBlockhash = await provider.connection.getLatestBlockhash();
      await provider.connection.confirmTransaction({
        signature: sig,
        ...latestBlockhash,
      });
    }

    [arenaConfigPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("arena_config")],
      program.programId
    );
  });

  it("1. Initializes the Arena global state with 2.5% protocol fee", async () => {
    await program.methods
      .initializeArena(250) // 250 bps = 2.5%
      .accounts({
        arenaConfig: arenaConfigPda,
        authority: authority.publicKey,
        treasury: treasury.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([authority])
      .rpc();

    const config = await program.account.arenaConfig.fetch(arenaConfigPda);
    expect(config.authority.toBase58()).to.equal(authority.publicKey.toBase58());
    expect(config.treasury.toBase58()).to.equal(treasury.publicKey.toBase58());
    expect(config.protocolFeeBps).to.equal(250);
  });

  it("2. Registers an Autonomous Steve Agent Profile", async () => {
    const agentId = "steve-titan-01";
    const [agentPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("agent_profile"), player1.publicKey.as_ref(), Buffer.from(agentId)],
      program.programId
    );

    await program.methods
      .registerAgent(agentId, "https://ipfs.io/ipfs/QmSteveMetadata")
      .accounts({
        agentProfile: agentPda,
        owner: player1.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([player1])
      .rpc();

    const profile = await program.account.agentProfile.fetch(agentPda);
    expect(profile.agentId).to.equal(agentId);
    expect(profile.eloRating).to.equal(1200);
    expect(profile.owner.toBase58()).to.equal(player1.publicKey.toBase58());
  });

  it("3. Executes Complete Match Lifecycle: Create, Join, Commit, Reveal, & Payout Settlement", async () => {
    const matchId = new anchor.BN(88201);
    const wager = new anchor.BN(1 * LAMPORTS_PER_SOL);
    const roundDurationSlots = new anchor.BN(50);

    const matchIdBuffer = Buffer.alloc(8);
    matchIdBuffer.writeBigUInt64LE(BigInt(88201));
    const [matchPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("arena_match"), matchIdBuffer],
      program.programId
    );

    // A. Player 1 creates match
    await program.methods
      .createMatch(matchId, wager, roundDurationSlots)
      .accounts({
        matchState: matchPda,
        arenaConfig: arenaConfigPda,
        playerOne: player1.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([player1])
      .rpc();

    let match = await program.account.matchState.fetch(matchPda);
    expect(match.wagerAmount.toNumber()).to.equal(wager.toNumber());

    // B. Player 2 joins match
    await program.methods
      .joinMatch()
      .accounts({
        matchState: matchPda,
        arenaConfig: arenaConfigPda,
        playerTwo: player2.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([player2])
      .rpc();

    // C. Generate Commitments
    // Player 1 picks STRIKE (1), Player 2 picks BLOCK (2) -> Block beats Strike: Player 2 wins
    const salt1 = randomBytes(16);
    const hash1 = createHash("sha256").update(Buffer.from([1])).update(salt1).digest();

    const salt2 = randomBytes(16);
    const hash2 = createHash("sha256").update(Buffer.from([2])).update(salt2).digest();

    // Commit moves
    await program.methods
      .commitMove(Array.from(hash1))
      .accounts({
        matchState: matchPda,
        player: player1.publicKey,
      })
      .signers([player1])
      .rpc();

    await program.methods
      .commitMove(Array.from(hash2))
      .accounts({
        matchState: matchPda,
        player: player2.publicKey,
      })
      .signers([player2])
      .rpc();

    // D. Reveal Moves
    await program.methods
      .revealMove(1, Array.from(salt1))
      .accounts({
        matchState: matchPda,
        arenaConfig: arenaConfigPda,
        player: player1.publicKey,
        playerOneAccount: player1.publicKey,
        playerTwoAccount: player2.publicKey,
        treasury: treasury.publicKey,
      })
      .signers([player1])
      .rpc();

    const p2BalanceBefore = await provider.connection.getBalance(player2.publicKey);

    await program.methods
      .revealMove(2, Array.from(salt2))
      .accounts({
        matchState: matchPda,
        arenaConfig: arenaConfigPda,
        player: player2.publicKey,
        playerOneAccount: player1.publicKey,
        playerTwoAccount: player2.publicKey,
        treasury: treasury.publicKey,
      })
      .signers([player2])
      .rpc();

    const p2BalanceAfter = await provider.connection.getBalance(player2.publicKey);
    const resolvedMatch = await program.account.matchState.fetch(matchPda);

    expect(resolvedMatch.winner.toBase58()).to.equal(player2.publicKey.toBase58());
    expect(p2BalanceAfter).to.be.greaterThan(p2BalanceBefore);
  });

  it("4. Rejects Tampered Reveal Hashes (Anti-Cheating Guard)", async () => {
    const matchId = new anchor.BN(99402);
    const wager = new anchor.BN(0.5 * LAMPORTS_PER_SOL);
    const matchIdBuffer = Buffer.alloc(8);
    matchIdBuffer.writeBigUInt64LE(BigInt(99402));

    const [matchPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("arena_match"), matchIdBuffer],
      program.programId
    );

    await program.methods
      .createMatch(matchId, wager, new anchor.BN(100))
      .accounts({
        matchState: matchPda,
        arenaConfig: arenaConfigPda,
        playerOne: player1.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([player1])
      .rpc();

    await program.methods
      .joinMatch()
      .accounts({
        matchState: matchPda,
        arenaConfig: arenaConfigPda,
        playerTwo: player2.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([player2])
      .rpc();

    const salt = randomBytes(16);
    const legitHash = createHash("sha256").update(Buffer.from([1])).update(salt).digest();

    await program.methods
      .commitMove(Array.from(legitHash))
      .accounts({ matchState: matchPda, player: player1.publicKey })
      .signers([player1])
      .rpc();

    await program.methods
      .commitMove(Array.from(legitHash))
      .accounts({ matchState: matchPda, player: player2.publicKey })
      .signers([player2])
      .rpc();

    // Adversary attempts revealing Move 2 while having committed Move 1
    try {
      await program.methods
        .revealMove(2, Array.from(salt))
        .accounts({
          matchState: matchPda,
          arenaConfig: arenaConfigPda,
          player: player1.publicKey,
          playerOneAccount: player1.publicKey,
          playerTwoAccount: player2.publicKey,
          treasury: treasury.publicKey,
        })
        .signers([player1])
        .rpc();
      expect.fail("Expected transaction to fail due to hash mismatch");
    } catch (err: any) {
      expect(err.error.errorCode.code).to.equal("InvalidRevealHash");
    }
  });
});