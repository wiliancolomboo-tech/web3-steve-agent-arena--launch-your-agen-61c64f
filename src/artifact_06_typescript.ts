import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { SteveAgentArena } from "../target/types/steve_agent_arena";
import {
  createMint,
  createAccount,
  mintTo,
  TOKEN_PROGRAM_ID,
  getAccount,
} from "@solana/spl-token";
import { Keypair, PublicKey, SystemProgram } from "@solana/web3.js";
import { expect } from "chai";
import * as crypto from "crypto";

describe("steve_agent_arena core test matrix", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.SteveAgentArena as Program<SteveAgentArena>;

  let usdcMint: PublicKey;
  let admin = Keypair.generate();
  let player1 = Keypair.generate();
  let player2 = Keypair.generate();
  let treasury = Keypair.generate();

  let player1Token: PublicKey;
  let player2Token: PublicKey;
  let treasuryToken: PublicKey;

  let arenaConfigPda: PublicKey;
  let player1ProfilePda: PublicKey;
  let player2ProfilePda: PublicKey;

  const matchId = new anchor.BN(42001);
  const stakeAmount = new anchor.BN(500_000_000); // 500 USDC (6 decimals)
  const minStake = new anchor.BN(10_000_000); // 10 USDC

  function computeCommitment(score: number, salt: Buffer): Buffer {
    const scoreBuf = Buffer.alloc(4);
    scoreBuf.writeUInt32LE(score, 0);
    const hasher = crypto.createHash("sha256");
    hasher.update(scoreBuf);
    hasher.update(salt);
    return hasher.digest();
  }

  before(async () => {
    // Airdrop SOL
    for (const kp of [admin, player1, player2]) {
      const sig = await provider.connection.requestAirdrop(kp.publicKey, 10 * anchor.web3.LAMPORTS_PER_SOL);
      await provider.connection.confirmTransaction(sig);
    }

    // Create Mock USDC Mint (6 Decimals)
    usdcMint = await createMint(
      provider.connection,
      admin,
      admin.publicKey,
      null,
      6
    );

    // Setup Token Accounts
    player1Token = await createAccount(provider.connection, player1, usdcMint, player1.publicKey);
    player2Token = await createAccount(provider.connection, player2, usdcMint, player2.publicKey);
    treasuryToken = await createAccount(provider.connection, admin, usdcMint, admin.publicKey);

    // Mint 2,000 USDC to both players
    await mintTo(provider.connection, admin, usdcMint, player1Token, admin, 2000_000_000);
    await mintTo(provider.connection, admin, usdcMint, player2Token, admin, 2000_000_000);

    // Derive PDAs
    [arenaConfigPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("arena_config")],
      program.programId
    );
    [player1ProfilePda] = PublicKey.findProgramAddressSync(
      [Buffer.from("agent_profile"), player1.publicKey.toBuffer()],
      program.programId
    );
    [player2ProfilePda] = PublicKey.findProgramAddressSync(
      [Buffer.from("agent_profile"), player2.publicKey.toBuffer()],
      program.programId
    );
  });

  it("1. Initializes the Arena Protocol Config", async () => {
    await program.methods
      .initializeArena(250, minStake, new anchor.BN(120)) // 2.5% protocol fee, 120s timeout
      .accounts({
        authority: admin.publicKey,
        usdcMint: usdcMint,
        arenaConfig: arenaConfigPda,
        systemProgram: SystemProgram.programId,
      })
      .signers([admin])
      .rpc();

    const config = await program.account.arenaConfig.fetch(arenaConfigPda);
    expect(config.authority.toBase58()).to.equal(admin.publicKey.toBase58());
    expect(config.protocolFeeBps).to.equal(250);
  });

  it("2. Registers Player 1 and Player 2 Agents", async () => {
    const stratHashP1 = Array.from(crypto.randomBytes(32));
    const stratHashP2 = Array.from(crypto.randomBytes(32));

    await program.methods
      .registerAgent("STEVE-SENTINEL-ALPHA", stratHashP1)
      .accounts({
        owner: player1.publicKey,
        agentProfile: player1ProfilePda,
        systemProgram: SystemProgram.programId,
      })
      .signers([player1])
      .rpc();

    await program.methods
      .registerAgent("STEVE-RIVAL-BETA", stratHashP2)
      .accounts({
        owner: player2.publicKey,
        agentProfile: player2ProfilePda,
        systemProgram: SystemProgram.programId,
      })
      .signers([player2])
      .rpc();

    const p1 = await program.account.agentProfile.fetch(player1ProfilePda);
    expect(p1.eloRating).to.equal(1200);
    expect(p1.agentName).to.equal("STEVE-SENTINEL-ALPHA");
  });

  it("3. Creates Match and Locks Escrow Collateral (500 USDC)", async () => {
    const [matchPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("match_escrow"), matchId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );
    const [vaultPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("vault_escrow"), matchId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );

    const salt1 = crypto.randomBytes(16);
    const commitmentP1 = computeCommitment(88, salt1);

    await program.methods
      .createMatch(matchId, stakeAmount, Array.from(commitmentP1))
      .accounts({
        player1: player1.publicKey,
        arenaConfig: arenaConfigPda,
        player1Profile: player1ProfilePda,
        arenaMatch: matchPda,
        vaultTokenAccount: vaultPda,
        player1TokenAccount: player1Token,
        usdcMint: usdcMint,
        tokenProgram: TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
        rent: anchor.web3.SYSVAR_RENT_PUBKEY,
      })
      .signers([player1])
      .rpc();

    const matchAcc = await program.account.arenaMatch.fetch(matchPda);
    expect(matchAcc.totalPot.toNumber()).to.equal(500_000_000);

    const vaultBalance = await getAccount(provider.connection, vaultPda);
    expect(Number(vaultBalance.amount)).to.equal(500_000_000);
  });

  it("4. Prevents an agent from battling itself (Self-Battle Exploit)", async () => {
    const [matchPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("match_escrow"), matchId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );
    const [vaultPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("vault_escrow"), matchId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );

    try {
      await program.methods
        .joinMatch(matchId, Array.from(crypto.randomBytes(32)))
        .accounts({
          player2: player1.publicKey, // Attempting self play
          arenaConfig: arenaConfigPda,
          player2Profile: player1ProfilePda,
          arenaMatch: matchPda,
          vaultTokenAccount: vaultPda,
          player2TokenAccount: player1Token,
          usdcMint: usdcMint,
          tokenProgram: TOKEN_PROGRAM_ID,
        })
        .signers([player1])
        .rpc();
      expect.fail("Expected CannotBattleSelf error");
    } catch (err: any) {
      expect(err.error.errorCode.code).to.equal("CannotBattleSelf");
    }
  });

  it("5. Player 2 Joins Match and Verifies Escrow Balance Doubles", async () => {
    const [matchPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("match_escrow"), matchId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );
    const [vaultPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("vault_escrow"), matchId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );

    const salt2 = crypto.randomBytes(16);
    const commitmentP2 = computeCommitment(45, salt2);

    await program.methods
      .joinMatch(matchId, Array.from(commitmentP2))
      .accounts({
        player2: player2.publicKey,
        arenaConfig: arenaConfigPda,
        player2Profile: player2ProfilePda,
        arenaMatch: matchPda,
        vaultTokenAccount: vaultPda,
        player2TokenAccount: player2Token,
        usdcMint: usdcMint,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .signers([player2])
      .rpc();

    const vaultBalance = await getAccount(provider.connection, vaultPda);
    expect(Number(vaultBalance.amount)).to.equal(1_000_000_000); // 1,000 USDC total pot
  });

  it("6. Rejects settlement if forged salt/score does not match on-chain commitment", async () => {
    const [matchPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("match_escrow"), matchId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );
    const [vaultPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("vault_escrow"), matchId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );

    const fakeSalt = Array.from(crypto.randomBytes(16));

    try {
      await program.methods
        .settleMatch(matchId, 999, 45, fakeSalt, fakeSalt)
        .accounts({
          referee: admin.publicKey,
          arenaConfig: arenaConfigPda,
          arenaMatch: matchPda,
          profile1: player1ProfilePda,
          profile2: player2ProfilePda,
          vaultTokenAccount: vaultPda,
          player1TokenAccount: player1Token,
          player2TokenAccount: player2Token,
          protocolTreasuryToken: treasuryToken,
          usdcMint: usdcMint,
          tokenProgram: TOKEN_PROGRAM_ID,
        })
        .signers([admin])
        .rpc();
      expect.fail("Expected commitment preimage mismatch error");
    } catch (err: any) {
      expect(err.error.errorCode.code).to.equal("InvalidProofCommitmentP1");
    }
  });

  it("7. Prevents premature timeout slashing while match is active and unexpired", async () => {
    const [matchPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("match_escrow"), matchId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );
    const [vaultPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("vault_escrow"), matchId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );

    try {
      await program.methods
        .handleTimeoutSlash(matchId)
        .accounts({
          caller: player1.publicKey,
          arenaMatch: matchPda,
          vaultTokenAccount: vaultPda,
          player1TokenAccount: player1Token,
          player2TokenAccount: player2Token,
          usdcMint: usdcMint,
          tokenProgram: TOKEN_PROGRAM_ID,
        })
        .signers([player1])
        .rpc();
      expect.fail("Expected MatchNotExpired error");
    } catch (err: any) {
      expect(err.error.errorCode.code).to.equal("MatchNotExpired");
    }
  });
});