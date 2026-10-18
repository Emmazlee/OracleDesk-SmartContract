import { Buffer } from "buffer";
import { Address } from "@stellar/stellar-sdk";
import {
  AssembledTransaction,
  Client as ContractClient,
  ClientOptions as ContractClientOptions,
  MethodOptions,
  Result,
  Spec as ContractSpec,
} from "@stellar/stellar-sdk/contract";
import type {
  u32,
  i32,
  u64,
  i64,
  u128,
  i128,
  u256,
  i256,
  Option,
  Timepoint,
  Duration,
} from "@stellar/stellar-sdk/contract";
export * from "@stellar/stellar-sdk";
export * as contract from "@stellar/stellar-sdk/contract";
export * as rpc from "@stellar/stellar-sdk/rpc";

if (typeof window !== "undefined") {
  //@ts-ignore Buffer exists
  window.Buffer = window.Buffer || Buffer;
}




export const Errors = {
  1: {message:"NotInitialized"},
  2: {message:"Unauthorized"},
  3: {message:"InvalidConfig"},
  4: {message:"MarketAlreadyConfigured"},
  5: {message:"MarketNotConfigured"},
  6: {message:"WrongMode"},
  7: {message:"MarketNotClosed"},
  8: {message:"PriceUnavailable"},
  9: {message:"StalePrice"},
  10: {message:"InvalidPrice"},
  11: {message:"NotSigner"},
  12: {message:"AlreadyAttested"},
  13: {message:"ThresholdNotReached"},
  14: {message:"DisputeActive"},
  15: {message:"DisputeWindowOpen"},
  16: {message:"NoProposal"},
  17: {message:"AlreadyFinalized"},
  18: {message:"SpecHashMismatch"},
  19: {message:"DeadlineNotReached"},
  20: {message:"ResolutionAvailable"},
  21: {message:"DomainMismatch"},
  22: {message:"StaleRound"},
  23: {message:"TooManySignatures"},
  24: {message:"DisputeWindowClosed"},
  25: {message:"NoDispute"}
}

/**
 * The escalation authority's ruling. `Invalid` voids the market.
 */
export type Verdict = {tag: "Yes", values: void} | {tag: "No", values: void} | {tag: "Invalid", values: void};


/**
 * Who rules on a dispute.
 */
export type Escalation = {tag: "Guardian", values: void} | {tag: "Committee", values: readonly [Array<string>, u32]};

/**
 * Which resolution path a market was on when it expired.
 */
export type ExpiryMode = {tag: "Unrevealed", values: void} | {tag: "Price", values: void} | {tag: "Signers", values: void};

export type OracleAsset = {tag: "Stellar", values: readonly [string]} | {tag: "Other", values: readonly [string]};


export interface PriceConfig {
  asset: OracleAsset;
  direction: PriceDirection;
  max_staleness: u64;
  reflector: string;
  threshold: i128;
}


export interface WeightedKey {
  /**
 * Raw Ed25519 public key. For a `G...` account address this is
 * `StrKey.decodeEd25519PublicKey(address)`; `C...` addresses have none.
 */
pubkey: Buffer;
  weight: u32;
}


export interface SignerConfig {
  dispute_window: u64;
  signers: Array<string>;
  threshold: u32;
}


export type PriceDirection = {tag: "Above", values: void} | {tag: "AtOrAbove", values: void} | {tag: "Below", values: void} | {tag: "AtOrBelow", values: void};

/**
 * The full resolution rule a market's `resolution_hash` commits to. Hashing
 * this (rather than the bare `PriceConfig`/`SignerConfig`) keeps the two
 * modes in disjoint hash spaces even if their field encodings could
 * otherwise collide. See `docs/adr/0001-resolution-commitment.md`.
 */
export type ResolutionSpec = {tag: "Price", values: readonly [PriceConfig]} | {tag: "Signers", values: readonly [SignerConfig]} | {tag: "WeightedKeys", values: readonly [WeightedKeyConfig]} | {tag: "SignersDisputable", values: readonly [DisputableSignerConfig]};


export type ResolutionState = {tag: "Unconfigured", values: void} | {tag: "SignersPending", values: void} | {tag: "Finalized", values: void} | {tag: "Expired", values: void} | {tag: "Disputed", values: void};



/**
 * Versioned commitment envelope. Unlike a bare [`ResolutionSpec`] it binds the
 * network and the market-core instance, and carries a creator-chosen `salt`
 * that makes a pre-reveal brute-force of the (low-entropy) rule infeasible.
 */
export interface ResolutionSpecV2 {
  market_core: string;
  /**
 * `sha256(network passphrase)`, i.e. `env.ledger().network_id()`.
 */
network_id: Buffer;
  rule: ResolutionSpec;
  salt: Buffer;
  version: u32;
}


export interface WeightedKeyConfig {
  dispute_window: u64;
  signers: Array<WeightedKey>;
  /**
 * Total weight that must sign one outcome. Choose it above half the total
 * weight so two outcomes can never both reach it.
 */
weight_threshold: u32;
}




/**
 * Attested resolution whose proposal can be challenged with a bond during
 * the dispute window. Hashed as `ResolutionSpec::SignersDisputable`, a
 * separate variant, so existing `Signers` commitments are unaffected.
 */
export interface DisputableSignerConfig {
  /**
 * Collateral escrowed by `dispute`; must be positive.
 */
dispute_bond: i128;
  dispute_window: u64;
  escalation: Escalation;
  /**
 * Seconds after a dispute opens before anyone may void it unresolved.
 */
escalation_timeout: u64;
  signers: Array<string>;
  threshold: u32;
}

export type Outcome = {tag: "Yes", values: void} | {tag: "No", values: void};

export interface Client {
  /**
   * Construct and simulate a state transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  state: ({market_id}: {market_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<ResolutionState>>

  /**
   * Construct and simulate a attest transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  attest: ({market_id, signer, outcome}: {market_id: u64, signer: string, outcome: Outcome}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a dispute transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Challenges the current proposal of a `SignersDisputable` market while
   * its dispute window is still open (strictly before `proposed_at +
   * dispute_window`, the instant `finalize_signers` becomes callable). The
   * disputer escrows `dispute_bond` of the collateral token, which freezes
   * finalization until the escalation authority rules or
   * `escalation_timeout` lapses. `alternative` is the outcome the disputer
   * claims is correct (`None`: the market is invalid); it only informs the
   * ruling and must differ from the proposal. The disputer may also be a
   * proposing signer.
   */
  dispute: ({market_id, disputer, alternative}: {market_id: u64, disputer: string, alternative: Option<Outcome>}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a current_round transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Current attestation round of a weighted-key market; the `round`
   * argument of `submit_signatures` and part of every signed message.
   */
  current_round: ({market_id}: {market_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<u32>>

  /**
   * Construct and simulate a expire_market transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Permissionless liveness fallback: once `resolution_deadline` has passed,
   * anyone may void a market that is still unresolved, so collateral is
   * never locked behind a missing reveal, a dead feed or silent signers.
   * 
   * Resolution is preferred over voiding. A price market whose feed is
   * valid right now is *resolved* instead (so a stalled caller can never
   * force a void on a resolvable market), and a market with a pending
   * signer proposal is refused with `ResolutionAvailable` until
   * `finalize_signers` settles it.
   */
  expire_market: ({market_id}: {market_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a resolve_price transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  resolve_price: ({market_id}: {market_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Result<Outcome>>>

  /**
   * Construct and simulate a expire_dispute transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Permissionless fallback when the escalation authority never rules:
   * `escalation_timeout` after the dispute opened, the bond is refunded and
   * the market is voided, so neither side wins by default.
   */
  expire_dispute: ({market_id}: {market_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a settle_dispute transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Guardian escalation: the admin rules on an open dispute.
   */
  settle_dispute: ({market_id, verdict}: {market_id: u64, verdict: Verdict}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a cancel_proposal transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Guardian action: withdraws an unfinalized signer proposal, e.g. because
   * evidence surfaced during the dispute window that it is wrong. For
   * address-based signers voting can restart once the counts are cleared by
   * re-attesting; a weighted-key market moves to a fresh round, which makes
   * every signature collected so far unusable.
   */
  cancel_proposal: ({market_id}: {market_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a finalize_signers transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  finalize_signers: ({market_id}: {market_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Result<Outcome>>>

  /**
   * Construct and simulate a register_spec_v2 transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Reveals a rule committed to through a [`ResolutionSpecV2`] envelope.
   * The envelope must name this network and this resolver's market-core
   * (`DomainMismatch`), use a non-zero salt and the current `version`
   * (`InvalidConfig`), and hash to the market's commitment
   * (`SpecHashMismatch`). The inner rule is then validated and stored
   * exactly as the legacy entrypoints do.
   */
  register_spec_v2: ({market_id, spec}: {market_id: u64, spec: ResolutionSpecV2}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a escalation_attest transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Committee escalation: each member votes once; the first verdict to
   * gather `k` votes settles the dispute.
   */
  escalation_attest: ({market_id, member, verdict}: {market_id: u64, member: string, verdict: Verdict}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a submit_signatures transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Counts Ed25519 signatures from a weighted committee. Permissionless: the
   * signatures, not the submitter, carry the authority. Each signature is
   * over the message built by `attestation_message`; non-members and keys
   * that already signed this round (for either outcome) are skipped, and a
   * single invalid signature aborts the whole batch, so submitters should
   * pre-filter with a verifier. Once the accumulated weight for `outcome`
   * first reaches the threshold a proposal is recorded and the dispute
   * window starts; later calls never move it.
   */
  submit_signatures: ({market_id, outcome, round, sigs}: {market_id: u64, outcome: Outcome, round: u32, sigs: Array<readonly [Buffer, Buffer]>}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a claim_dispute_bond transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Pays a settled dispute's bond to its payee (the disputer, or the
   * market-core treasury when the proposal was upheld). Permissionless.
   */
  claim_dispute_bond: ({market_id}: {market_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a register_price_spec transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Reveals the resolution rule committed to at market creation. Anyone
   * may call this (there is no admin gate) because the only thing that
   * makes a spec acceptable is that it hashes to `market.resolution_hash`
   * — the commitment, not the caller, is the authority. Fails if a spec
   * was already registered for this market.
   */
  register_price_spec: ({market_id, spec}: {market_id: u64, spec: PriceConfig}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a resolution_deadline transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * When `expire_market` becomes callable for `market_id`: `close_time` plus
   * [`RESOLUTION_GRACE_SECS`]. Needs only market-core's `close_time`, so it
   * is known before (and without) a spec reveal.
   */
  resolution_deadline: ({market_id}: {market_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Result<u64>>>

  /**
   * Construct and simulate a guardian_void_market transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Guardian action: voids an unresolvable market directly through
   * market-core. The resolver is the direct caller, so market-core's
   * `require_resolver` check passes the same way `resolve_price` does.
   */
  guardian_void_market: ({market_id}: {market_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a register_signer_spec transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * See `register_price_spec` — same commit/reveal rule, signer mode.
   */
  register_signer_spec: ({market_id, spec}: {market_id: u64, spec: SignerConfig}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a register_disputable_spec transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * See `register_price_spec` — same commit/reveal rule, signer mode whose
   * proposals can be challenged with a bond (see `dispute`).
   */
  register_disputable_spec: ({market_id, spec}: {market_id: u64, spec: DisputableSignerConfig}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a register_weighted_key_spec transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * See `register_price_spec` — same commit/reveal rule, weighted Ed25519
   * committee that signs off-chain and is verified by `submit_signatures`.
   */
  register_weighted_key_spec: ({market_id, spec}: {market_id: u64, spec: WeightedKeyConfig}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

}
export class Client extends ContractClient {
  static async deploy<T = Client>(
        /** Constructor/Initialization Args for the contract's `__constructor` method */
        {admin, market_core}: {admin: string, market_core: string},
    /** Options for initializing a Client as well as for calling a method, with extras specific to deploying. */
    options: MethodOptions &
      Omit<ContractClientOptions, "contractId"> & {
        /** The hash of the Wasm blob, which must already be installed on-chain. */
        wasmHash: Buffer | string;
        /** Salt used to generate the contract's ID. Passed through to {@link Operation.createCustomContract}. Default: random. */
        salt?: Buffer | Uint8Array;
        /** The format used to decode `wasmHash`, if it's provided as a string. */
        format?: "hex" | "base64";
      }
  ): Promise<AssembledTransaction<T>> {
    return ContractClient.deploy({admin, market_core}, options)
  }
  constructor(public readonly options: ContractClientOptions) {
    super(
      new ContractSpec([ "AAAABAAAAAAAAAAAAAAABUVycm9yAAAAAAAAEgAAAAAAAAAOTm90SW5pdGlhbGl6ZWQAAAAAAAEAAAAAAAAADFVuYXV0aG9yaXplZAAAAAIAAAAAAAAADUludmFsaWRDb25maWcAAAAAAAADAAAAAAAAABdNYXJrZXRBbHJlYWR5Q29uZmlndXJlZAAAAAAEAAAAAAAAABNNYXJrZXROb3RDb25maWd1cmVkAAAAAAUAAAAAAAAACVdyb25nTW9kZQAAAAAAAAYAAAAAAAAAD01hcmtldE5vdENsb3NlZAAAAAAHAAAAAAAAABBQcmljZVVuYXZhaWxhYmxlAAAACAAAAAAAAAAKU3RhbGVQcmljZQAAAAAACQAAAAAAAAAMSW52YWxpZFByaWNlAAAACgAAAAAAAAAJTm90U2lnbmVyAAAAAAAACwAAAAAAAAAPQWxyZWFkeUF0dGVzdGVkAAAAAAwAAAAAAAAAE1RocmVzaG9sZE5vdFJlYWNoZWQAAAAADQAAAAAAAAANRGlzcHV0ZUFjdGl2ZQAAAAAAAA4AAAAAAAAAEURpc3B1dGVXaW5kb3dPcGVuAAAAAAAADwAAAAAAAAAKTm9Qcm9wb3NhbAAAAAAAEAAAAAAAAAAQQWxyZWFkeUZpbmFsaXplZAAAABEAAAAAAAAAEFNwZWNIYXNoTWlzbWF0Y2gAAAAS",
        "AAAAAgAAAAAAAAAAAAAAC09yYWNsZUFzc2V0AAAAAAIAAAABAAAAAAAAAAdTdGVsbGFyAAAAAAEAAAATAAAAAQAAAAAAAAAFT3RoZXIAAAAAAAABAAAAEQ==",
        "AAAAAQAAAAAAAAAAAAAAC1ByaWNlQ29uZmlnAAAAAAUAAAAAAAAABWFzc2V0AAAAAAAH0AAAAAtPcmFjbGVBc3NldAAAAAAAAAAACWRpcmVjdGlvbgAAAAAAB9AAAAAOUHJpY2VEaXJlY3Rpb24AAAAAAAAAAAANbWF4X3N0YWxlbmVzcwAAAAAAAAYAAAAAAAAACXJlZmxlY3RvcgAAAAAAABMAAAAAAAAACXRocmVzaG9sZAAAAAAAAAs=",
        "AAAAAQAAAAAAAAAAAAAAC1dlaWdodGVkS2V5AAAAAAIAAACCUmF3IEVkMjU1MTkgcHVibGljIGtleS4gRm9yIGEgYEcuLi5gIGFjY291bnQgYWRkcmVzcyB0aGlzIGlzCmBTdHJLZXkuZGVjb2RlRWQyNTUxOVB1YmxpY0tleShhZGRyZXNzKWA7IGBDLi4uYCBhZGRyZXNzZXMgaGF2ZSBub25lLgAAAAAABnB1YmtleQAAAAAD7gAAACAAAAAAAAAABndlaWdodAAAAAAABA==",
        "AAAAAQAAAAAAAAAAAAAADFNpZ25lckNvbmZpZwAAAAMAAAAAAAAADmRpc3B1dGVfd2luZG93AAAAAAAGAAAAAAAAAAdzaWduZXJzAAAAA+oAAAATAAAAAAAAAAl0aHJlc2hvbGQAAAAAAAAE",
        "AAAABQAAAAAAAAAAAAAADU1hcmtldEV4cGlyZWQAAAAAAAABAAAADm1hcmtldF9leHBpcmVkAAAAAAAEAAAAAAAAAAltYXJrZXRfaWQAAAAAAAAGAAAAAQAAAAAAAAAKY2xvc2VfdGltZQAAAAAABgAAAAAAAAAAAAAACGRlYWRsaW5lAAAABgAAAAAAAAAAAAAABG1vZGUAAAfQAAAACkV4cGlyeU1vZGUAAAAAAAAAAAAC",
        "AAAAAgAAAAAAAAAAAAAADlByaWNlRGlyZWN0aW9uAAAAAAAEAAAAAAAAAAAAAAAFQWJvdmUAAAAAAAAAAAAAAAAAAAlBdE9yQWJvdmUAAAAAAAAAAAAAAAAAAAVCZWxvdwAAAAAAAAAAAAAAAAAACUF0T3JCZWxvdwAAAA==",
        "AAAAAgAAAAAAAAAAAAAAD1Jlc29sdXRpb25TdGF0ZQAAAAADAAAAAAAAAAAAAAAMVW5jb25maWd1cmVkAAAAAAAAAAAAAAAOU2lnbmVyc1BlbmRpbmcAAAAAAAAAAAAAAAAACUZpbmFsaXplZAAAAA==",
        "AAAABQAAAAAAAAAAAAAAD1ByaWNlUmVzb2x1dGlvbgAAAAABAAAAEHByaWNlX3Jlc29sdXRpb24AAAAEAAAAAAAAAAltYXJrZXRfaWQAAAAAAAAGAAAAAQAAAAAAAAAFcHJpY2UAAAAAAAALAAAAAAAAAAAAAAAPcHJpY2VfdGltZXN0YW1wAAAAAAYAAAAAAAAAAAAAAAdvdXRjb21lAAAAB9AAAAAHT3V0Y29tZQAAAAAAAAAAAg==",
        "AAAABQAAAAAAAAAAAAAAElJlc29sdXRpb25Qcm9wb3NlZAAAAAAAAQAAABNyZXNvbHV0aW9uX3Byb3Bvc2VkAAAAAAMAAAAAAAAACW1hcmtldF9pZAAAAAAAAAYAAAABAAAAAAAAAAdvdXRjb21lAAAAB9AAAAAHT3V0Y29tZQAAAAAAAAAAAAAAAAtwcm9wb3NlZF9hdAAAAAAGAAAAAAAAAAI=",
        "AAAAAAAAAIRQYXlzIGEgc2V0dGxlZCBkaXNwdXRlJ3MgYm9uZCB0byBpdHMgcGF5ZWUgKHRoZSBkaXNwdXRlciwgb3IgdGhlCm1hcmtldC1jb3JlIHRyZWFzdXJ5IHdoZW4gdGhlIHByb3Bvc2FsIHdhcyB1cGhlbGQpLiBQZXJtaXNzaW9ubGVzcy4AAAASY2xhaW1fZGlzcHV0ZV9ib25kAAAAAAABAAAAAAAAAAltYXJrZXRfaWQAAAAAAAAGAAAAAQAAA+kAAAACAAAAAw==",
        "AAAABQAAAAAAAAAAAAAAE1Jlc29sdXRpb25GaW5hbGl6ZWQAAAAAAQAAABRyZXNvbHV0aW9uX2ZpbmFsaXplZAAAAAMAAAAAAAAACW1hcmtldF9pZAAAAAAAAAYAAAABAAAAAAAAAAdvdXRjb21lAAAAB9AAAAAHT3V0Y29tZQAAAAAAAAAAAAAAAAxmaW5hbGl6ZWRfYXQAAAAGAAAAAAAAAAI=",
        "AAAAAgAAAAAAAAAAAAAAB091dGNvbWUAAAAAAgAAAAAAAAAAAAAAA1llcwAAAAAAAAAAAAAAAAJObwAA",
        "AAAAAAAAAAAAAAAFc3RhdGUAAAAAAAABAAAAAAAAAAltYXJrZXRfaWQAAAAAAAAGAAAAAQAAB9AAAAAPUmVzb2x1dGlvblN0YXRlAA==",
        "AAAAAAAAAAAAAAAGYXR0ZXN0AAAAAAADAAAAAAAAAAltYXJrZXRfaWQAAAAAAAAGAAAAAAAAAAZzaWduZXIAAAAAABMAAAAAAAAAB291dGNvbWUAAAAH0AAAAAdPdXRjb21lAAAAAAEAAAPpAAAAAgAAAAM=",
        "AAAAAAAAAAAAAAANX19jb25zdHJ1Y3RvcgAAAAAAAAIAAAAAAAAABWFkbWluAAAAAAAAEwAAAAAAAAALbWFya2V0X2NvcmUAAAAAEwAAAAA=",
        "AAAAAAAAAAAAAAANcmVzb2x2ZV9wcmljZQAAAAAAAAEAAAAAAAAACW1hcmtldF9pZAAAAAAAAAYAAAABAAAD6QAAB9AAAAAHT3V0Y29tZQAAAAAD",
        "AAAAAAAAAMVHdWFyZGlhbiBhY3Rpb246IHdpdGhkcmF3cyBhbiB1bmZpbmFsaXplZCBzaWduZXIgcHJvcG9zYWwsIGUuZy4gYmVjYXVzZQpldmlkZW5jZSBzdXJmYWNlZCBkdXJpbmcgdGhlIGRpc3B1dGUgd2luZG93IHRoYXQgaXQgaXMgd3JvbmcuIFZvdGluZwpjYW4gcmVzdGFydCBvbmNlIHRoZSBjb3VudHMgYXJlIGNsZWFyZWQgYnkgcmUtYXR0ZXN0aW5nLgAAAAAAAA9jYW5jZWxfcHJvcG9zYWwAAAAAAQAAAAAAAAAJbWFya2V0X2lkAAAAAAAABgAAAAEAAAPpAAAAAgAAAAM=",
        "AAAAAAAAAAAAAAAQZmluYWxpemVfc2lnbmVycwAAAAEAAAAAAAAACW1hcmtldF9pZAAAAAAAAAYAAAABAAAD6QAAB9AAAAAHT3V0Y29tZQAAAAAD",
        "AAAAAAAAATpSZXZlYWxzIHRoZSByZXNvbHV0aW9uIHJ1bGUgY29tbWl0dGVkIHRvIGF0IG1hcmtldCBjcmVhdGlvbi4gQW55b25lCm1heSBjYWxsIHRoaXMgKHRoZXJlIGlzIG5vIGFkbWluIGdhdGUpIGJlY2F1c2UgdGhlIG9ubHkgdGhpbmcgdGhhdAptYWtlcyBhIHNwZWMgYWNjZXB0YWJsZSBpcyB0aGF0IGl0IGhhc2hlcyB0byBgbWFya2V0LnJlc29sdXRpb25faGFzaGAK4oCUIHRoZSBjb21taXRtZW50LCBub3QgdGhlIGNhbGxlciwgaXMgdGhlIGF1dGhvcml0eS4gRmFpbHMgaWYgYSBzcGVjCndhcyBhbHJlYWR5IHJlZ2lzdGVyZWQgZm9yIHRoaXMgbWFya2V0LgAAAAAAE3JlZ2lzdGVyX3ByaWNlX3NwZWMAAAAAAgAAAAAAAAAJbWFya2V0X2lkAAAAAAAABgAAAAAAAAAEc3BlYwAAB9AAAAALUHJpY2VDb25maWcAAAAAAQAAA+kAAAACAAAAAw==",
        "AAAAAAAAAL1XaGVuIGBleHBpcmVfbWFya2V0YCBiZWNvbWVzIGNhbGxhYmxlIGZvciBgbWFya2V0X2lkYDogYGNsb3NlX3RpbWVgIHBsdXMKW2BSRVNPTFVUSU9OX0dSQUNFX1NFQ1NgXS4gTmVlZHMgb25seSBtYXJrZXQtY29yZSdzIGBjbG9zZV90aW1lYCwgc28gaXQKaXMga25vd24gYmVmb3JlIChhbmQgd2l0aG91dCkgYSBzcGVjIHJldmVhbC4AAAAAAAATcmVzb2x1dGlvbl9kZWFkbGluZQAAAAABAAAAAAAAAAltYXJrZXRfaWQAAAAAAAAGAAAAAQAAA+kAAAAGAAAAAw==",
        "AAAAAAAAAMJHdWFyZGlhbiBhY3Rpb246IHZvaWRzIGFuIHVucmVzb2x2YWJsZSBtYXJrZXQgZGlyZWN0bHkgdGhyb3VnaAptYXJrZXQtY29yZS4gVGhlIHJlc29sdmVyIGlzIHRoZSBkaXJlY3QgY2FsbGVyLCBzbyBtYXJrZXQtY29yZSdzCmByZXF1aXJlX3Jlc29sdmVyYCBjaGVjayBwYXNzZXMgdGhlIHNhbWUgd2F5IGByZXNvbHZlX3ByaWNlYCBkb2VzLgAAAAAAFGd1YXJkaWFuX3ZvaWRfbWFya2V0AAAAAQAAAAAAAAAJbWFya2V0X2lkAAAAAAAABgAAAAEAAAPpAAAAAgAAAAM=",
        "AAAAAAAAAENTZWUgYHJlZ2lzdGVyX3ByaWNlX3NwZWNgIOKAlCBzYW1lIGNvbW1pdC9yZXZlYWwgcnVsZSwgc2lnbmVyIG1vZGUuAAAAABRyZWdpc3Rlcl9zaWduZXJfc3BlYwAAAAIAAAAAAAAACW1hcmtldF9pZAAAAAAAAAYAAAAAAAAABHNwZWMAAAfQAAAADFNpZ25lckNvbmZpZwAAAAEAAAPpAAAAAgAAAAM=",
        "AAAAAQAAANBBdHRlc3RlZCByZXNvbHV0aW9uIHdob3NlIHByb3Bvc2FsIGNhbiBiZSBjaGFsbGVuZ2VkIHdpdGggYSBib25kIGR1cmluZwp0aGUgZGlzcHV0ZSB3aW5kb3cuIEhhc2hlZCBhcyBgUmVzb2x1dGlvblNwZWM6OlNpZ25lcnNEaXNwdXRhYmxlYCwgYQpzZXBhcmF0ZSB2YXJpYW50LCBzbyBleGlzdGluZyBgU2lnbmVyc2AgY29tbWl0bWVudHMgYXJlIHVuYWZmZWN0ZWQuAAAAAAAAABZEaXNwdXRhYmxlU2lnbmVyQ29uZmlnAAAAAAAGAAAAM0NvbGxhdGVyYWwgZXNjcm93ZWQgYnkgYGRpc3B1dGVgOyBtdXN0IGJlIHBvc2l0aXZlLgAAAAAMZGlzcHV0ZV9ib25kAAAACwAAAAAAAAAOZGlzcHV0ZV93aW5kb3cAAAAAAAYAAAAAAAAACmVzY2FsYXRpb24AAAAAB9AAAAAKRXNjYWxhdGlvbgAAAAAAQ1NlY29uZHMgYWZ0ZXIgYSBkaXNwdXRlIG9wZW5zIGJlZm9yZSBhbnlvbmUgbWF5IHZvaWQgaXQgdW5yZXNvbHZlZC4AAAAAEmVzY2FsYXRpb25fdGltZW91dAAAAAAABgAAAAAAAAAHc2lnbmVycwAAAAPqAAAAEwAAAAAAAAAJdGhyZXNob2xkAAAAAAAABA==",
        "AAAAAAAAAIFTZWUgYHJlZ2lzdGVyX3ByaWNlX3NwZWNgIOKAlCBzYW1lIGNvbW1pdC9yZXZlYWwgcnVsZSwgc2lnbmVyIG1vZGUgd2hvc2UKcHJvcG9zYWxzIGNhbiBiZSBjaGFsbGVuZ2VkIHdpdGggYSBib25kIChzZWUgYGRpc3B1dGVgKS4AAAAAAAAYcmVnaXN0ZXJfZGlzcHV0YWJsZV9zcGVjAAAAAgAAAAAAAAAJbWFya2V0X2lkAAAAAAAABgAAAAAAAAAEc3BlYwAAB9AAAAAWRGlzcHV0YWJsZVNpZ25lckNvbmZpZwAAAAAAAQAAA+kAAAACAAAAAw==",
        "AAAAAAAAAI5TZWUgYHJlZ2lzdGVyX3ByaWNlX3NwZWNgIOKAlCBzYW1lIGNvbW1pdC9yZXZlYWwgcnVsZSwgd2VpZ2h0ZWQgRWQyNTUxOQpjb21taXR0ZWUgdGhhdCBzaWducyBvZmYtY2hhaW4gYW5kIGlzIHZlcmlmaWVkIGJ5IGBzdWJtaXRfc2lnbmF0dXJlc2AuAAAAAAAacmVnaXN0ZXJfd2VpZ2h0ZWRfa2V5X3NwZWMAAAAAAAIAAAAAAAAACW1hcmtldF9pZAAAAAAAAAYAAAAAAAAABHNwZWMAAAfQAAAAEVdlaWdodGVkS2V5Q29uZmlnAAAAAAAAAQAAA+kAAAACAAAAAw==" ]),
      options
    )
  }
  public readonly fromJSON = {
    state: this.txFromJSON<ResolutionState>,
        attest: this.txFromJSON<Result<void>>,
        dispute: this.txFromJSON<Result<void>>,
        current_round: this.txFromJSON<u32>,
        expire_market: this.txFromJSON<Result<void>>,
        resolve_price: this.txFromJSON<Result<Outcome>>,
        expire_dispute: this.txFromJSON<Result<void>>,
        settle_dispute: this.txFromJSON<Result<void>>,
        cancel_proposal: this.txFromJSON<Result<void>>,
        finalize_signers: this.txFromJSON<Result<Outcome>>,
        register_spec_v2: this.txFromJSON<Result<void>>,
        escalation_attest: this.txFromJSON<Result<void>>,
        submit_signatures: this.txFromJSON<Result<void>>,
        claim_dispute_bond: this.txFromJSON<Result<void>>,
        register_price_spec: this.txFromJSON<Result<void>>,
        resolution_deadline: this.txFromJSON<Result<u64>>,
        guardian_void_market: this.txFromJSON<Result<void>>,
        register_signer_spec: this.txFromJSON<Result<void>>,
        register_disputable_spec: this.txFromJSON<Result<void>>,
        register_weighted_key_spec: this.txFromJSON<Result<void>>
  }
}