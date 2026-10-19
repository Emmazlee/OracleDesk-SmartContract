import { createHash } from "node:crypto";

export interface TraceRecord {
  traceId: string;
  ipfsCid: string;
  traceHash: string;
}

export function sha256Hex(content: Uint8Array): string {
  return createHash("sha256").update(content).digest("hex");
}

export function verifyTraceHash(content: Uint8Array, expectedHash: string): boolean {
  const normalized = expectedHash.toLowerCase().replace(/^0x/, "");
  return /^[0-9a-f]{64}$/.test(normalized) && sha256Hex(content) === normalized;
}

export function ipfsUrl(gateway: string, cid: string): string {
  const base = gateway.endsWith("/") ? gateway.slice(0, -1) : gateway;
  return `${base}/${encodeURIComponent(cid)}`;
}

// ─── Hash-chained traces (#52) ───────────────────────────────────────────────

/** One link of an agent's on-chain trace log, as read from the registry. */
export interface ChainEntry {
  seq: bigint;
  traceHash: string;
  prevTraceHash: string;
}

export type ChainVerdict =
  | { ok: true }
  | { ok: false; reason: "gap" | "broken-link" | "bad-genesis"; atSeq: bigint };

const GENESIS_HASH = "0".repeat(64);

function normalizeHash(hash: string): string {
  return hash.toLowerCase().replace(/^0x/, "");
}

/**
 * Walks a contiguous range of an agent's chain and reports the first
 * problem: a missing sequence number (`gap`), a link whose `prevTraceHash`
 * isn't the previous entry's hash (`broken-link`), or — when the range is
 * meant to start at the beginning — a first entry that isn't seq 1 on the
 * all-zero genesis hash (`bad-genesis`). Entries must be sorted by `seq`.
 */
export function verifyChain(entries: ChainEntry[], fromGenesis: boolean): ChainVerdict {
  if (entries.length === 0) return { ok: true };

  const first = entries[0];
  if (fromGenesis && (first.seq !== 1n || normalizeHash(first.prevTraceHash) !== GENESIS_HASH)) {
    return { ok: false, reason: "bad-genesis", atSeq: first.seq };
  }

  for (let i = 1; i < entries.length; i += 1) {
    const prev = entries[i - 1];
    const cur = entries[i];
    if (cur.seq !== prev.seq + 1n) {
      return { ok: false, reason: "gap", atSeq: prev.seq + 1n };
    }
    if (normalizeHash(cur.prevTraceHash) !== normalizeHash(prev.traceHash)) {
      return { ok: false, reason: "broken-link", atSeq: cur.seq };
    }
  }
  return { ok: true };
}
