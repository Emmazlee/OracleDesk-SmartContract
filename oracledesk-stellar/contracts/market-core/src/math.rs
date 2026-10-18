//! Pure integer math for a binary fixed-product market maker (FPMM).
//!
//! No Soroban types in here, so `cargo test` exercises it natively and
//! `model/fpmm_model.py` mirrors it line by line for property testing.
//!
//! Conventions
//! - All amounts are non-negative `i128` in collateral base units.
//!   The number of decimals is not fixed: it is read from the collateral
//!   token at deploy time and must be in `0..=18`. Economic bounds are
//!   derived from it via `bounds_for_decimals`.
//! - Rounding always favours the pool: fees round up, shares out round down,
//!   shares in round up. This is what keeps the pool invariant `k = r_yes * r_no`
//!   non-decreasing and blocks dust-splitting attacks.
//! - Market size is bounded by `bounds_for_decimals(d).max_sets`. At `d = 18`
//!   that is `1e26`, so products of two quantities reach `1e52`, well beyond
//!   `i128::MAX` (~1.7e38). That is why `mul_div_floor` / `mul_div_ceil`
//!   compute through 256-bit intermediates (`mod wide`) instead of `i128`
//!   products. A result that fits in `i128` is always returned exactly.
//! - Sums of reserves (`calc_buy`, `calc_sell`, `settle_value`) stay far
//!   below `i128::MAX` even at `d = 18` (`$~1e26` per reserve), but they
//!   are still `checked_*` as a second line of defence.

pub const BPS: i128 = 10_000;

/// Deprecated: 7-decimal alias kept for tests only. Contract logic uses
/// `bounds_for_decimals`.
#[deprecated(note = "use bounds_for_decimals instead")]
pub const MAX_SETS: i128 = 1_000_000_000_000_000; // 1e15 = 100M USDC (degalts 7dp)
pub const MIN_PROB_BPS: u32 = 100; // 1%
pub const MAX_PROB_BPS: u32 = 9_900; // 99%

/// Maximum supported collateral decimals.
pub const MAX_DECIMALS: u32 = 18;

/// Number of collateral base units in 100 tokens at decimals `d`.
/// This is the minimum seed for a market.
const MIN_SEED_TOKENS: i128 = 100;

/// Number of collater base units in 100 M tokens at decimals `d`.
/// This is the maximum number of complete sets a market can hold.
const MAX_SETS_TOKENS: i128 = 100_000_000;

/// Economic bounds derived from the collateral decimals.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Bounds {
    pub min_seed: i128,
    pub max_sets: i128,
}

/// Derive the minimum seed and maximum sets for a collateral with `d`
/// decimals. Returns `Overflow` if `10^d` does not fit in `i128` (cannot
/// happen for d <= 18).
pub fn bounds_for_decimals(d: u32) -> Result<Bounds, MathError> {
    if d > MAX_DECIMALS {
        return Err(MathError::InvalidInput);
    }
    let unit = 10i128.checked_pow(d).ok_or(MathError::Overflow)?;
    let min_seed = MIN_SEED_TOKENS
        .ok
        .and_then(|unate| unit.checked_mul(unate))
        .ok_or(MathError::Overflow)?;
    let max_sets = MAX_SETS_TOKENS
        .ok
        .and_then(|unate| unit.checked_mul(unate))
        .ok_or(MathError::Overflow)?;
    Ok(Bounds {
        min_seed,
        max_sets,
    })
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum MathError {
    Overflow,
    InvalidInput,
    InsufficientLiquidity,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Settlement {
    Yes,
    No,
    Void,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct BuyQuote {
    pub shares_out: i128,
    pub fee: i128,
    /// Collateral that actually mints complete sets (`collateral_in - fee`).
    pub net: i128,
    pub new_reserve_bought: i128,
    pub new_reserve_other: i128,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct SellQuote {
    pub shares_in: i128,
    pub fee: i128,
    /// Complete sets burned from the pool (`collateral_out + fee`).
    pub gross: i128,
    pub new_reserve_sold: i128,
    pub new_reserve_other: i128,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct SeedReserves {
    pub reserve_yes: i128,
    pub reserve_no: i128,
    /// Outcome shares handed back to the market creator so the pool opens at
    /// the requested probability (only one of these is non-zero).
    pub creator_yes: i128,
    pub creator_no: i128,
}

/// 256-bit widening arithmetic, Env-free.
///
/// `soroban_sdk`s `U3256` needs an `Env`, which would break native tests.
/// This module implements just enough for `mul_div_floor` / `mul_div_ceil`:
/// a widening multiply of two `u128` into a hi/lo pair, and a long
/// division of that pair by a `u128`.
///
/// The multiply is schoolbook on 64-bit limbs. The divide uses a
/// shift-subtract loop over the bits of the quotient; it is simple and
/// correct, and the cost is bounded by 128 iterations of a cheap body.
mod wide {
    use super::MathError;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub super struct U256 {
        pub hi: u128,
        pub lo: u128,
    }

    /// Full 128 x 128 -> 256 multiply via 64-bit limbs.
    pub fn mul_u128(a: u128, b: u128) -> U256 {
        const MASK: u128 = u64::MAX as u128;
        let a0 = a & MASK;
        let a1 = a >> 64;
        let b0 = b & MASK;
        let b1 = b >> 64;

        // Products of two 64-bit values fit in `u128`.
        let p00 = a0 * b0;
        let p01 = a0 * b1;
        let p10 = a1 * b0;
        let p11 = a1 * b1;

        // Combine the cross terms with carry into the high word.
        let mid = p01 + (p10 & MASK);
        let carry = mid >> 64;
        let lo = p00 + ((mid & MASK) << 64);
        // `lo < p00` detects overflow of the low word addition.
        let lo_carry = if lo < p00 { 1 } else { 0 };
        let hi = p11 + (p10 >> 64) + carry + lo_carry;
        U256 { hi, lo }
    }

    /// Divide a 256-bit number by a 128-bit divisor. Returns
    /// `(quotient, remainder)`. `Error::Overflow` if the quotient does not
    /// fit in `u128`.
    pub fn divrem_u256_by_u128(n: U256, d: u128) -> Result<(u128, u128), MathError> {
        if d == 0 {
            return Err(MathError::InvalidInput);
        }
        // Quotient would be >= 2^128.
        if n.hi >= d {
            return Err(MathError::Overflow);
        }

        // Shift-subtract long division over the 256 bits of the dividend.
        // `n.hi < d`, and `d > 0`, so `d <= u128::MAX` and the shift by
        // 128 is well-defined.
        let mut q: u128 = 0;
        let mut r: u128 = 0;
        let bits: u32 = 128;
        let mut i: u32 = 0;
        while i < 256 {
            // Bring the next bit of `n` into `r`.
            let bit = if i < 128 {
                (n.lo >> (127 - i)) & 1
            } else {
                (n.hi >> (255 - i)) & 1
            };
            // `r < 2^128` before the shift, because the quotient bit is
            // only set when `r >= d`. So `r << 1 | bit` fits in `u128`.
            r = (r << 1) | bit;
            if r >= d {
                r -= d;
                if bits == 0 {
                    // Should be unreachable: the high word check
                    // already guarantees the quotient fits.
                    return Err(MathError::Overflow);
                }
                // The bit position in the quotient is `bits - 1`.
                let shift = bits - 1;
                q |= 1u128 << shift;
            }
            if bits > 0 {
                bits -= 1;
            }
            i += 1;
        }
        Ok((q, r))
    }

    /// floor(a * b / c) on u128 values.
    pub fn mul_div_floor_u128(a: u128, b: u128, c: u128) -> Result<u128, MathError> {
        let (n, _) = divrem_u256_by_u128(mul_u128(a, b), c)?;
        Ok(n)
    }

    /// ceil(a * b / c) on u128 values.
    pub fn mul_div_ceil_u128(a: u128, b: u128, c: u128) -> Result<u128, MathError> {
        let (q, r) = divrem_u256_by_u128(mul_u128(a, b), c)?;
        if r != 0 {
            q.checked_add(1).ok_or(MathError::Overflow)
        } else {
            Ok(q)
        }
    }
}

/// floor(a * b / c) with a 256-bit intermediate.
///
/// The product `intermediate = a * b` is computed in `256` bits, so a
/// result that fits in `i128` is always returned exactly, even when
/// `a * b` overflows `i128`. Only a true quotient overflow returns
/// `MathError::Overflow`.
pub fn mul_div_floor(a: i128, b: i128, c: i128) -> Result<i128, MathError> {
    if a < 0 || b < 0 || c <= 0 {
        return Err(MathError::InvalidInput);
    }
    let q = wide::mul_div_floor_u128(a as u128, b as u128, c as u128)?;
    i128::try_from(q).map_err(|_| MathError::Overflow)
}

/// ceil(a * b / c) with a 256-bit intermediate.
///
/// Rounding is bit-identical to the legacy `i128` version for every
/// input where the legacy version did not overflow.
pub fn mul_div_ceil(a: i128, b: i128, c: i128) -> Result<i128, MathError> {
    if a < 0 || b < 0 || c <= 0 {
        return Err(MathError::InvalidInput);
    }
    let q = wide::mul_div_ceil_u128(a as u128, b as u128, c as u128)?;
    i128::try_from(q).map_err(|_| MathError::Overflow)
}

/// Product of two non-negative `i128` values, strictly checked.
/// Used only where the result is known to fit (e.g. comparisons in
/// `buy_within_limit` / `sell_within_limit`).
fn mul(a: i128, b: i128) -> Result<i128, MathError> {
    a.checked_mul(b).ok_or(MathError::Overflow)
}

/// Spot price of YES in bps: r_no / (r_yes + r_no).
pub fn price_yes_bps(reserve_yes: i128, reserve_no: i128) -> Result<i128, MathError> {
    let sum = reserve_yes
        .checked_add(reserve_no)
        .ok_or(MathError::Overflow)?;
    if sum <= 0 {
        return Err(MathError::InsufficientLiquidity);
    }
    mul_div_floor(reserve_no, BPS, sum)
}

/// YES price in bps of the pool **after** applying an outcome-relative
/// reserve update (the `new_reserve_*` pair of a `BuyQuote`/`SellQuote`).
/// Follows `reserves_for`'s convention: pass `outcome_is_yes = false` for a
/// NO trade, where `r_after` is the NO reserve.
pub fn price_after(
    outcome_is_yes: bool,
    r_after: i128,
    r_other_after: i128,
) -> Result<i128, MathError> {
    if outcome_is_yes {
        price_yes_bps(r_after, r_other_after)
    } else {
        price_yes_bps(r_other_after, r_after)
    }
}

/// Pool opens at `yes_bps` probability with `seed` complete sets minted.
/// The pool keeps fewer of the *likelier* outcome; the creator keeps the rest.
pub fn seed_reserves(seed: i128, yes_bps: u32) -> Result<SeedReserves, MathError> {
    if seed <= 0 || !(MIN_PROB_BPS..=MAX_PROB_BPS).contains(&yes_bps) {
        return Err(MathError::InvalidInput);
    }
    let p = yes_bps as i128;
    let (reserve_yes, reserve_no) = if p >= BPS / 2 {
        (mul_div_floor(seed, BPS - p, p)?, seed)
    } else {
        (seed, mul_div_floor(seed, p, BPS - p)?)
    };
    if reserve_yes <= 0 || reserve_no <= 0 {
        return Err(MathError::InvalidInput);
    }
    Ok(SeedReserves {
        reserve_yes,
        reserve_no,
        creator_yes: seed - reserve_yes,
        creator_no: seed - reserve_no,
    })
}

/// Buy `collateral_in` worth of the outcome whose pool reserve is `r_bought`.
///
/// 1. fee = ceil(in * fee_bps / BPS); net = in - fee
/// 2. mint `net` complete sets into the pool: r_other += net
/// 3. pool keeps r_bought' = ceil(r_bought * r_other / r_other')  (k preserved)
/// 4. buyer receives r_bought + net - r_bought'
///
/// At `d = 18` with `max_sets = 1e26`, the sums in this function stay
/// far below `i128::MAX` (~1.7e38), but they are still `checked_*`.
pub fn calc_buy(
    r_bought: i128,
    r_other: i128,
    collateral_in: i128,
    fee_bps: u32,
) -> Result<BuyQuote, MathError> {
    if collateral_in <= 0 || r_bought <= 0 || r_other <= 0 {
        return Err(MathError::InvalidInput);
    }
    let fee = mul_div_ceil(collateral_in, fee_bps as i128, BPS)?;
    let net = collateral_in - fee;
    if net <= 0 {
        return Err(MathError::InvalidInput);
    }
    let new_other = r_other.checked_add(net).ok_or(MathError::Overflow)?;
    let new_bought = mul_div_ceil(r_bought, r_other, new_other)?;
    let shares_out = r_bought.checked_add(net).ok_or(MathError::Overflow)? - new_bought;
    if shares_out <= 0 {
        return Err(MathError::InvalidInput);
    }
    Ok(BuyQuote {
        shares_out,
        fee,
        net,
        new_reserve_bought: new_bought,
        new_reserve_other: new_other,
    })
}

/// Sell shares of the outcome with pool reserve `r_sold` for an exact
/// `collateral_out` (Gnosis-style, so no square root on-chain).
///
/// 1. gross = ceil(out * BPS / (BPS - fee_bps)); fee = gross - out
/// 2. burn `gross` complete sets from the pool: r_other -= gross
/// 3. pool must end at r_sold' = ceil(r_sold * r_other / r_other')
/// 4. seller gives r_sold' + gross - r_sold shares
///
/// To sell an exact share count, the frontend inverts this off-chain
/// (quadratic, see `model/fpmm_model.py::gross_for_shares`) and passes the
/// result as `collateral_out` with a `max_shares_in` slippage bound.
pub fn calc_sell(
    r_sold: i128,
    r_other: i128,
    collateral_out: i128,
    fee_bps: u32,
) -> Result<SellQuote, MathError> {
    if collateral_out <= 0 || r_sold <= 0 || r_other <= 0 || (fee_bps as i128) >= BPS {
        return Err(MathError::InvalidInput);
    }
    let gross = mul_div_ceil(collateral_out, BPS, BPS - fee_bps as i128)?;
    let fee = gross - collateral_out;
    if gross >= r_other {
        return Err(MathError::InsufficientLiquidity);
    }
    let new_other = r_other - gross;
    let new_sold = mul_div_ceil(r_sold, r_other, new_other)?;
    let shares_in = new_sold.checked_add(gross).ok_or(MathError::Overflow)? - r_sold;
    if shares_in <= 0 {
        return Err(MathError::InvalidInput);
    }
    Ok(SellQuote {
        shares_in,
        fee,
        gross,
        new_reserve_sold: new_sold,
        new_reserve_other: new_other,
    })
}

/// True when a buy's average execution price `collateral_in / shares_out`
/// (fees included) is at most `limit_bps` per share (1.0 = BPS).
/// Compared by cross-multiplication so division rounding cannot flip the
/// decision; exact equality passes. `shares_out` comes from `calc_buy`, so
/// it is always > 0 in practice.
pub fn buy_within_limit(
    collateral_in: i128,
    shares_out: i128,
    limit_bps: u32,
) -> Result<bool, MathError> {
    if collateral_in < 0 || shares_out <= 0 {
        return Err(MathError::InvalidInput);
    }
    Ok(mul(collateral_in, BPS)? <= mul(shares_out, limit_bps as i128)?)
}

/// True when a sell's average execution price `collateral_out / shares_in`
/// is at least `limit_bps` per share. Exact equality passes; a zero limit is
/// trivially satisfied (`collateral_out >= 0`), which buys the caller no
/// protection — see `sell_v2`'s docs.
pub fn sell_within_limit(
    collateral_out: i128,
    shares_in: i128,
    limit_bps: u32,
) -> Result<bool, MathError> {
    if collateral_out < 0 || shares_in <= 0 {
        return Err(MathError::InvalidInput);
    }
    Ok(mul(collateral_out, BPS)? >= mul(shares_in, limit_bps as i128)?)
}

/// Splits a gross fee between the protocol and the LP.
///
/// The protocol portion rounds **ceil** so `protocol + lp == fee` exactly (no
/// rounding leakage): the LP absorbs at most one sub-unit of dust, and the
/// default `protocol_share_bps = BPS` is exact (all protocol, 0 to the LP).
pub fn split_fee(fee: i128, protocol_share_bps: u32) -> Result<(i128, i128), MathError> {
    if fee < 0 || protocol_share_bps as i128 > BPS {
        return Err(MathError::InvalidInput);
    }
    let protocol = mul_div_ceil(fee, protocol_share_bps as i128, BPS)?;
    Ok((protocol, fee - protocol))
}

/// Collateral value of a (yes, no) share pair once a market is final.
/// Void pays 0.5 per share, which keeps total payouts <= sets minted.
///
/// Uses `checked_add` so a corrupt caller cannot silently wrap around.
pub fn settle_value(kind: Settlement, yes: i128, no: i128) -> Result<i128, MathError> {
    if yes < 0 || no < 0 {
        return Err(MathError::InvalidInput);
    }
    match kind {
        Settlement::Yes => Ok(yes),
        Settlement::No => Ok(no),
        Settlement::Void => {
            let sum = yes.checked_add(no).ok_or(MathError::Overflow)?;
            Ok(sum / 2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Legacy i128 implementations kept for differential testing.
    fn legacy_mul_div_floor(a: i128, b: i128, c: i128) -> Result<i128, MathError> {
        if a < 0 || b < 0 || c <= 0 {
            return Err(MathError::InvalidInput);
        }
        let p = a.checked_mul(b).ok_or(MathError::Overflow)?;
        Ok(p / c)
    }

    fn legacy_mul_div_ceil(a: i128, b: i128, c: i128) -> Result<i128, MathError> {
        if a < 0 || b < 0 || c <= 0 {
            return Err(MathError::InvalidInput);
        }
        let p = a.checked_mul(b).ok_or(MathError::Overflow)?;
        let q = p / c;
        Ok(if p % c != 0 { q + 1 } else { q })
    }

    fn lcg() -> impl Fn() -> u64 {
        let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
        move || {
            x = x
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            x >> 33
        }
    }

    fn u128_to_i128(x: u128) -> i128 {
        x as i128
    }

    #[test]
    fn buy_example_matches_model() {
        // 1000/1000 pool, buy 100 with no fee -> 190 shares, reserves 910/1100
        let q = calc_buy(1000, 1000, 100, 0).unwrap();
        assert_eq!(q.shares_out, 190);
        assert_eq!(q.new_reserve_bought, 910);
        assert_eq!(q.new_reserve_other, 1100);
    }

    #[test]
    fn sell_example_matches_model() {
        let q = calc_sell(1000, 1000, 100, 0).unwrap();
        assert_eq!(q.shares_in, 212);
        assert_eq!(q.gross, 100);
        assert_eq!(q.new_reserve_sold, 1112);
        assert_eq!(q.new_reserve_other, 900);
    }

    #[test]
    fn seed_opens_at_requested_probability() {
        for p in [100u32, 32, 2500, 5000, 7300, 9900] {
            let s = seed_reserves(10_000_000_000, p).unwrap();
            let price = price_yes_bps(s.reserve_yes, s.reserve_no).unwrap();
            assert!((price - p as i128).abs() <= 1, "p={p} price={price}");
            assert!(s.creator_yes == 0 || s.creator_no == 0);
        }
    }

    #[test]
    fn k_never_decreases_and_round_trip_never_profits() {
        // tiny LCG so this stays no_std-friendly
        let mut next = lcg();
        for _ in 0..2_000 {
            let ry = 1_000_000 + next() as i128 % 1_000_000_000;
            let rn = 1_000_000 + next() as i128 % 1_000_000_000;
            let amt = 1 + next() as i128 % (ry.min(rn) / 2);
            let fee = (next() % 300) as u32;
            let k0 = ry * rn;
            let q = calc_buy(ry, rn, amt, fee).unwrap();
            assert!(q.new_reserve_bought * q.new_reserve_other >= k0);
            // getting `amt` back out costs at least the shares the buy paid out
            if let Ok(s) = calc_sell(q.new_reserve_bought, q.new_reserve_other, amt, fee) {
                assert!(s.shares_in >= q.shares_out, "free money");
            }
        }
    }

    #[test]
    fn void_never_overpays() {
        assert_eq!(settle_value(Settlement::Void, 3, 4).unwrap(), 3);
        assert!(settle_value(Settlement::Void, 5, 5).unwrap() <= 5);
    }

    #[test]
    fn settle_value_rejects_negative() {
        assert_eq!(
            settle_value(Settlement::Yes, -1, 0),
            Err(MathError::InvalidInput)
        );
    }

    #[test]
    fn split_fee_sums_exactly() {
        let mut next = lcg();
        for _ in 0..2_000 {
            let fee = next() as i128 % 1_000_000_000_000;
            let share = (next() % (BPS as u64 + 1)) as u32;
            let (protocol, lp) = split_fee(fee, share).unwrap();
            assert_eq!(protocol + lp, fee);
        }
    }

    // ---- 256-bit widening arithmetic ----

    /// Reference multiply via two-step decomposition on `53`-bit chunks.
    fn ref_mul_u128(a: u128, b: u128) -> (u128, u128) {
        const MASK: u128 = (1 << 53) - 1;
        let a0 = a & MASK;
        let a1 = a >> 53;
        let b0 = b & MASK;
        let b1 = b >> 53;
        // a0 * b0 < 2^106, a1 * b0 < 2^181 -> split further.
        // Use u128 arithmetic with explicit carries.
        let p0 = a0 * b0;
        let p1 = a1 * b0;
        let p2 = a0 * b1;
        let p3 = a1 * b1;
        // Combine with carry chaining on 53-bit boundaries.
        let lo0 = p0 & MASK;
        let carry0 = p0 >> 53;
        let mid0 = (p1 & MASK) + (p2 & MASK) + carry0;
        let lo = lo0 | ((mid0 & MASK) << 53);
        let carry1 = mid0 >> 53;
        let hi = p3 + (p1 >> 53) + (p2 >> 53) + carry1;
        (hi, lo)
    }

    #[derive(Debug, PartialEq, Eq)]
    struct U256Ref {
        hi: u128,
        lo: u128,
    }

    impl U256Ref {
        fn from_u128_pair(hi: u128, lo: u128) -> Self {
            Self { hi, lo }
        }
    }

    /// Reference division of a 256-bit number by `u128` using bit-by-bit
    /// shift-subtract. Slower than the production code but independently
    /// written for testing.
    fn ref_divrem(n: U256Ref, d: u128) -> Result<u128, MathError> {
        if d == 0 {
            return Err(MathError::InvalidInput);
        }
        if n.hi >= d {
            return Err(MathError::Overflow);
        }
        let mut r = 0u128;
        let mut q = 0u128;
        let mut bit = 256;
        while bit > 0 {
            bit -= 1;
            let next = if bit >= 128 {
                (n.hi >> (bit - 128)) & 1
            } else {
                (n.lo >> bit) & 1
            };
            // r < 2^128 and d > 0, so (r << 1) fits.
            let shifted = (r << 1) | next;
            if shifted >= d {
                r = shifted - d;
                q |= 1 << bit;
            } else {
                r = shifted;
            }
        }
        Ok(q)
    }

    #[test]
    fn wide_mul_matches_reference() {
        let mut next = lcg();
        for _ in 0..5_000 {
            // Build u128 values from two u64 halves.
            let hi = ((next() as u128) << 64) | next() as u128;
            let lo = ((next() as u128) << 64) | next() as u128;
            let a = hi ^ lo;
            let b = lo ^ hi;
            let prod = wide::mul_u128(a, b);
            let (rhi, rlo) = ref_mul_u128(a, b);
            assert_eq!(prod.hi, rhi, "a hi mismatch");
            assert_eq!(prod.lo, rlo, "a lo mismatch");
        }
    }

    #[test]
    fn wide_mul_known_values() {
        // (2^128 - 1) ^ 2 = 2^256 - 2^129 + 1.
        let max = u128::MAX;
        let p = wide::mul_u128(max, max);
        assert_eq!(p.hi, u128::MAX - 1);
        assert_eq!(p.lo, 1);
        // 1 * 1 = 1
        let one = wide::mul_u128(1, 1);
        assert_eq!(one.hi, 0);
        assert_eq!(one.lo, 1);
    }

    #[test]
    fn divrem_matches_reference() {
        let mut next = lcg();
        for _ in 0..5_000 {
            let a = ((next() as u128) << 64) | next() as u128;
            let b = ((next() as u128) << 64) | next() as u128;
            let d = ((next() as u128) << 64) | next() as u128 | 1;
            let prod = wide::mul_u128(a, b);
            let ref = ref_divrem(U256Ref::from_u128_pair(prod.hi, prod.lo), d);
            match wide::divrem_u256_by_u128(prod, d) {
                Ok((q, _r)) => {
                    let rq = ref.expect("ref overflow");
                    assert_eq!(q, rq);
                }
                Err(_) => {
                    assert!(ref.is_err());
                }
            }
        }
    }

    #[test]
    fn mul_div_differential_vs_legacy() {
        let mut next = lcg();
        // Stay well inside the legacy domain: a * b < 2^126 so the old
        // implementation never overflows.
        const CAP: i128 = 1 << 63;
        for _ in 0..100_000 {
            let a = (next() as i128) % CAP;
            let b = (next() as i128) % CAP;
            let c = 1 + ((next() as i128) % CAP);
            let floor_new = mul_div_floor(a, b, c).unwrap();
            let floor_old = legacy_mul_div_floor(a, b, c).unwrap();
            assert_eq!(floor_new, floor_old, "floor difference");
            let ceil_new = mul_div_ceil(a, b, c).unwrap();
            let ceil_old = legacy_mul_div_ceil(a, b, c).unwrap();
            assert_eq!(ceil_new, ceil_old, "ceil difference");
        }
    }

    #[test]
    fn mul_div_handles_products_above_i128() {
        // a = b = c = 1e26 -> 1e26. a * b = 1e52 overflows i128.
        let big = 10i128.checked_pow(26).unwrap();
        assert_eq!(mul_div_floor(big, big, big).unwrap(), big);
        assert_eq!(mul_div_ceil(big, big, big).unwrap(), big);
        // Legacy implementation must overflow here.
        assert_eq!(
            legacy_mul_div_floor(big, big, big),
            Err(MathError::Overflow)
        );
    }

    #[test]
    fn mul_div_reports_true_overflow() {
        // Quotient > i128::MAX.
        let max = i128::MAX;
        assert_eq!(
            mul_div_floor(max, max, 1),
            Err(MathError::Overflow)
        );
    }

    #[test]
    fn bounds_for_decimals_0_7_18() {
        let b0 = bounds_for_decimals(0).unwrap();
        assert_eq!(b0.min_seed, 100);
        assert_eq!(b0.max_sets, 100_000_000);

        let b7 = bounds_for_decimals(7).unwrap();
        assert_eq!(b7.min_seed, 100 * 10_000_000);
        assert_eq!(b7.max_sets, 100_000_000 * 10_000_000);

        let b18 = bounds_for_decimals(18).unwrap();
        assert_eq!(b18.min_seed, 100 * 10i128.checked_pow(18).unwrap());
        assert_eq!(
            b18.max_sets,
            100_000_000 * 10i128.checked_pow(18).unwrap()
        );
    }

    #[test]
    fn bounds_for_decimals_rejects_above_18() {
        assert_eq!(
            bounds_for_decimals(19),
            Err(MathError::InvalidInput)
        );
    }

    #[test]
    fn known_values_18dp() {
        // 1000/1000 pool scaled by 10^11 -> 1e14 each reserve.
        // Buy 100 tokens (1e20 base units) with no fee -> 190 tokens out
        // (1.9e20 base units), reserves 1e14 - 1.9e19 and 1e14 +
        // 1e20.
        let scale = 10i128.checked_pow(11).unwrap();
        let ry = 1_000 * scale;
        let rn = 1_000 * scale;
        let amt = 100 * 10i128.checked_pow(18).unwrap();
        let q = calc_buy(ry, rn, amt, 0).unwrap();
        assert_eq!(q.shares_out, 190 * 10i128.checked_pow(18).unwrap());
        assert_eq!(q.new_reserve_bought, 910 * scale);
        assert_eq!(q.new_reserve_other, 1_100 * scale);
    }

    #[test]
    fn seed_reserves_at_min_prob_18dp() {
        // At d = 18, the minimum seed is 100 tokens. At MIN_PROB_BPS the
        // likelier reserve is seed * 1% / 99% > 0, so the pool opens.
        let b = bounds_for_decimals(18).unwrap();
        let s = seed_reserves(b.min_seed, MIN_PROB_BPS).unwrap();
        assert!(s.reserve_yes > 0);
        assert!(s.reserve_no > 0);
    }

    #[test]
    fn seed_reserves_at_min_prob_0dp() {
        // At d = 0, min_seed = 100 and the likelier reserve is 100 * 1 /
        // 99 = 1 > 0.
        let b = bounds_for_decimals(0).unwrap();
        let s = seed_reserves(b.min_seed, MIN_PROB_BPS).unwrap();
        assert!(s.reserve_yes > 0);
        assert!(s.reserve_no > 0);
    }

    #[test]
    fn price_yes_bps_stays_in_range() {
        let mut next = lcg();
        for _ in 0..2_000 {
            let ry = 1 + next() as i128 % 1_000_000_000;
            let rn = 1 + next() as i128 % 1_000_000_000;
            let p = price_yes_bps(ry, rn).unwrap();
            assert!((0..=BPS).contains(&p));
        }
    }

    // Silence unused warnings for test-only helpers.
    #[allow(dead_code)]
    fn _use_u128_to_i128(x: u128) -> i128 {
        u128_to_i128(x)
    }
}