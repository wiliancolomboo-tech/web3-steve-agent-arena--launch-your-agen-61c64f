import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { VanguardArena } from "../target/types/vanguard_arena";
import {
  TOKEN_PROGRAM_ID,
  createMint,
  createAccount,
  mintTo,
  getAccount,
} from "@solana/spl-token";
import { assert, expect } from "chai";
import * as crypto from "crypto";

describe("vanguard_arena integration tests", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.VanguardArena as Program<VanguardArena>;

  let mint: anchor.web3.PublicKey;
  let initiatorTokenAccount: anchor.web3.PublicKey;
  let opponentTokenAccount: anchor.web3.PublicKey;

  const initiator = anchor.web3.Keypair.generate();
  const opponent = anchor.web3.Keypair.generate();
  const matchId = new anchor.BN(Math.floor(Math.random() * 1000000));
  const stakeAmount = new anchor.BN(1_000_000); // 1 Token with 6 decimals

  // Helper to calculate action commitments
  function makeCommitment(secret: bigint, action: number) {
    const secretBuf = Buffer.alloc(8);
    secretBuf.writeBigUInt64LE(secret);
    const actionBuf = Buffer.from([action]);
    const preimage = Buffer.concat([secretBuf, actionBuf]);
    return crypto.createHash("sha256").update(preimage).digest();
  }

  const initSecret = BigInt("0x123456789abcdef0");
  const initAction = 0; // Attack
  const initHash = makeCommitment(initSecret, initAction);

  const oppSecret = BigInt("0x0fedcba987654321");
  const oppAction = 2; // Counter (0 beats 2 => Initiator should win!)
  const oppHash = makeCommitment(oppSecret, oppAction);

  before(async () => {
    // Airdrop SOL to test participants
    const airdrop1 = await provider.connection.requestAirdrop(initiator.publicKey, 2 * anchor.web3.LAMPORTS_PER_SOL);
    const airdrop2 = await provider.connection.requestAirdrop(opponent.publicKey, 2 * anchor.web3.LAMPORTS_PER_SOL);
    await provider.connection.confirmTransaction(airdrop1);
    await provider.connection.confirmTransaction(airdrop2);

    // Setup SPL Token Mint
    mint = await createMint(
      provider.connection,
      (provider.wallet as any).payer,
      provider.wallet.publicKey,
      null,
      6
    );

    initiatorTokenAccount = await createAccount(
      provider.connection,
      (provider.wallet as any).payer,
      mint,
      initiator.publicKey
    );

    opponentTokenAccount = await createAccount(
      provider.connection,
      (provider.wallet as any).payer,
      mint,
      opponent.publicKey
    );

    await mintTo(provider.connection, (provider.wallet as any).payer, mint, initiatorTokenAccount, provider.wallet.publicKey, 10_000_000);
    await mintTo(provider.connection, (provider.wallet as any).payer, mint, opponentTokenAccount, provider.wallet.publicKey, 10_000_000);
  });

  it("Registers Vanguard Agent Profile successfully", async () => {
    const agentId = "VANGUARD-001";
    const [agentPda] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("agent"), initiator.publicKey.toBuffer(), Buffer.from(agentId)],
      program.programId
    );

    await program.methods
      .registerAgent(agentId, "https://vanguard.ai/metadata.json")
      .accounts({
        agentProfile: agentPda,
        authority: initiator.publicKey,
        systemProgram: anchor.web3.SystemProgram.programId,
      })
      .signers([initiator])
      .rpc();

    const profile = await program.account.agentProfile.fetch(agentPda);
    assert.equal(profile.agentId, agentId);
    assert.equal(profile.eloRating, 1000);
    assert.isTrue(profile.isActive);
  });

  it("Registers Opponent Agent Profile successfully", async () => {
    const agentId = "CHALLENGER-002";
    const [oppPda] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("agent"), opponent.publicKey.toBuffer(), Buffer.from(agentId)],
      program.programId
    );

    await program.methods
      .registerAgent(agentId, "https://challenger.ai/metadata.json")
      .accounts({
        agentProfile: oppPda,
        authority: opponent.publicKey,
        systemProgram: anchor.web3.SystemProgram.programId,
      })
      .signers([opponent])
      .rpc();

    const profile = await program.account.agentProfile.fetch(oppPda);
    assert.equal(profile.agentId, agentId);
  });

  it("Initiates a match and locks stake in vault", async () => {
    const [matchStatePda] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("match_state"), matchId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );

    const [matchVaultPda] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("match_vault"), matchId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );

    await program.methods
      .initiateMatch(matchId, stakeAmount, Array.from(initHash))
      .accounts({
        matchState: matchStatePda,
        matchVault: matchVaultPda,
        tokenMint: mint,
        initiatorTokenAccount: initiatorTokenAccount,
        initiator: initiator.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
        systemProgram: anchor.web3.SystemProgram.programId,
        rent: anchor.web3.SYSVAR_RENT_PUBKEY,
      })
      .signers([initiator])
      .rpc();

    const vaultAccount = await getAccount(provider.connection, matchVaultPda);
    assert.equal(vaultAccount.amount.toString(), stakeAmount.toString());

    const state = await program.account.matchState.fetch(matchStatePda);
    assert.deepEqual(state.status, { initialized: {} });
  });

  it("Opponent accepts the challenge and deposits matching stake", async () => {
    const [matchStatePda] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("match_state"), matchId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );

    const [matchVaultPda] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("match_vault"), matchId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );

    await program.methods
      .acceptMatch(Array.from(oppHash))
      .accounts({
        matchState: matchStatePda,
        matchVault: matchVaultPda,
        opponentTokenAccount: opponentTokenAccount,
        opponent: opponent.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .signers([opponent])
      .rpc();

    const vaultAccount = await getAccount(provider.connection, matchVaultPda);
    assert.equal(vaultAccount.amount.toString(), stakeAmount.mul(new anchor.BN(2)).toString());

    const state = await program.account.matchState.fetch(matchStatePda);
    assert.deepEqual(state.status, { active: {} });
  });

  it("Settles the match deterministically and credits the winner", async () => {
    const [matchStatePda] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("match_state"), matchId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );

    const [matchVaultPda] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("match_vault"), matchId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );

    const [initProfilePda] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("agent"), initiator.publicKey.toBuffer(), Buffer.from("VANGUARD-001")],
      program.programId
    );

    const [oppProfilePda] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("agent"), opponent.publicKey.toBuffer(), Buffer.from("CHALLENGER-002")],
      program.programId
    );

    const initialWinnerBal = (await getAccount(provider.connection, initiatorTokenAccount)).amount;

    await program.methods
      .settleMatch(
        new anchor.BN(initSecret.toString()),
        initAction,
        new anchor.BN(oppSecret.toString()),
        oppAction
      )
      .accounts({
        matchState: matchStatePda,
        matchVault: matchVaultPda,
        initiatorProfile: initProfilePda,
        opponentProfile: oppProfilePda,
        initiatorTokenAccount: initiatorTokenAccount,
        opponentTokenAccount: opponentTokenAccount,
        settler: initiator.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
        systemProgram: anchor.web3.SystemProgram.programId,
      })
      .signers([initiator])
      .rpc();

    const finalWinnerBal = (await getAccount(provider.connection, initiatorTokenAccount)).amount;
    const expectedAddition = stakeAmount.mul(new anchor.BN(2));
    assert.equal(
      BigInt(finalWinnerBal.toString()) - BigInt(initialWinnerBal.toString()),
      BigInt(expectedAddition.toString()),
      "Initiator should have received the entire escrow pot"
    );

    const updatedProfile = await program.account.agentProfile.fetch(initProfilePda);
    assert.equal(updatedProfile.matchesWon.toNumber(), 1);
    assert.equal(updatedProfile.eloRating, 1025);
  });
});