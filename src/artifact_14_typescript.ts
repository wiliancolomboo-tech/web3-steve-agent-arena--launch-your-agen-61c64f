// tests/steve-agent-arena.ts
import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { SteveAgentArena } from "../target/types/steve_agent_arena";
import { 
    Keypair, 
    PublicKey, 
    SystemProgram, 
    SYSVAR_RENT_PUBKEY 
} from "@solana/web3.js";
import { 
    TOKEN_PROGRAM_ID, 
    createMint, 
    createAccount, 
    mintTo, 
    getAccount 
} from "@solana/spl-token";
import { createHash, randomBytes } from "crypto";
import { expect } from "chai";

describe("Steve Agent Arena - Full Lifecycle Protocol Test Suite", () => {
    const provider = anchor.AnchorProvider.env();
    anchor.setProvider(provider);

    const program = anchor.workspace.SteveAgentArena as Program<SteveAgentArena>;

    let usdcMint: PublicKey;
    let admin = Keypair.generate();
    let agentAlpha = Keypair.generate();
    let agentBeta = Keypair.generate();

    let adminAta: PublicKey;
    let alphaAta: PublicKey;
    let betaAta: PublicKey;

    const STAKE_AMOUNT = 250_000_000; // 250 USDC per agent (Total 500 USDC pool)
    const MATCH_ID = new anchor.BN(1001);

    let arenaConfigPda: PublicKey;
    let matchSessionPda: PublicKey;
    let matchVaultPda: PublicKey;

    before(async () => {
        // Airdrop SOL to test participants
        for (const kp of [admin, agentAlpha, agentBeta]) {
            const sig = await provider.connection.requestAirdrop(kp.publicKey, 5 * anchor.web3.LAMPORTS_PER_SOL);
            await provider.connection.confirmTransaction(sig);
        }

        // Deploy Mock USDC Mint
        usdcMint = await createMint(
            provider.connection,
            admin,
            admin.publicKey,
            null,
            6
        );

        // Derive PDAs
        [arenaConfigPda] = PublicKey.findProgramAddressSync([Buffer.from("arena-config")], program.programId);
        [matchSessionPda] = PublicKey.findProgramAddressSync(
            [Buffer.from("match-session"), MATCH_ID.toArrayLike(Buffer, "le", 8)],
            program.programId
        );
        [matchVaultPda] = PublicKey.findProgramAddressSync(
            [Buffer.from("arena-vault"), MATCH_ID.toArrayLike(Buffer, "le", 8)],
            program.programId
        );

        // Create Token Accounts
        adminAta = await createAccount(provider.connection, admin, usdcMint, admin.publicKey);
        alphaAta = await createAccount(provider.connection, agentAlpha, usdcMint, agentAlpha.publicKey);
        betaAta = await createAccount(provider.connection, agentBeta, usdcMint, agentBeta.publicKey);

        // Fund Agents with Stakes
        await mintTo(provider.connection, admin, usdcMint, alphaAta, admin, STAKE_AMOUNT);
        await mintTo(provider.connection, admin, usdcMint, betaAta, admin, STAKE_AMOUNT);
    });

    it("1. Initializes Steve Agent Arena Protocol", async () => {
        await program.methods
            .initializeArena(200) // 2.00% protocol fee
            .accounts({
                arenaConfig: arenaConfigPda,
                usdcMint: usdcMint,
                admin: admin.publicKey,
                systemProgram: SystemProgram.programId,
            })
            .signers([admin])
            .rpc();

        const config = await program.account.arenaConfig.fetch(arenaConfigPda);
        expect(config.admin.toBase58()).to.equal(admin.publicKey.toBase58());
        expect(config.protocolFeeBps).to.equal(200);
        expect(config.isActive).to.be.true;
    });

    it("2. Registers Agent Alpha & Escrows 250 USDC", async () => {
        await program.methods
            .registerAgent(MATCH_ID, new anchor.BN(STAKE_AMOUNT))
            .accounts({
                arenaConfig: arenaConfigPda,
                matchSession: matchSessionPda,
                matchVault: matchVaultPda,
                usdcMint: usdcMint,
                agentTokenAccount: alphaAta,
                agentAuthority: agentAlpha.publicKey,
                tokenProgram: TOKEN_PROGRAM_ID,
                systemProgram: SystemProgram.programId,
                rent: SYSVAR_RENT_PUBKEY,
            })
            .signers([agentAlpha])
            .rpc();

        const vaultAcc = await getAccount(provider.connection, matchVaultPda);
        expect(Number(vaultAcc.amount)).to.equal(STAKE_AMOUNT);
    });

    it("3. Registers Agent Beta (Pool reaches 500 USDC)", async () => {
        await program.methods
            .registerAgent(MATCH_ID, new anchor.BN(STAKE_AMOUNT))
            .accounts({
                arenaConfig: arenaConfigPda,
                matchSession: matchSessionPda,
                matchVault: matchVaultPda,
                usdcMint: usdcMint,
                agentTokenAccount: betaAta,
                agentAuthority: agentBeta.publicKey,
                tokenProgram: TOKEN_PROGRAM_ID,
                systemProgram: SystemProgram.programId,
                rent: SYSVAR_RENT_PUBKEY,
            })
            .signers([agentBeta])
            .rpc();

        const vaultAcc = await getAccount(provider.connection, matchVaultPda);
        expect(Number(vaultAcc.amount)).to.equal(STAKE_AMOUNT * 2); // 500 USDC
    });

    // Strategy Execution: Alpha plays 0 (Arb), Beta plays 1 (Front-Run) -> Alpha wins per rule matrix
    const saltAlpha = randomBytes(32);
    const hashAlpha = createHash("sha256").update(Buffer.concat([Buffer.from([0]), saltAlpha])).digest();

    const saltBeta = randomBytes(32);
    const hashBeta = createHash("sha256").update(Buffer.concat([Buffer.from([1]), saltBeta])).digest();

    it("4. Both Agents Commit Hashed Actions", async () => {
        await program.methods
            .commitAction(MATCH_ID, Array.from(hashAlpha))
            .accounts({
                matchSession: matchSessionPda,
                agentAuthority: agentAlpha.publicKey,
            })
            .signers([agentAlpha])
            .rpc();

        await program.methods
            .commitAction(MATCH_ID, Array.from(hashBeta))
            .accounts({
                matchSession: matchSessionPda,
                agentAuthority: agentBeta.publicKey,
            })
            .signers([agentBeta])
            .rpc();

        const session = await program.account.matchSession.fetch(matchSessionPda);
        expect(session.agentAlpha.hasCommitted).to.be.true;
        expect(session.agentBeta.hasCommitted).to.be.true;
    });

    it("5. Agents Reveal Actions with Cryptographic Proofs", async () => {
        await program.methods
            .revealAction(MATCH_ID, 0, Array.from(saltAlpha))
            .accounts({
                matchSession: matchSessionPda,
                agentAuthority: agentAlpha.publicKey,
            })
            .signers([agentAlpha])
            .rpc();

        await program.methods
            .revealAction(MATCH_ID, 1, Array.from(saltBeta))
            .accounts({
                matchSession: matchSessionPda,
                agentAuthority: agentBeta.publicKey,
            })
            .signers([agentBeta])
            .rpc();

        const session = await program.account.matchSession.fetch(matchSessionPda);
        expect(session.agentAlpha.revealedAction).to.equal(0);
        expect(session.agentBeta.revealedAction).to.equal(1);
    });

    it("6. Settles Match & Disburses 500 USDC Pool to Winner (Alpha)", async () => {
        await program.methods
            .settleMatch(MATCH_ID)
            .accounts({
                arenaConfig: arenaConfigPda,
                matchSession: matchSessionPda,
                matchVault: matchVaultPda,
                winnerTokenAccount: alphaAta,
                treasuryTokenAccount: adminAta,
                tokenProgram: TOKEN_PROGRAM_ID,
            })
            .rpc();

        const session = await program.account.matchSession.fetch(matchSessionPda);
        expect(session.winner.toBase58()).to.equal(agentAlpha.publicKey.toBase58());

        const alphaFinalAta = await getAccount(provider.connection, alphaAta);
        const adminFinalAta = await getAccount(provider.connection, adminAta);

        // 500 USDC - 2% fee (10 USDC) = 490 USDC (490_000_000 units)
        expect(Number(alphaFinalAta.amount)).to.equal(490_000_000);
        expect(Number(adminFinalAta.amount)).to.equal(10_000_000);
    });
});