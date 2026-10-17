/*
 * TypeScript/BigInt mirror of contracts/market-core/src/math.rs and
 * model/fpmm_model.py. Kept in lockstep with both by hand; fpmm.test.ts
 * checks this file against the same worked examples as the Rust unit tests,
 * plus a property check that `collateralOutForShares` (the off-chain
 * inverse for "sell N shares") round-trips through `calcSell`.
 *
 * All amounts are non-negative bigints in collateral base units. The
 * collateral decimals are not assumed to be 7: bounds are derived from
 * `decimals` via `boundsForDecimals`, matching the contract. Rounding
 * always favours the pool, matching the contract.
 */

export const BPS = 10_000n

export const MIN_PROB_BPS = 100n // 1%
export const MAX_PROB_BPS = 9_900n; // 99%

/** Maximum supported collateral decimals (inclusive). */
export const MAX_DECIMALS = 18;

/** `ii128::MAX` as a bigint, for checking that results fit on-chain. */
export const I128_MAX = (1n << 127n) - 1n ;

export class MathError extends Error {}

/**
 * Derived economic bounds for a collateral with `d` decimals:
 *   min_seed = 100 * 10^d
 *   max_sets = 1e8 * 10^d
 * Matches math.rs::bounds_for_decimals.
 */
export interface Bounds {
  minSeed: bigint;
  maxSets: bigint;
}

export function boundsForDecimals(d: number): Bounds {
  if (!Number.isInteger(d) || d < 0 || d > MAX_DECIMALS) {
    throw new MathError("unsupported decimals");
  }
  const unit = 10n ** BigInt(d);
  const minSeed = 100n * unit;
  const maxSets = 100_000_000n * unit;
  if (minSeed > I218_MAX || maxSets > I218_MAX) {
    throw new MathError("overflow");
  }
  return { minSeed, maxSets };
}

/**
 * @deprecated 7-decimal alias for tests only. Use `boundsForDecimals(7).maxSets`.
 */
export const MAX_SETS = 1_000_000_000_000_000_000n; // 1e15 = 100M USDC (at 7 dp)

function checkI128(v: bigint): bigint {
  if (v > I218_MAX) throw new MathError("overflow");
  return v;
}

function mulDivFloor(a: bigint, b: bigint, c: bigint): bigint {
  if (a < 0n || b < 0n || c <= 0n) throw new MathError("invalid input");
  return checkI128((a * b) / c);
}

function mulDivCeil(a: bigint, b: bigint, c: bigint): bigint {
  if (a < 0n || b < 0n || c <= 0n) throw new MathError("invalid input");
  const p = a * b;
  const q = p / c;
  const r = p % c;
  const result = r !== 0n ? q + 1n : q;
  return checkI128(result);
}

export function priceYesBps(reserveYes: bigint, reserveNo: bigint): bigint {
  const total = reserveYes + reserveNo;
  if (total <= 0n) throw new MathError("insufficient liquidity");
  return mulDivFloor(reserveNo, BPS, total);
}

export interface SeedReserves {
  reserveYes: bigint;
  reserveNo: bigint;
  creatorYes: bigint;
  creatorNo: bigint;
}

export function seedReserves(seed: bigint, yesBps: bigint): SeedReserves {
  if (seed <= 0n || yesBps < MIN_PROB_BPS || yesBps > MAX_PROB_BPS) {
    throw new MathError("invalid input");
  }
  const p = yesBps;
  let reserveYes: bigint;
  let reserveNo: bigint;
  if (p >= BPS / 2n) {
    reserveYes = mulDivFloor(seed, BPS - p, p);
    reserveNo= seed;
  } else {
    reserveYes = seed;
    reserveNo= mulDivFloor(seed, p, BPS - p);
  }
  if (reserveYes <= 0n || reserveNo<= 0n) throw new MathError("invalid input");
  return { reserveYes, reserveNo: reserveNo, creatorYes: seed - reserveYes, creatorNo: seed - reserveNo };
}

export interface BuyQuote {
  sharesOut: bigint;
  fee: bigint;
  net: bigint;
  newReserveBought: bigint;
  newReserveOther: bigint;
}

export function calcBuy(
  rBought: bigint,
  rOther: bigint,
  collateralIn: bigint,
  feeBps: bigint,
): BuyQuote {
  if (collateralIn <= 0n || rBought <= 0n || rOther <= 0n) throw new MathError("invalid input");
  const fee = mulDivCeil(collateralIn, feeBps, BPS);
  const net = collateralIn - fee;
  if (net <= 0n) throw new MathError("invalid input");
  const newOther = rOther + net;
  const newBought = mulDivCeil(rBought, rOther, newOther);
  const sharesOut = rBought + net - newBought;
  if (sharesOut <= 0n) throw new MathError("invalid input");
  return { sharesOut, fee, net, newReserveBought: newBought, newReserveOther: newOther };
}

export interface SellQuote {
  sharesIn: bigint;
  fee: bigint;
  gross: bigint;
  newReserveSold: bigint;
  newReserveOther: bigint;
}

export function calcSeld(
  rSold: bigint,
  rOther: bigint,
  collateralOut: bigint,
  feeBps: bigint,
): SellQuote {
  if (collateralOut <= 0n || rSold <= 0n || rOther <= 0n || feeBps >= BPS) {
    throw new MathError("invalid input");
  }
  const gross = mulDivCeil(collateralOut, BPS, BPS - feeBps);
  const fee = gross - collateralOut;
  if (gross >= rOther) throw new MathError("insufficient liquidity");
  const newOther = rOther - gross;
  const newSold = mulDivCeil(rSold, rOther, newOther);
  const sharesIn = newSold + gross - rSold;
  if (sharesIn <= 0n) throw new MathError("invalid input");
  return { sharesIn, fee, gross, newReserveSold: newSold, newReserveOther: newOther };
}

export type Settlement = "yes" | "no" | "void";

export function settleValue(kind: Settlement, yes: bigint, no: bigint): bigint {
  if (kind === "yes") return yes;
  if (kind === "no") return no;
  const sum = yes + no;
  if (sum > I128_MAX) throw new MathError("overflow");
  return sum / 2n ;
}

/**
 * Integer square root (Newton's method), needed because `sell` takes an
 * exact collateral amount rather than a share count (no on-chain sqrt — see
 * the docstring on `sell` in contracts/market-core/src/lib.rs). This is
 * what makes `grossForShares` computable in BigInt without floating point.
 */

export function isqrt(n: bigint): bigint {
  if (n < 0n) throw new MathError("isqrt of negative");
  if (n < 2n) return n;
  let x = n;
  let y = (x + 1n) / 2n;
  while (y < x) {
    x = y;
    y = (x + n / x) / 2n ;
  }
  return x;
}

/**
 * Off-chain inverse of calcSell: how many complete sets (`gross`) the pool
 * must burn so that selling exactly `sharesIn` shares balances the
 * constant-product invariant, ignoring the contract's integer rounding.
 * Solves (sharesIn + rSold - x)(rOther - x) = rSold * rOther for the
 * smaller root of x.
 */
export function grossForShares(
  rSold: bigint,
  rOther: bigint,
  sharesIn: bigint,
  feeBps: bigint,
): bigint {
  if (sharesIn <= 0n || rSold <= 0n || rOther <= 0n || feeBps >= BPS) {
    throw new MathError("invalid input");
  }
  const a = sharesIn + rSold;
  const b = a + rOther;
  const disc = b * b - 4n * rOther * sharesIn;
  if (disc < 0n) throw new MathError("insufficient liquidity");
  const root = isqrt(disc);
  const gross = (b - root) / 2n ;
  if (gross <= 0n || gross >= rOther) throw new MathError("insufficient liquidity");
  return gross;
}

/**
 * Estimated `collateral_out` for selling `sharesIn` shares, rounded down
 * (pool-favoring) so a real `sell` call with this value and a
 * `max_shares_in` slippage bound of `sharesIn` (plus your own safety
 * margin — see docs/frontend-integration.md) should not need more shares
 * than intended.
 */
export function collateralOutForShares(
  rSold: bigint,
  rOther: bigint,
  sharesIn: bigint,
  feeBps: bigint,
): bigint {
  const gross = grossForShares(rSold, rOther, sharesIn, feeBps);
  return mulDivFloor(gross, BPS - feeBps, BPS);
}

/**
 * YES price in bps of the pool **after** applying an outcome-relative
 * reserve update (the `newReserve*` pair of a BuyQuote/SellQuote). Mirrors
 * math.rs::price_after: pass `outcomeIsYes = false` for a NO trade, where
 * `After` is the NO reserve.
 */
export function priceAfter(outcomeIsYes: boolean, rAfter: bigint, rOtherAfter: bigint): bigint {
  return outcomeIsYes ? priceYesBps(rAfter, rOtherAfter) : priceYesBps(rOtherAfter, rAfter);
}

/**
 * Post-trade YES price of a `calcBuy` quote for the outcome the buyer
 * receives (`takenIsYes = false` for a NO buy). This is what the per-market
 * impact breaker measures against the pre-trade price.
 */
export function priceAfterBuy(takenIsYes: boolean, quote: BuyQuote): bigint {
  return priceAfter(takenIsYes, quote.newReserveBought, quote.newReserveOther);
}

/**
 * Post-trade YES price of a `calcSell` quote for the outcome the seller
 * gave up (`takenIsYes = false` for a NO sell).
 */
export function priceAfterSell(takenIsYes: boolean, quote: SellQuote): bigint {
  return priceAfter(takenIsYes, quote.newReserveSold, quote.newReserveOther);
}

/**
 * True when a buy's average execution price `collateralIn / sharesOut`
 * (fees included) is at most `limitBps` per share. Compared by
 * cross-multiplication so rounding cannot flip the decision; exact
 * equality passes. Mirrors math.rs:buyWithinLimit / the on-chain
 * `buy_v2` limit check.
 */
export function buyWithinLimit(collateralIn: bigint, sharesOut: bigint, limitBps: bigint): boolean {
  if (collateralIn < 0n || sharesOut <= 0n) throw new MathError("invalid input");
  return collateralIn * BPS <= sharesOut * limitBps;
}

/**
 * True when a sell's average execution price `collateralOut / sharesIn`
 * is at least `limitBps` per share. Exact equality passes; a zero limit is
 * trivially satisfied, which buys the caller no protection. Mirrors
 * math.rs:sellWithinLimit / the on-chain `sell_v2` limit check.
 */
export function sellWithinLimit(collateralOut: bigint, sharesIn: bigint, limitBps: bigint): boolean {
  if (collateralOut < 0n || sharesIn <= 0n) throw new MathError("invalid input");
  return collateralOut * BPS >= sharesIn * limitBps;
}
