import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { SteveAgentArena } from "../target/types/steve_agent_arena";
import {
  createMint,
  createAccount,
  mintTo,
  getAccount,
  TOKEN_PROGRAM_ID,
} from "@solana/spl-token";
import { Keypair, PublicKey, SystemProgram, SYSVAR_RENT_PUBKEY } from "@solana/web3.js";
import { expect } from "chai";
import { createHash } from "crypto";

describe("Steve Agent Arena Full Test Suite", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.SteveAgentArena as Program<SteveAgentArena>;

  const authority = Keypair.generate();
  const treasury = Keypair.generate();
  const playerA = Keypair.generate();
  const playerB = Keypair.generate();

  let mint: PublicKey;
  let treasuryToken: PublicKey;
  let playerAToken: PublicKey;
  let playerBToken: PublicKey;
  let arenaConfigPda: PublicKey;

  const matchId = new anchor.BN(9941);
  const wagerAmount = new anchor.BN(50_000_000); // 50 USDC (6 decimals)
  const maxTurns = 2;

  before(async () => {
    // Fund test wallets with SOL
    for (const kp of [authority, treasury, playerA, playerB]) {
      const sig = await provider.connection.requestAirdrop(kp.publicKey, 10 * anchor.web3.LAMPORTS_PER_SOL);
      await provider.connection.confirmTransaction(sig);
    }

    // Create Mock USDC Mint
    mint = await createMint(provider.connection, authority, authority.publicKey, null, 6);

    // Create Token Accounts
    treasuryToken = await createAccount(provider.connection, authority, mint, treasury.publicKey);
    playerAToken = await createAccount(provider.connection, playerA, mint, playerA.publicKey);
    playerBToken = await createAccount(provider.connection, playerB, mint, playerB.publicKey);

    // Mint USDC to Players
    await mintTo(provider.connection, authority, mint, playerAToken, authority, 500_000_000);
    await mintTo(provider.connection, authority, mint, playerBToken, authority, 500_000_000);

    [arenaConfigPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("arena_config")],
      program.programId
    );
  });

  it("1. Initializes the Arena with valid protocol fee parameters", async () => {
    await program.methods
      .initializeArena(new anchor.BN(250)) // 2.5% fee
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
    expect(config.protocolFeeBps.toNumber()).to.equal(250);
  });

  it("2. Creates a match and escrows Player A's wager", async () => {
    const [matchPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("match"), matchId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );
    const [escrowVault] = PublicKey.findProgramAddressSync(
      [Buffer.from("escrow"), matchId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );

    const balanceBefore = (await getAccount(provider.connection, playerAToken)).amount;

    await program.methods
      .createMatch(matchId, wagerAmount, maxTurns)
      .accounts({
        matchAccount: matchPda,
        arenaConfig: arenaConfigPda,
        escrowVault: escrowVault,
        tokenMint: mint,
        playerAToken: playerAToken,
        playerA: playerA.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
        rent: SYSVAR_RENT_PUBKEY,
      })
      .signers([playerA])
      .rpc();

    const matchState = await program.account.matchAccount.fetch(matchPda);
    expect(matchState.matchId.toNumber()).to.equal(matchId.toNumber());
    expect(matchState.playerA.toBase58()).to.equal(playerA.publicKey.toBase58());
    expect(matchState.wagerAmount.toNumber()).to.equal(wagerAmount.toNumber());

    const vaultAccount = await getAccount(provider.connection, escrowVault);
    expect(vaultAccount.amount).to.equal(BigInt(wagerAmount.toString()));

    const balanceAfter = (await getAccount(provider.connection, playerAToken)).amount;
    expect(balanceBefore - balanceAfter).to.equal(BigInt(wagerAmount.toString()));
  });

  it("3. Player B joins match and escrows matching collateral", async () => {
    const [matchPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("match"), matchId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );
    const [escrowVault] = PublicKey.findProgramAddressSync(
      [Buffer.from("escrow"), matchId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );

    await program.methods
      .joinMatch()
      .accounts({
        matchAccount: matchPda,
        escrowVault: escrowVault,
        playerBToken: playerBToken,
        playerB: playerB.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .signers([playerB])
      .rpc();

    const matchState = await program.account.matchAccount.fetch(matchPda);
    expect(matchState.playerB.toBase58()).to.equal(playerB.publicKey.toBase58());

    const vaultAccount = await getAccount(provider.connection, escrowVault);
    expect(vaultAccount.amount).to.equal(BigInt(wagerAmount.toNumber() * 2));
  });

  it("4. Rejects out-of-turn execution attempts", async () => {
    const [matchPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("match"), matchId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );
    const dummyHash = Array.from(createHash("sha256").update("invalid").digest());

    try {
      // Turn 0 belongs to Player A. Calling as Player B must fail!
      await program.methods
        .submitMove(dummyHash, 45)
        .accounts({
          matchAccount: matchPda,
          player: playerB.publicKey,
        })
        .signers([playerB])
        .rpc();
      expect.fail("Should have thrown NotYourTurn error");
    } catch (err: any) {
      expect(err.error.errorCode.code).to.equal("NotYourTurn");
    }
  });

  it("5. Completes alternating rounds and determines Player A as winner", async () => {
    const [matchPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("match"), matchId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );

    const hash1 = Array.from(createHash("sha256").update("turn_1").digest());
    const hash2 = Array.from(createHash("sha256").update("turn_2").digest());

    // Turn 1: Player A scores 90
    await program.methods
      .submitMove(hash1, 90)
      .accounts({ matchAccount: matchPda, player: playerA.publicKey })
      .signers([playerA])
      .rpc();

    // Turn 2: Player B scores 75 (Reaches maxTurns = 2)
    await program.methods
      .submitMove(hash2, 75)
      .accounts({ matchAccount: matchPda, player: playerB.publicKey })
      .signers([playerB])
      .rpc();

    const matchState = await program.account.matchAccount.fetch(matchPda);
    expect(matchState.winner.toBase58()).to.equal(playerA.publicKey.toBase58());
  });

  it("6. Settles payout atomically with protocol fee deduction", async () => {
    const [matchPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("match"), matchId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );
    const [escrowVault] = PublicKey.findProgramAddressSync(
      [Buffer.from("escrow"), matchId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );

    const playerABalanceBefore = (await getAccount(provider.connection, playerAToken)).amount;
    const treasuryBalanceBefore = (await getAccount(provider.connection, treasuryToken)).amount;

    await program.methods
      .settlePayout()
      .accounts({
        arenaConfig: arenaConfigPda,
        matchAccount: matchPda,
        escrowVault: escrowVault,
        treasuryToken: treasuryToken,
        winnerToken: playerAToken,
        playerAToken: playerAToken,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .rpc();

    const playerABalanceAfter = (await getAccount(provider.connection, playerAToken)).amount;
    const treasuryBalanceAfter = (await getAccount(provider.connection, treasuryToken)).amount;

    // Total pot: 100 USDC. Protocol fee: 2.5% = 2.5 USDC. Payout = 97.5 USDC
    const expectedFee = BigInt(2_500_000);
    const expectedPayout = BigInt(97_500_000);

    expect(treasuryBalanceAfter - treasuryBalanceBefore).to.equal(expectedFee);
    expect(playerABalanceAfter - playerABalanceBefore).to.equal(expectedPayout);

    const vaultAccount = await getAccount(provider.connection, escrowVault);
    expect(vaultAccount.amount).to.equal(BigInt(0));
  });
});