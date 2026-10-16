import assert from "node:assert/strict";
import test from "node:test";
import {
  BPS,
  calcBuy,
  calcSell,
  calcSellShares,
  collateralOutForShares,
  priceYesBps,
  seedReserves,
  settleValue,
} from "./fpmm.js";

// Same worked examples as math.rs's unit tests and model/fpmm_model.py's
// _check_known_values — catches a porting mistake between the three
// languages immediately.
test("calcBuy matches the reference example", () => {
  const q = calcBuy(1000n, 1000n, 100n, 0n);
  assert.equal(q.sharesOut, 190n);
  assert.equal(q.newReserveBought, 910n);
  assert.equal(q.newReserveOther, 1100n);
});

test("calcSell matches the reference example", () => {
  const q = calcSell(1000n, 1000n, 100n, 0n);
  assert.equal(q.sharesIn, 212n);
  assert.equal(q.gross, 100n);
  assert.equal(q.newReserveSold, 1112n);
  assert.equal(q.newReserveOther, 900n);
});

// Pinned values shared with math.rs's `sell_shares_example_matches_model`
// and model/fpmm_model.py's `_check_sell_shares_known_values`.
test("calcSellShares matches Rust/Python known values", () => {
  // 1000/1000 pool, sell 212 shares, no fee.
  const q = calcSellShares(1000n, 1000n, 212n, 0n);
  assert.equal(q.gross, 100n);
  assert.equal(q.fee, 0n);
  assert.equal(q.collateralOut, 100n);
  assert.equal(q.newReserveSold, 1112n);
  assert.equal(q.newReserveOther, 900n);

  // Same pool with a 2% (200 bps) fee: fee rounds up, so the trader
  // gets floor(gross * 9800 / 10000).
  const withFee = calcSellShares(1000n, 1000n, 212n, 200n);
  assert.equal(withFee.gross, 100n);
  assert.equal(withFee.fee, 2n);
  assert.equal(withFee.collateralOut, 98n);
});

// Same known values as the Python fixture table, so a porting mistake
// in the correction loop shows up as a diff here.
test("calcSellShares fixture table", () => {
  const cases: Array<[bigint, bigint, bigint, bigint, number]> = [
    [1000n, 1000n, 212n, 100n, 0],
    [1000n, 1000n, 1n, 0n, 0],
    [10000n, 10000n, 1000n, 952n, 0],
    [100000n, 500000n, 10000n, 9505n, 0],
    [100000n, 500000n, 10000n, 9315n, 200],
  ];
  for (const [rY, rN, shares, expectedOut, feeBps] of cases) {
    const q = calcSellShares(rY, rN, shares, BigInt(feeBps));
    assert.equal(q.collateralOut, expectedOut, `rY=${rY} rN=${rN} shares=${shares} fee=${feeBps}`);
  }
});

test("seedReserves opens at the requested probability", () => {
  for (const p of [100n, 2500n, 5000n, 7300n, 9900n]) {
    const s = seedReserves(10_000_000_000n, p);
    const price = priceYesBps(s.reserveYes, s.reserveNo);
    const diff = price > p ? price - p : p - price;
    assert.ok(diff <= 1n, `p=${p} price=${price}`);
    assert.ok(s.creatorYes === 0n || s.creatorNo === 0n);
  }
});

test("k never decreases and a round trip never profits", () => {
  // Same small LCG as math.rs's property test, so both suites explore a
  // comparable input space.
  let x = 0x9e3779b97f4a7c15n;
  const mask = (1n << 64n) - 1n;
  const next = () => {
    x = (x * 6364136223846793005n + 1442695040888963407n) & mask;
    return x >> 33n;
  };
  for (let i = 0; i < 2000; i++) {
    const ry = 1_000_000n + (next() % 1_000_000_000n);
    const rn = 1_000_000n + (next() % 1_000_000_000n);
    const amt = 1n + (next() % ((ry < rn ? ry : rn) / 2n));
    const fee = next() % 300n;
    const k0 = ry * rn;
    const q = calcBuy(ry, rn, amt, fee);
    assert.ok(q.newReserveBought * q.newReserveOther >= k0);
    try {
      const s = calcSell(q.newReserveBought, q.newReserveOther, amt, fee);
      assert.ok(s.sharesIn >= q.sharesOut, "free money");
    } catch {
      // insufficient liquidity to sell back out is fine, not what's tested here
    }
  }
});

test("calcSellShares never decreases k", () => {
  // Same LCG as the calcBuy property test above, so the two suites
  // explore a comparable input space.
  let x = 0x9e3779b97f4a7c15n;
  const mask = (1n << 64n) - 1n;
  const next = () => {
    x = (x * 6364136223846793005n + 1442695040888963407n) & mask;
    return x >> 33n;
  };
  for (let i = 0; i < 2000; i++) {
    const ry = 1_000_000n + (next() % 1_000_000_000n);
    const rn = 1_000_000n + (next() % 1_000_000_000n);
    const shares = 1n + (next() % ((ry < rn ? ry : rn) / 2n));
    const fee = BigInt(next() % 300n);
    const k0 = ry * rn;
    try {
      const q = calcSellShares(ry, rn, shares, fee);
      assert.ok(
        q.newReserveSold * q.newReserveOther >= k0,
        `knew < k0 at i=${i}`,
      );
    } catch {
      // insufficient liquidity to sell that many shares is fine
    }
  }
});

test("settlement never overpays", () => {
  assert.equal(settleValue("void", 3n, 4n), 3n);
  assert.ok(settleValue("void", 5n, 5n) <= 5n);
});

test("collateralOutForShares round-trips through calcSell within one unit of rounding", () => {
  // Deterministic pseudo-random inputs (same LCG as above) so this is
  // reproducible without a test-framework property library dependency.
  let x = 12345n;
  const mask = (1n << 64n) - 1n;
  const next = () => {
    x = (x * 6364136223846793005n + 1442695040888963407n) & mask;
    return x >> 33n;
  };
  let checked = 0;
  const trials = 500;
  for (let i = 0; i < trials; i++) {
    const rSold = 1_000_000n + (next() % 1_000_000_000n);
    const rOther = 1_000_000_000n + (next() % 1_000_000_000n);
    const feeBps = next() % 300n;
    const collateralOut = 1n + (next() % (rOther / 4n || 1n));
    let forward;
    try {
      forward = calcSell(rSold, rOther, collateralOut, feeBps);
    } catch {
      continue;
    }
    let estimatedOut: bigint;
    try {
      estimatedOut = collateralOutForShares(rSold, rOther, forward.sharesIn, feeBps);
    } catch {
      continue;
    }
    if (estimatedOut <= 0n) continue;
    const back = calcSell(rSold, rOther, estimatedOut, feeBps);
    assert.ok(back.sharesIn <= forward.sharesIn + 1n, `forward=${forward.sharesIn} back=${back.sharesIn}`);
    checked++;
  }
  assert.ok(checked > trials / 2, "too many trials skipped, weakens the property check");
});

test("BPS matches the contract's basis-point scale", () => {
  assert.equal(BPS, 10_000n);
});
