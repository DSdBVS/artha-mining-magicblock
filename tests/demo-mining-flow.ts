// demo-mining-flow.ts
//
// ARTHA Mining — полная демонстрация игрового цикла для MagicBlock хакатона.
// Запускать и записывать терминал как демо-видео (рядом можно открыть
// Solana Explorer на адресе minerPda, чтобы показывать транзакции вживую).
//
// Флоу: выбор фракции → delegate в Ephemeral Rollup → VRF-тик майнинга
// (request + callback) → показ намайненного → undelegate → claim в токен фракции.

import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { PublicKey, LAMPORTS_PER_SOL } from "@solana/web3.js";
import { ArthaMining } from "../target/types/artha_mining";

const EPHEMERAL_ENDPOINT = "https://devnet-as.magicblock.app/";
const EPHEMERAL_WS_ENDPOINT = "wss://devnet-as.magicblock.app/";
const BOBBY_MINT = new PublicKey("LxUpczgFu1jE5QmRcRhjYgW3fP5MV3nGm1woJQsFR5a");
const RABBIT_MINT = new PublicKey("2mAjpRkrthCAtA2VjhBiWL9pem4QmbzBTgTCmHn6Rsij");

// --- Выбери фракцию для демо здесь: ---
const CHOSEN_FACTION = "bobby"; // "bobby" | "rabbit"
// ----------------------------------------

const FACTION_ARG = CHOSEN_FACTION === "bobby" ? { bobby: {} } : { blackRabbit: {} };
const FACTION_MINT = CHOSEN_FACTION === "bobby" ? BOBBY_MINT : RABBIT_MINT;
const FACTION_LABEL = CHOSEN_FACTION === "bobby" ? "🐕  BOBBY" : "🐇  BLACK RABBIT";

function line() { console.log("─".repeat(60)); }
function step(n: number, title: string) {
  console.log("");
  line();
  console.log(`  STEP ${n} — ${title}`);
  line();
}

async function main() {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const providerEphemeralRollup = new anchor.AnchorProvider(
    new anchor.web3.Connection(EPHEMERAL_ENDPOINT, { wsEndpoint: EPHEMERAL_WS_ENDPOINT }),
    anchor.Wallet.local(),
  );

  const program = anchor.workspace.ArthaMining as Program<ArthaMining>;
  const ephemeralProgram = new Program(program.idl, providerEphemeralRollup) as Program<ArthaMining>;
  const wallet = anchor.Wallet.local();

  const [minerPda] = PublicKey.findProgramAddressSync(
    [Buffer.from("miner"), wallet.publicKey.toBuffer()],
    program.programId,
  );

  console.log("");
  console.log("╔════════════════════════════════════════════════════════╗");
  console.log("║   ARTHA MINING — Bangkok 2099                           ║");
  console.log("║   Real-time mining on MagicBlock Ephemeral Rollups       ║");
  console.log("╚════════════════════════════════════════════════════════╝");
  console.log("");
  console.log(`  Player wallet: ${wallet.publicKey.toString()}`);
  console.log(`  Miner PDA:     ${minerPda.toString()}`);
  console.log(`  Faction:       ${FACTION_LABEL}`);
  console.log(`  Program ID:    ${program.programId.toString()}`);

  // STEP 1 — Initialize miner
  step(1, `Choosing faction: ${FACTION_LABEL}`);
  const existing = await provider.connection.getAccountInfo(minerPda);
  if (existing) {
    console.log("  Miner already exists (owner: " + existing.owner.toString() + ") — continuing with existing account.");
  } else {
    const tx = await program.methods
      .initializeMiner(FACTION_ARG as any)
      .accounts({ player: wallet.publicKey, miner: minerPda })
      .rpc({ skipPreflight: true });
    console.log(`  ✅ Miner initialized. Tx: ${tx}`);
  }

  // STEP 2 — Delegate to Ephemeral Rollup
  step(2, "Delegating miner account to Ephemeral Rollup");
  const DELEGATION_PROGRAM_ID_CHECK = new PublicKey("DELeGGvXpWV2fqJUhqcF5ZSYMS4JTLjteaAMARRSaeSh");
  const accountBeforeDelegate = await provider.connection.getAccountInfo(minerPda);
  if (accountBeforeDelegate?.owner.equals(DELEGATION_PROGRAM_ID_CHECK)) {
    console.log("  Already delegated — skipping.");
  } else {
    console.log("  Handing off control to the real-time rollup for gasless, instant ticks...");
    // No remainingAccounts here: VALIDATOR (mAGicPQY...) is the LOCALNET-only
    // validator identity. Passing it on devnet delegates to a validator identity
    // the router/ER don't recognize, breaking every later ER operation with
    // "Unknown action 'undefined'". Omit it so the delegation program assigns
    // the real devnet ER validator.
    const delegateTx = await program.methods
      .delegateMiner()
      .accounts({ player: wallet.publicKey, miner: minerPda })
      .rpc({ skipPreflight: true });
    console.log(`  ✅ Delegated. Tx: ${delegateTx}`);
    await new Promise((r) => setTimeout(r, 1500));
  }

  // STEP 3 — Mine tick (VRF request + callback)
  step(3, "Mining tick — requesting verifiable randomness");
  const clientSeed = Math.floor(Math.random() * 256);
  const DELEGATION_PROGRAM_ID = new PublicKey("DELeGGvXpWV2fqJUhqcF5ZSYMS4JTLjteaAMARRSaeSh");
  const [delegationRecordMiner] = PublicKey.findProgramAddressSync(
    [Buffer.from("delegation"), minerPda.toBuffer()],
    DELEGATION_PROGRAM_ID,
  );
  const oracleQueue = new PublicKey(process.env.VRF_ORACLE_QUEUE || "5hBR571xnXppuCPveTrctfTU7tJLSN94nq7kv7FRK5Tc");

  const minerBeforeTick = await program.account.minerAccount.fetch(minerPda);

  let mineTx = await ephemeralProgram.methods
    .requestMineTick(clientSeed)
    .accounts({
      player: wallet.publicKey,
      miner: minerPda,
      oracleQueue,
      delegationRecordMiner,
    })
    .transaction();
  mineTx.feePayer = wallet.publicKey;
  mineTx.recentBlockhash = (await providerEphemeralRollup.connection.getLatestBlockhash()).blockhash;

  const mineTxHash = await providerEphemeralRollup
    .sendAndConfirm(mineTx, [wallet.payer], { skipPreflight: true })
    .catch((err) => { console.log(`  ⚠ Request error: ${err.message}`); return null; });

  if (mineTxHash) {
    console.log(`  ⛏  Tick requested. Tx: ${mineTxHash}`);
    console.log("  ⏳ Waiting for VRF oracle callback (consume_mine_tick)...");

    // Poll ER account state instead of subscribing to logs: no WS listener to
    // leak on timeout, and it can't miss a callback due to a dropped/reconnected
    // WS subscription.
    const pollDeadline = Date.now() + 45000;
    let callbackLanded = false;
    while (Date.now() < pollDeadline) {
      await new Promise((r) => setTimeout(r, 2000));
      const minerNow = await ephemeralProgram.account.minerAccount.fetch(minerPda);
      if (minerNow.lastMineTs.toString() !== minerBeforeTick.lastMineTs.toString()) {
        console.log(`  📡 Callback landed — total ore now ${minerNow.totalOre.toString()}.`);
        callbackLanded = true;

        // One-shot log fetch now that we know the callback landed — not a
        // subscription, so there's nothing to leak. Newest ER signature for
        // this account is the consume_mine_tick callback.
        const recentSigs = await providerEphemeralRollup.connection.getSignaturesForAddress(minerPda, { limit: 1 });
        const callbackSig = recentSigs[0]?.signature;
        if (callbackSig) {
          const callbackTx = await providerEphemeralRollup.connection.getTransaction(callbackSig, {
            commitment: "confirmed",
            maxSupportedTransactionVersion: 0,
          });
          if (callbackTx?.meta?.logMessages) {
            console.log("  📜 consume_mine_tick logs:");
            callbackTx.meta.logMessages.forEach((l) => console.log("     " + l));
          }
        }
        break;
      }
    }
    if (!callbackLanded) {
      console.log("  ⚠ Callback timeout (45s) — result may still be pending.");
    }
  }

  // STEP 4 — Show miner state
  step(4, "Checking miner state");
  // Fetch via ephemeralProgram (ER connection), not program (base): the miner
  // is still delegated at this point, so base-layer data is stale until the
  // undelegate commit in STEP 5 lands. Reading through base here silently
  // showed 0 ore even after a successful mine tick.
  const minerState = await ephemeralProgram.account.minerAccount.fetch(minerPda);
  console.log(`  ⛏  Total Solanite mined: ${minerState.totalOre.toString()}`);
  console.log(`  ✦  Rare finds:           ${minerState.rareFinds}`);

  // STEP 5 — Undelegate
  step(5, "Undelegating — committing final state back to Solana L1");
  let undelegateTx = await ephemeralProgram.methods
    .undelegateMiner()
    .accounts({ player: wallet.publicKey, miner: minerPda })
    .transaction();
  undelegateTx.feePayer = providerEphemeralRollup.wallet.publicKey;
  undelegateTx.recentBlockhash = (await providerEphemeralRollup.connection.getLatestBlockhash()).blockhash;
  undelegateTx = await providerEphemeralRollup.wallet.signTransaction(undelegateTx);

  const undelegateTxHash = await providerEphemeralRollup
    .sendAndConfirm(undelegateTx, [], { skipPreflight: true })
    .catch((err) => { console.log(`  ⚠ Undelegate error: ${err.message}`); return null; });
  if (undelegateTxHash) console.log(`  ✅ Undelegated. Tx: ${undelegateTxHash}`);

  // Undelegate only *schedules* the base-layer commit — it does not confirm it.
  // claim_rewards runs on base layer and requires the miner account to be owned
  // by our program again; calling it immediately races the commit and fails
  // with AccountOwnedByWrongProgram while ownership is still the Delegation
  // Program. Poll base ownership until it flips back before claiming.
  if (undelegateTxHash) {
    console.log("  ⏳ Waiting for base-layer commit to finalize...");
    const DELEGATION_PROGRAM_ID_WAIT = new PublicKey("DELeGGvXpWV2fqJUhqcF5ZSYMS4JTLjteaAMARRSaeSh");
    const commitDeadline = Date.now() + 20000;
    let committed = false;
    while (Date.now() < commitDeadline) {
      const info = await provider.connection.getAccountInfo(minerPda);
      if (info && info.owner.equals(program.programId)) {
        committed = true;
        break;
      }
      await new Promise((r) => setTimeout(r, 1000));
    }
    if (committed) {
      console.log("  ✅ Base-layer commit confirmed — miner owned by program again.");
    } else {
      console.log("  ⚠ Commit not confirmed after 20s — claim may still fail.");
    }
  }

  // STEP 6 — Claim rewards
  step(6, `Claiming rewards in ${CHOSEN_FACTION.toUpperCase()} token`);
  const claimTx = await program.methods
    .claimRewards()
    .accounts({ player: wallet.publicKey, miner: minerPda, rewardMint: FACTION_MINT })
    .rpc({ skipPreflight: true })
    .catch((err) => { console.log(`  ⚠ Claim error: ${err.message}`); return null; });
  if (claimTx) console.log(`  ✅ Claimed. Tx: ${claimTx}`);

  console.log("");
  line();
  console.log("  🎉 DEMO COMPLETE — full mining cycle executed on MagicBlock");
  line();
  console.log("");
}

main().catch((err) => {
  console.error("Fatal error:", err);
  process.exit(1);
});
