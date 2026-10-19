import assert from "node:assert/strict";
import { test } from "node:test";
import { createHash } from "node:crypto";
import { ipfsUrl, verifyChain, verifyTraceHash, type ChainEntry } from "./trace-verification.js";

test("verifies the exact bytes received from IPFS", () => {
  const content = new TextEncoder().encode('{"action":"buy","confidence":0.72}');
  const hash = createHash("sha256").update(content).digest("hex");

  assert.equal(verifyTraceHash(content, hash), true);
  assert.equal(verifyTraceHash(content, `0x${hash}`), true);
  assert.equal(verifyTraceHash(new TextEncoder().encode("{}"), hash), false);
  assert.equal(verifyTraceHash(content, "not-a-hash"), false);
});

test("builds an encoded IPFS gateway URL", () => {
  assert.equal(
    ipfsUrl("https://ipfs.example/", "bafy trace"),
    "https://ipfs.example/bafy%20trace",
  );
});

// ─── Hash-chained traces (#52) ───────────────────────────────────────────────


const h = (n: number) => n.toString(16).padStart(2, "0").repeat(32);
const ZERO = "0".repeat(64);

function chain(n: number): ChainEntry[] {
  return Array.from({ length: n }, (_, i) => ({
    seq: BigInt(i + 1),
    traceHash: h(i + 1),
    prevTraceHash: i === 0 ? ZERO : h(i),
  }));
}

test("verifyChain accepts a valid chain from genesis", () => {
  assert.deepEqual(verifyChain(chain(4), true), { ok: true });
  assert.deepEqual(verifyChain([], true), { ok: true });
});

test("verifyChain accepts a valid middle range without genesis", () => {
  assert.deepEqual(verifyChain(chain(5).slice(2), false), { ok: true });
});

test("verifyChain detects a gap", () => {
  const entries = chain(4);
  entries.splice(2, 1); // drop seq 3
  assert.deepEqual(verifyChain(entries, true), { ok: false, reason: "gap", atSeq: 3n });
});

test("verifyChain detects a broken link", () => {
  const entries = chain(3);
  entries[2] = { ...entries[2], prevTraceHash: h(9) };
  assert.deepEqual(verifyChain(entries, true), { ok: false, reason: "broken-link", atSeq: 3n });
});

test("verifyChain detects a bad genesis", () => {
  assert.deepEqual(verifyChain(chain(4).slice(1), true), { ok: false, reason: "bad-genesis", atSeq: 2n });
  const forged = chain(2);
  forged[0] = { ...forged[0], prevTraceHash: h(7) };
  assert.deepEqual(verifyChain(forged, true), { ok: false, reason: "bad-genesis", atSeq: 1n });
});
