/**
 * Builds and hashes the reasoning traces that get recorded on-chain via
 * reasoning-registry.publish_trace and sold over x402. A trace records why
 * an agent took an action, in a form a paying subscriber can check against
 * the on-chain hash (see x402/trace-verification.ts) before trusting it.
 *
 * The hash is sha256 of the *canonical* JSON encoding — same field order,
 * no whitespace variance — so the same trace object always hashes the same
 * way regardless of how it was constructed or serialized upstream.
 */

import { createHash, randomBytes } from "node:crypto";

export type AgentRole = "market-maker" | "trader";

export interface ReasoningTrace {
  agent: AgentRole;
  marketId: number | null; // null for a not-yet-created market (agent proposals)
  action: string;
  reasoning: string;
  /** Whatever signals/estimates informed the decision — kept as plain
   * JSON-serializable data so it survives the canonicalization below. */
  inputs: Record<string, unknown>;
  timestamp: number; // unix seconds
}

/**
 * Deterministic JSON encoding: object keys sorted recursively, no
 * insignificant whitespace. Arrays keep their given order (order is
 * significant there). This must stay byte-for-byte stable over time —
 * changing it changes every future trace's hash relative to past ones, so
 * treat it as part of the public interface, not an implementation detail.
 */
export function canonicalize(value: unknown): string {
  return JSON.stringify(sortKeysDeep(value));
}

function sortKeysDeep(value: unknown): unknown {
  // JSON has no bigint literal; stringify it explicitly rather than
  // letting JSON.stringify throw. Traces regularly carry stroop amounts.
  if (typeof value === "bigint") return value.toString();
  if (Array.isArray(value)) return value.map(sortKeysDeep);
  if (value !== null && typeof value === "object") {
    const sorted: Record<string, unknown> = {};
    for (const key of Object.keys(value as Record<string, unknown>).sort()) {
      sorted[key] = sortKeysDeep((value as Record<string, unknown>)[key]);
    }
    return sorted;
  }
  return value;
}

export function hashTrace(trace: ReasoningTrace): string {
  return createHash("sha256").update(canonicalize(trace)).digest("hex");
}

export interface BuiltTrace {
  trace: ReasoningTrace;
  json: string;
  hash: string;
}

export function buildTrace(input: Omit<ReasoningTrace, "timestamp"> & { timestamp?: number }): BuiltTrace {
  const trace: ReasoningTrace = { ...input, timestamp: input.timestamp ?? Math.floor(Date.now() / 1000) };
  const json = canonicalize(trace);
  return { trace, json, hash: createHash("sha256").update(json).digest("hex") };
}

/**
 * Where a built trace's JSON is persisted before its CID goes on-chain.
 * `LocalTraceStore` is the dry-run/dev default (writes under a local
 * directory); a real deployment swaps in an IPFS-pinning implementation of
 * this same interface without touching any calling code.
 */
export interface TraceStore {
  put(hash: string, json: string): Promise<{ cid: string }>;
}

export class LocalTraceStore implements TraceStore {
  constructor(private readonly dir: string) {}

  async put(hash: string, json: string): Promise<{ cid: string }> {
    const { mkdir, writeFile } = await import("node:fs/promises");
    const { join } = await import("node:path");
    await mkdir(this.dir, { recursive: true });
    const path = join(this.dir, `${hash}.json`);
    await writeFile(path, json, "utf8");
    // Not a real content identifier — this is the dry-run/dev store. A
    // production TraceStore (pinning to IPFS) returns a real CID here.
    return { cid: `local://${path}` };
  }
}

// ─── Commit-reveal (#51) ─────────────────────────────────────────────────────

/**
 * Domain tag for trace commitments. Must match `COMMIT_DOMAIN_TAG` in
 * contracts/reasoning-registry/src/lib.rs byte-for-byte; the shared golden
 * vector in trace.test.ts and the Rust tests keeps the two from drifting.
 */
export const COMMIT_DOMAIN_TAG = "oracledesk.trace.commit.v1";

function hex32(value: string, name: string): Buffer {
  const normalized = value.toLowerCase().replace(/^0x/, "");
  if (!/^[0-9a-f]{64}$/.test(normalized)) {
    throw new Error(`${name} must be 32 bytes of hex`);
  }
  return Buffer.from(normalized, "hex");
}

/**
 * Commitment the agent records before acting:
 * `sha256(COMMIT_DOMAIN_TAG || trace_hash || salt)`, as lowercase hex. It
 * fixes the trace content without revealing it until `reveal_trace`.
 */
export function commitmentFor(hashHex: string, saltHex: string): string {
  return createHash("sha256")
    .update(Buffer.from(COMMIT_DOMAIN_TAG, "utf8"))
    .update(hex32(hashHex, "trace hash"))
    .update(hex32(saltHex, "salt"))
    .digest("hex");
}

/**
 * A fresh 32-byte random salt (hex). It must be random and full-length: a
 * short or predictable salt lets anyone brute-force the trace hash from
 * known trace templates. Keep it — losing it makes the reveal impossible.
 */
export function newSalt(): string {
  return randomBytes(32).toString("hex");
}
