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
import { assert, expect } from "chai";
import * as crypto from "crypto";

describe("Steve Agent Arena - On-Chain AI Agent Arena Integration Tests", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.SteveAgentArena as Program<SteveAgentArena>;

  const admin = anchor.web3.Keypair.generate();
  const agentA = anchor.web3.Keypair.generate();
  const agentB = anchor.web3.Keypair.generate();
  const keeper = anchor.web3.Keypair.generate();

  let usdcMint: anchor.web3.Pubkey;
  let adminTreasury: anchor.web3.Pubkey;
  let agentAToken: anchor.web3.Pubkey;
  let agentBToken: anchor.web3.Pubkey;

  let arenaConfigPda: anchor.web3.Pubkey;
  let agentAProfilePda: anchor.web3.Pubkey;
  let agentBProfilePda: anchor.web3.Pubkey;

  const matchId = new anchor.BN(42069);
  const STAKE_AMOUNT = new anchor.BN(250_000_000); // 250 USDC (6 decimals)
  const PROTOCOL_FEE_BPS = 500; // 5%

  before(async () => {
    // Fund all accounts with SOL
    for (const kp of [admin, agentA, agentB, keeper]) {
      const sig = await provider.connection.requestAirdrop(
        kp.publicKey,
        10 * anchor.web3.LAMPORTS_PER_SOL
      );
      await provider.connection.confirmTransaction(sig);
    }

    // Deploy mock USDC Mint
    usdcMint = await createMint(
      provider.connection,
      admin,
      admin.publicKey,
      null,
      6
    );

    // Create Token Accounts
    adminTreasury = await createAccount(
      provider.connection,
      admin,
      usdcMint,
      admin.publicKey
    );
    agentAToken = await createAccount(
      provider.connection,
      agentA,
      usdcMint,
      agentA.publicKey
    );
    agentBToken = await createAccount(
      provider.connection,
      agentB,
      usdcMint,
      agentB.publicKey
    );

    // Mint 1,000 USDC to each agent
    await mintTo(
      provider.connection,
      admin,
      usdcMint,
      agentAToken,
      admin,
      1_000_000_000
    );
    await mintTo(
      provider.connection,
      admin,
      usdcMint,
      agentBToken,
      admin,
      1_000_000_000
    );

    [arenaConfigPda] = anchor.web3.Pubkey.findProgramAddressSync(
      [Buffer.from("arena_config")],
      program.programId
    );

    [agentAProfilePda] = anchor.web3.Pubkey.findProgramAddressSync(
      [Buffer.from("agent_profile"), agentA.publicKey.toBuffer()],
      program.programId
    );

    [agentBProfilePda] = anchor.web3.Pubkey.findProgramAddressSync(
      [Buffer.from("agent_profile"), agentB.publicKey.toBuffer()],
      program.programId
    );
  });

  it("1. Initializes Arena Global Configuration", async () => {
    await program.methods
      .initializeArena(PROTOCOL_FEE_BPS)
      .accounts({
        arenaConfig: arenaConfigPda,
        usdcMint: usdcMint,
        admin: admin.publicKey,
        systemProgram: anchor.web3.SystemProgram.programId,
      })
      .signers([admin])
      .rpc();

    const config = await program.account.arenaConfig.fetch(arenaConfigPda);
    expect(config.admin.toBase58()).to.equal(admin.publicKey.toBase58());
    expect(config.protocolFeeBps).to.equal(PROTOCOL_FEE_BPS);
    expect(config.totalMatches.toNumber()).to.equal(0);
  });

  it("2. Registers Agent Profiles with Verifiable Model Hashes", async () => {
    const hashA = crypto.createHash("sha256").update("steve_agent_alpha_v1").digest();
    const hashB = crypto.createHash("sha256").update("steve_agent_beta_v1").digest();

    await program.methods
      .registerAgent("Steve-Alpha", "https://steve.ai/alpha.json", Array.from(hashA))
      .accounts({
        agentProfile: agentAProfilePda,
        authority: agentA.publicKey,
        systemProgram: anchor.web3.SystemProgram.programId,
      })
      .signers([agentA])
      .rpc();

    await program.methods
      .registerAgent("Steve-Beta", "https://steve.ai/beta.json", Array.from(hashB))
      .accounts({
        agentProfile: agentBProfilePda,
        authority: agentB.publicKey,
        systemProgram: anchor.web3.SystemProgram.programId,
      })
      .signers([agentB])
      .rpc();

    const profileA = await program.account.agentProfile.fetch(agentAProfilePda);
    expect(profileA.name).to.equal("Steve-Alpha");
    expect(profileA.eloRating).to.equal(1000);
    expect(profileA.matchesPlayed).to.equal(0);
  });

  it("3. Creates Match and Locks 500 USDC Total Escrow", async () => {
    const [matchStatePda] = anchor.web3.Pubkey.findProgramAddressSync(
      [Buffer.from("match_state"), matchId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );

    const [matchVaultPda] = anchor.web3.Pubkey.findProgramAddressSync(
      [Buffer.from("match_vault"), matchId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );

    await program.methods
      .createMatch(matchId, STAKE_AMOUNT)
      .accounts({
        matchState: matchStatePda,
        matchVault: matchVaultPda,
        arenaConfig: arenaConfigPda,
        usdcMint: usdcMint,
        agentAProfile: agentAProfilePda,
        agentAAuth: agentA.publicKey,
        agentAToken: agentAToken,
        agentBProfile: agentBProfilePda,
        agentBAuth: agentB.publicKey,
        agentBToken: agentBToken,
        tokenProgram: TOKEN_PROGRAM_ID,
        systemProgram: anchor.web3.SystemProgram.programId,
        rent: anchor.web3.SYSVAR_RENT_PUBKEY,
      })
      .signers([agentA, agentB])
      .rpc();

    const vaultAccount = await getAccount(provider.connection, matchVaultPda);
    expect(Number(vaultAccount.amount)).to.equal(500_000_000); // 500 USDC total
  });

  it("4. Executes Full Deterministic Agent Turn Combat Cycle", async () => {
    const [matchStatePda] = anchor.web3.Pubkey.findProgramAddressSync(
      [Buffer.from("match_state"), matchId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );

    // Agent A: Strike action (type: 0, power: 100)
    await program.methods
      .executeAgentAction(0, 100)
      .accounts({
        matchState: matchStatePda,
        agentAuth: agentA.publicKey,
      })
      .signers([agentA])
      .rpc();

    let state = await program.account.matchState.fetch(matchStatePda);
    expect(state.currentTurn).to.equal(1);
    expect(state.activeAgent.toBase58()).to.equal(agentB.publicKey.toBase58());
    expect(state.statsB.health).to.be.lessThan(100);

    // Agent B: Fortify Shield (type: 1, power: 80)
    await program.methods
      .executeAgentAction(1, 80)
      .accounts({
        matchState: matchStatePda,
        agentAuth: agentB.publicKey,
      })
      .signers([agentB])
      .rpc();

    state = await program.account.matchState.fetch(matchStatePda);
    expect(state.currentTurn).to.equal(2);
    expect(state.activeAgent.toBase58()).to.equal(agentA.publicKey.toBase58());
  });

  it("5. Completes Match and Disburses 500 USDC Bounty (475 USDC Winner, 25 USDC Protocol)", async () => {
    const [matchStatePda] = anchor.web3.Pubkey.findProgramAddressSync(
      [Buffer.from("match_state"), matchId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );
    const [matchVaultPda] = anchor.web3.Pubkey.findProgramAddressSync(
      [Buffer.from("match_vault"), matchId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );

    // Simulate Agent A delivering knockout strikes
    for (let i = 0; i < 4; i++) {
      const state = await program.account.matchState.fetch(matchStatePda);
      if (JSON.stringify(state.status) === JSON.stringify({ completed: {} })) break;

      const activeSigner = state.activeAgent.toBase58() === agentA.publicKey.toBase58() ? agentA : agentB;
      await program.methods
        .executeAgentAction(0, 100)
        .accounts({
          matchState: matchStatePda,
          agentAuth: activeSigner.publicKey,
        })
        .signers([activeSigner])
        .rpc();
    }

    const completedState = await program.account.matchState.fetch(matchStatePda);
    expect(JSON.stringify(completedState.status)).to.equal(JSON.stringify({ completed: {} }));
    const winnerPubkey = completedState.winner!;

    const winnerTokenAccount = winnerPubkey.toBase58() === agentA.publicKey.toBase58() ? agentAToken : agentBToken;

    const preBalanceWinner = Number((await getAccount(provider.connection, winnerTokenAccount)).amount);

    await program.methods
      .settleAndDisburse()
      .accounts({
        matchState: matchStatePda,
        matchVault: matchVaultPda,
        arenaConfig: arenaConfigPda,
        protocolFeeTreasury: adminTreasury,
        winnerTokenAccount: winnerTokenAccount,
        agentAProfile: agentAProfilePda,
        agentBProfile: agentBProfilePda,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .rpc();

    const postBalanceWinner = Number((await getAccount(provider.connection, winnerTokenAccount)).amount);
    const adminBalance = Number((await getAccount(provider.connection, adminTreasury)).amount);

    // 500 USDC Pot -> 5% Protocol Fee = 25 USDC, Winner = 475 USDC
    expect(postBalanceWinner - preBalanceWinner).to.equal(475_000_000);
    expect(adminBalance).to.equal(25_000_000);

    const updatedProfileA = await program.account.agentProfile.fetch(agentAProfilePda);
    expect(updatedProfileA.eloRating).to.equal(1032); // Initial 1000 + 32 K-Factor
  });
});