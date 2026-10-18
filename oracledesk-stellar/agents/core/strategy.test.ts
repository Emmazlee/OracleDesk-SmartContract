import assert from "node:assert/strict";
import test from "node:test";
import { calcBuy, priceAfterBuy } from "./fpmm.js";
import { decideMarketProposal, decideTrade } from "./strategy.js";

test("decideMarketProposal skips a signal with no actionable estimate", () => {
  const d = decideMarketProposal(
    { summary: "vague news", source: "test", observedAt: 0 },
    { confidenceThresholdBps: 500, defaultSeedStroops: 1_000_000_000n },
  );
  assert.equal(d.action, "skip");
});

test("decideMarketProposal skips a low-confidence estimate", () => {
  const d = decideMarketProposal(
    {
      summary: "coin flip",
      proposedQuestion: "Will X happen?",
      fairValueBps: 5100,
      source: "test",
      observedAt: 0,
    },
    { confidenceThresholdBps: 500, defaultSeedStroops: 1_000_000_000n },
  );
  assert.equal(d.action, "skip");
});

test("decideMarketProposal proposes a market for a confident estimate", () => {
  const d = decideMarketProposal(
    {
      summary: "strong signal",
      proposedQuestion: "Will X happen?",
      fairValueBps: 7500,
      source: "test",
      observedAt: 0,
    },
    { confidenceThresholdBps: 500, defaultSeedStroops: 1_000_000_000n },
  );
  assert.equal(d.action, "propose_market");
  assert.equal(d.initialYesBps, 7500);
  assert.equal(d.seedAmountStroops, 1_000_000_000n);
});

test("decideTrade skips when the edge is below threshold", () => {
  const d = decideTrade(
    { marketId: 1, question: "q", yesBps: 5000, closeTime: 0 },
    5050,
    { minEdgeBps: 100, tradeSizeStroops: 10_000_000n },
  );
  assert.equal(d.action, "skip");
});

test("decideTrade buys Yes when the market underprices it", () => {
  const d = decideTrade(
    { marketId: 1, question: "q", yesBps: 4000, closeTime: 0 },
    6000,
    { minEdgeBps: 100, tradeSizeStroops: 10_000_000n },
  );
  assert.equal(d.action, "buy");
  assert.equal(d.outcome, "Yes");
});

test("decideTrade buys No when the market overprices Yes", () => {
  const d = decideTrade(
    { marketId: 1, question: "q", yesBps: 8000, closeTime: 0 },
    3000,
    { minEdgeBps: 100, tradeSizeStroops: 10_000_000n },
  );
  assert.equal(d.action, "buy");
  assert.equal(d.outcome, "No");
});

// The 100-sets pool at 50% with a 1% fee used by the calcBuy reference
// example: 10 USDC moves the price 470bps, so a 200bps cap must clamp well
// below the requested size.
const deepMarket = {
  marketId: 1,
  question: "q",
  yesBps: 5000,
  closeTime: 0,
  reserveYesStroops: 1_000_000_000n,
  reserveNoStroops: 1_000_000_000n,
  feeBps: 100,
};

test("decideTrade clamps the size under maxImpactBps", () => {
  const opts = { minEdgeBps: 100, tradeSizeStroops: 100_000_000n, maxImpactBps: 200 };
  const d = decideTrade(deepMarket, 6000, opts);
  assert.equal(d.action, "buy");
  const size = d.collateralInStroops!;
  assert.ok(size > 0n && size < opts.tradeSizeStroops, `size=${size}`);
  // The clamped size respects the cap...
  const q = calcBuy(deepMarket.reserveYesStroops, deepMarket.reserveNoStroops, size, 100n);
  assert.ok(priceAfterBuy(true, q) - 5000n <= 200n);
  // ...and the clamp was actually needed.
  const full = calcBuy(deepMarket.reserveYesStroops, deepMarket.reserveNoStroops, opts.tradeSizeStroops, 100n);
  assert.ok(priceAfterBuy(true, full) - 5000n > 200n);
});

test("decideTrade leaves the size alone when maxImpactBps is 0 (disabled)", () => {
  const d = decideTrade(deepMarket, 6000, {
    minEdgeBps: 100,
    tradeSizeStroops: 100_000_000n,
    maxImpactBps: 0,
  });
  assert.equal(d.action, "buy");
  assert.equal(d.collateralInStroops, 100_000_000n);
});

test("decideTrade under maxImpactBps requires reserves on the market state", () => {
  assert.throws(
    () =>
      decideTrade(
        { marketId: 1, question: "q", yesBps: 5000, closeTime: 0 },
        6000,
        { minEdgeBps: 100, tradeSizeStroops: 1_000_000n, maxImpactBps: 200 },
      ),
    /reserveYesStroops/,
  );
});

test("decideTrade skips when every size would trip the impact cap", () => {
  // A 100-stroop pool: even the smallest executable buy moves the floored
  // price by tens of bps, so a 10bps cap leaves no size at all.
  const d = decideTrade(
    {
      marketId: 1,
      question: "q",
      yesBps: 5000,
      closeTime: 0,
      reserveYesStroops: 100n,
      reserveNoStroops: 100n,
      feeBps: 100,
    },
    6000,
    { minEdgeBps: 100, tradeSizeStroops: 50n, maxImpactBps: 10 },
  );
  assert.equal(d.action, "skip");
});
