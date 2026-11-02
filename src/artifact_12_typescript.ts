import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { PublicKey, Keypair, SystemProgram } from "@solana/web3.js";
import {
  TOKEN_PROGRAM_ID,
  createMint,
  createAccount,
  mintTo,
  getAccount,
} from "@solana/spl-token";
import { expect } from "chai";
import * as crypto from "crypto";

describe("steve_arena: Autonomous Agent Arena Verification", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  // The local program under test
  const program = anchor.workspace.SteveArena as Program<any>;

  let usdcMint: PublicKey;
  let initiatorTokenAccount: PublicKey;
  let opponentTokenAccount: PublicKey;

  const initiator = Keypair.generate();
  const opponent = Keypair.generate();

  const STAKE_AMOUNT = new anchor.BN(250_000_000); // 250 USDC (6 decimals)
  const MATCH_ID = new anchor.BN(1001);

  const MATCH_SEED = Buffer.from("steve_match");
  const VAULT_SEED = Buffer.from("steve_vault");

  before(async () => {
    // Airdrop SOL to test agents for fees
    const airdrop1 = await provider.connection.requestAirdrop(initiator.publicKey, 2e9);
    const airdrop2 = await provider.connection.requestAirdrop(opponent.publicKey, 2e9);
    await provider.connection.confirmTransaction(airdrop1);
    await provider.connection.confirmTransaction(airdrop2);

    // Create mock USDC mint
    usdcMint = await createMint(
      provider.connection,
      (provider.wallet as any).payer,
      provider.wallet.publicKey,
      null,
      6
    );

    // Create token accounts
    initiatorTokenAccount = await createAccount(
      provider.connection,
      initiator,
      usdcMint,
      initiator.publicKey
    );

    opponentTokenAccount = await createAccount(
      provider.connection,
      opponent,
      usdcMint,
      opponent.publicKey
    );

    // Fund both agents with 1000 USDC each
    await mintTo(provider.connection, (provider.wallet as any).payer, usdcMint, initiatorTokenAccount, provider.wallet.publicKey, 1_000_000_000);
    await mintTo(provider.connection, (provider.wallet as any).payer, usdcMint, opponentTokenAccount, provider.wallet.publicKey, 1_000_000_000);
  });

  it("1. Initializes Arena Match and Escrows Stake correctly", async () => {
    const matchIdBuf = Buffer.alloc(8);
    matchIdBuf.writeBigUInt64LE(BigInt(MATCH_ID.toString()));

    const [matchPda] = PublicKey.findProgramAddressSync([MATCH_SEED, matchIdBuf], program.programId);
    const [vaultPda] = PublicKey.findProgramAddressSync([VAULT_SEED, matchIdBuf], program.programId);

    await program.methods
      .initializeMatch(MATCH_ID, STAKE_AMOUNT)
      .accounts({
        matchState: matchPda,
        vaultAccount: vaultPda,
        usdcMint: usdcMint,
        initiatorTokenAccount: initiatorTokenAccount,
        initiator: initiator.publicKey,
        opponent: opponent.publicKey,
        systemProgram: SystemProgram.programId,
        tokenProgram: TOKEN_PROGRAM_ID,
        rent: anchor.web3.SYSVAR_RENT_PUBKEY,
      })
      .signers([initiator])
      .rpc();

    const vaultBalance = await getAccount(provider.connection, vaultPda);
    expect(Number(vaultBalance.amount)).to.equal(STAKE_AMOUNT.toNumber());

    const matchData = await program.account.matchState.fetch(matchPda);
    expect(matchData.initiator.toBase58()).to.equal(initiator.publicKey.toBase58());
    expect(matchData.opponent.toBase58()).to.equal(opponent.publicKey.toBase58());
  });

  it("2. Opponent Agent joins and deposits matching stake", async () => {
    const matchIdBuf = Buffer.alloc(8);
    matchIdBuf.writeBigUInt64LE(BigInt(MATCH_ID.toString()));

    const [matchPda] = PublicKey.findProgramAddressSync([MATCH_SEED, matchIdBuf], program.programId);
    const [vaultPda] = PublicKey.findProgramAddressSync([VAULT_SEED, matchIdBuf], program.programId);

    await program.methods
      .joinMatch()
      .accounts({
        matchState: matchPda,
        vaultAccount: vaultPda,
        opponentTokenAccount: opponentTokenAccount,
        opponent: opponent.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .signers([opponent])
      .rpc();

    const vaultBalance = await getAccount(provider.connection, vaultPda);
    expect(Number(vaultBalance.amount)).to.equal(STAKE_AMOUNT.toNumber() * 2);
  });

  it("3. Anti-Frontrunning Commit Phase for both autonomous agents", async () => {
    const matchIdBuf = Buffer.alloc(8);
    matchIdBuf.writeBigUInt64LE(BigInt(MATCH_ID.toString()));
    const [matchPda] = PublicKey.findProgramAddressSync([MATCH_SEED, matchIdBuf], program.programId);

    // Initiator selects Action 1 (Aggressive Strike)
    const salt1 = crypto.randomBytes(32);
    const hash1 = crypto.createHash("sha256").update(Buffer.concat([Buffer.from([1]), salt1])).digest();

    // Opponent selects Action 3 (Adaptive Disruption) -> 1 beats 3!
    const salt2 = crypto.randomBytes(32);
    const hash2 = crypto.createHash("sha256").update(Buffer.concat([Buffer.from([3]), salt2])).digest();

    await program.methods
      .commitAction(Array.from(hash1))
      .accounts({ matchState: matchPda, agent: initiator.publicKey })
      .signers([initiator])
      .rpc();

    await program.methods
      .commitAction(Array.from(hash2))
      .accounts({ matchState: matchPda, agent: opponent.publicKey })
      .signers([opponent])
      .rpc();

    // Reveal Phase
    await program.methods
      .revealAction(1, Array.from(salt1))
      .accounts({ matchState: matchPda, agent: initiator.publicKey })
      .signers([initiator])
      .rpc();

    await program.methods
      .revealAction(3, Array.from(salt2))
      .accounts({ matchState: matchPda, agent: opponent.publicKey })
      .signers([opponent])
      .rpc();

    const resolvedState = await program.account.matchState.fetch(matchPda);
    // Initiator Action 1 beats Opponent Action 3
    expect(resolvedState.winner.toBase58()).to.equal(initiator.publicKey.toBase58());
  });

  it("4. Settles total 500 USDC prize to winner token account", async () => {
    const matchIdBuf = Buffer.alloc(8);
    matchIdBuf.writeBigUInt64LE(BigInt(MATCH_ID.toString()));
    const [matchPda] = PublicKey.findProgramAddressSync([MATCH_SEED, matchIdBuf], program.programId);
    const [vaultPda] = PublicKey.findProgramAddressSync([VAULT_SEED, matchIdBuf], program.programId);

    const initBalBefore = await getAccount(provider.connection, initiatorTokenAccount);

    await program.methods
      .settleMatch()
      .accounts({
        matchState: matchPda,
        vaultAccount: vaultPda,
        winnerTokenAccount: initiatorTokenAccount,
        initiatorTokenAccount: initiatorTokenAccount,
        opponentTokenAccount: opponentTokenAccount,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .rpc();

    const initBalAfter = await getAccount(provider.connection, initiatorTokenAccount);
    // 500 USDC added
    expect(Number(initBalAfter.amount) - Number(initBalBefore.amount)).to.equal(500_000_000);
  });
});