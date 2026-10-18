/**
 * Off-chain Ed25519 attestations for `resolver.submit_signatures`.
 *
 * A committee member signs the message built here with a plain Ed25519 key;
 * anyone (typically the agent or a keeper) collects the signatures and submits
 * them in one transaction. The byte layout must match
 * `Resolver::attestation_message` in contracts/resolver/src/lib.rs exactly:
 *
 *   "oracledesk-attest-v1"                  20 ASCII bytes, domain tag
 *   || network_id                           32 bytes, sha256(network passphrase)
 *   || ScVal-XDR(resolver contract address) 40 bytes for a C... address
 *   || market_id                            u64 big-endian
 *   || round                                u32 big-endian
 *   || outcome                              1 byte: 0 = Yes, 1 = No
 *   || spec_hash                            32 bytes, the market's resolution_hash
 *
 * The message is signed raw (never pre-hashed), matching the contract's
 * `ed25519_verify`. The golden vectors in attestation.test.ts are shared with
 * the Rust test `golden_attestation_message_and_signature`.
 */

import { createHash } from "node:crypto";
import { Address, Keypair, StrKey } from "@stellar/stellar-sdk";

const DOMAIN = Buffer.from("oracledesk-attest-v1", "ascii");
/** Order of the Ed25519 base-point subgroup. */
const GROUP_ORDER = (1n << 252n) + 27742317777372353535851937790883648493n;

export type AttestedOutcome = "Yes" | "No";

export interface AttestationContext {
  networkPassphrase: string;
  /** The resolver contract (`C...`) the signatures will be submitted to. */
  resolverId: string;
  marketId: bigint;
  /** `resolver.current_round(marketId)` at signing time. */
  round: number;
  outcome: AttestedOutcome;
  /** 32-byte `resolution_hash` of the market (from market-core `get_market`). */
  specHash: Uint8Array;
}

export interface Attestation {
  pubkey: Uint8Array;
  sig: Uint8Array;
}

export function buildAttestationMessage(ctx: AttestationContext): Uint8Array {
  if (ctx.specHash.length !== 32) {
    throw new Error(`specHash must be 32 bytes, got ${ctx.specHash.length}`);
  }
  if (!Number.isInteger(ctx.round) || ctx.round < 0 || ctx.round > 0xffff_ffff) {
    throw new Error(`round must be a u32, got ${ctx.round}`);
  }
  const marketId = Buffer.alloc(8);
  marketId.writeBigUInt64BE(ctx.marketId);
  const round = Buffer.alloc(4);
  round.writeUInt32BE(ctx.round);

  return Buffer.concat([
    DOMAIN,
    createHash("sha256").update(ctx.networkPassphrase).digest(),
    Address.fromString(ctx.resolverId).toScVal().toXDR(),
    marketId,
    round,
    Buffer.from([ctx.outcome === "Yes" ? 0 : 1]),
    ctx.specHash,
  ]);
}

export function signAttestation(keypair: Keypair, ctx: AttestationContext): Attestation {
  const message = buildAttestationMessage(ctx);
  return {
    pubkey: new Uint8Array(keypair.rawPublicKey()),
    sig: new Uint8Array(keypair.sign(Buffer.from(message))),
  };
}

/**
 * Pre-filter for a batch before `submit_signatures`: the contract aborts the
 * whole batch on one invalid signature, so drop anything this rejects.
 *
 * Besides the Ed25519 check it rejects a non-canonical `S` (>= the group
 * order), which the contract also rejects. It does not re-implement the
 * contract's rejection of weak (small-order) public keys and small-order `R`,
 * so it can pass a signature the contract refuses; that only makes the batch
 * fail, it can never make an invalid signature count.
 */
export function verifyAttestation(
  pubkey: Uint8Array,
  sig: Uint8Array,
  ctx: AttestationContext,
): boolean {
  if (pubkey.length !== 32 || sig.length !== 64) return false;
  const s = BigInt(`0x${Buffer.from(sig.slice(32)).reverse().toString("hex")}`);
  if (s >= GROUP_ORDER) return false;
  const message = Buffer.from(buildAttestationMessage(ctx));
  const key = Keypair.fromPublicKey(StrKey.encodeEd25519PublicKey(Buffer.from(pubkey)));
  return key.verify(message, Buffer.from(sig));
}
