import assert from "node:assert/strict";
import test from "node:test";
import { Keypair } from "@stellar/stellar-sdk";
import {
  buildAttestationMessage,
  signAttestation,
  verifyAttestation,
  type AttestationContext,
} from "./attestation.js";

// Shared byte-for-byte with `golden_attestation_message_and_signature` in
// contracts/resolver/src/lib.rs.
const SEED = Uint8Array.from({ length: 32 }, (_, i) => i);
const CTX: AttestationContext = {
  networkPassphrase: "Test SDF Network ; September 2015",
  resolverId: "CC3KU5TWAATOBXLWIV4ZTXNEM44VIOY3ZTR7KTJTBOOBP7YIXMCDSWDJ",
  marketId: 7n,
  round: 3,
  outcome: "No",
  specHash: Uint8Array.from({ length: 32 }, (_, i) => 0xa0 + i),
};
const GOLDEN_MESSAGE =
  "6f7261636c656465736b2d6174746573742d7631cee0302d59844d32bdca915c8203dd44b33fbb7edc19051ea37abedf28ecd4720000001200000001b6aa76760026e0dd76457999dda46739543b1bcce3f54d330b9c17ff08bb043900000000000000070000000301a0a1a2a3a4a5a6a7a8a9aaabacadaeafb0b1b2b3b4b5b6b7b8b9babbbcbdbebf";
const GOLDEN_SIGNATURE =
  "3f7bfc9882bbe894518d059b50326069523358f3da1d74b7ed90b8b395862367155ae6ac7934b3ae610e21c3dd595b7d99c2ab122037918917c96543c0cde002";
const GOLDEN_PUBKEY = "03a107bff3ce10be1d70dd18e74bc09967e4d6309ba50d5f1ddc8664125531b8";

const hex = (bytes: Uint8Array) => Buffer.from(bytes).toString("hex");
const keypair = () => Keypair.fromRawEd25519Seed(Buffer.from(SEED));

test("buildAttestationMessage matches the golden vector", () => {
  assert.equal(hex(buildAttestationMessage(CTX)), GOLDEN_MESSAGE);
});

test("signAttestation matches the golden signature (Ed25519 is deterministic)", () => {
  const { pubkey, sig } = signAttestation(keypair(), CTX);
  assert.equal(hex(pubkey), GOLDEN_PUBKEY);
  assert.equal(hex(sig), GOLDEN_SIGNATURE);
});

test("verifyAttestation accepts the golden signature", () => {
  const { pubkey, sig } = signAttestation(keypair(), CTX);
  assert.equal(verifyAttestation(pubkey, sig, CTX), true);
});

test("verifyAttestation rejects any tampered context field", () => {
  const { pubkey, sig } = signAttestation(keypair(), CTX);
  const tampered: AttestationContext[] = [
    { ...CTX, marketId: 8n },
    { ...CTX, round: 4 },
    { ...CTX, outcome: "Yes" },
    { ...CTX, networkPassphrase: "Public Global Stellar Network ; September 2015" },
    { ...CTX, resolverId: "CCILJXZCHKAIBTMJ7NSQE756OGLKTOYNSWNNZXWAYZZA4ECAUEC2CNVF" },
    { ...CTX, specHash: Uint8Array.from({ length: 32 }, () => 1) },
  ];
  for (const ctx of tampered) {
    assert.equal(verifyAttestation(pubkey, sig, ctx), false);
  }
});

test("verifyAttestation rejects a malleated signature (S + group order)", () => {
  const { pubkey, sig } = signAttestation(keypair(), CTX);
  const order = (1n << 252n) + 27742317777372353535851937790883648493n;
  const s = BigInt(`0x${Buffer.from(sig.slice(32)).reverse().toString("hex")}`) + order;
  const malleated = Uint8Array.from(sig);
  malleated.set(Buffer.from(s.toString(16).padStart(64, "0"), "hex").reverse(), 32);
  assert.equal(verifyAttestation(pubkey, malleated, CTX), false);
});

test("verifyAttestation rejects malformed lengths", () => {
  const { pubkey, sig } = signAttestation(keypair(), CTX);
  assert.equal(verifyAttestation(pubkey.slice(1), sig, CTX), false);
  assert.equal(verifyAttestation(pubkey, sig.slice(1), CTX), false);
});

test("buildAttestationMessage validates its inputs", () => {
  assert.throws(() => buildAttestationMessage({ ...CTX, specHash: new Uint8Array(31) }));
  assert.throws(() => buildAttestationMessage({ ...CTX, round: -1 }));
  assert.throws(() => buildAttestationMessage({ ...CTX, round: 2 ** 32 }));
});
