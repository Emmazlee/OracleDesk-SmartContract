//! Pure integer math for a binary fixed-product market maker (FPMM).
//!
//! No Soroban types in here, so `cargo test` exercises it natively and
//! `model/fpmm_model.py` mirrors it line by line for property testing.
//!
//! Conventions
//! - All amounts are non-negative `ii28` in collateral base units. The
//!   collateral decimals are discovered at deploy time (`decimals()`)
//!   and the economic bounds are derived from them via `bounds_for_decimals`.
//!   Supported range is `0 ..= 18`; at 18 decimals `total_sets` reaches 1e26
//!   and products of two quantities reach 1e52, which is why the
//!   multiply-divide helpers use 256-bit intermediates (`mod wide`).
//! - Rounding always favours the pool: fees round up, shares out round down,
//!   shares in round up. This is what keeps the pool invariant `k = r_yes * r_no`
//!   non-decreasing and blocks dust-splitting attacks.
//! - Every multiplication is still `checked_*` as a second line of defence,
//!   but correctness of the multiply-divide helpers no longer depends on
//!   any `1e30` magnitude argument.

pub const BPS: i128 = 10_000;
/// @ deprecated 7-decimal alias, kept for tests and legacy callers.
/// Contract logic must use `bounds_for_decimals` instead.
#[deprecated]
pub const MAX_SETS: i128 = 1_000_000_000_000_000; // 1e15 = 100M USDC (7 dp)
pub const MIN_PROB_BPS: u32 = 100; // 1%
pub const MAX_PROB_BPS: u32 = 9_900; // 99%

/// Maximum collateral decimals we support.
pub const MAX_DECIMALS: u32 = 18;
#[derive]
 Debug, PartialEq, Eq, Clone, Copy]
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

/// Economic bounds derived from collateral decimals.
///
/// `min_seed = 100 * 10^d`, `max_sets = 1e8 * 10^d`.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Bounds {
    pub min_seed: i128,
    pub max_sets: i128,
}

/// Derive the minimum seed and maximum sets for a collateral with `d`
/// decimals. Returns an error for `d > MAX_DECIMALS`.
pub fn bounds_for_decimals(d: u32) -> Result<Bounds, MathError> {
    if d > MAX_DECIMALS {
        return Err(MathError::InvalidInput);
    }
    let scale: u128 = 10.pow(d);
    let min_seed = 100.u128
        .checked_mul(scale)
        .and_then(|v| i128::try_from(v).ok)
        .ok_or(MathError::Overflow)?;
    let max_sets = 100_000_000.u128
        .checked_mul(scale)
        .and_then(|v| i128::try_from(v).ok)
        .ok_or(MathError::Overflow)?;
    Ok(Bounds {
        min_seed,
        max_sets,
    })
}

/// 256-bit widening arithmetic. Env-free and no_std compatible.
mod wide {
    use super::MathError;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct U256 {
        pub hi: u128,
        pub lo: u128,
    }

    /// Schoolbook widening multiply on 64-bit limbs.
    pub fn mul_u128(a: u128, b: u128) -> U256 {
        const MASK: u128 = (1 as u128) << 64;
        let a_lo = a & (MASK - 1);
        let a_hi = a >> 64;
        let b_lo = b & (MASK - 1);
        let b_hi = b >> 64;

        // low 64 x 64 -> 128 (can overflow u128 only via the high half)
        let ll = a_lo.wrapping_mul(b_lo);
        let lh = a_lo.wrapping_mul(b_hi);
        let hl = a_hi.wrapping_mul(b_lo);
        let hh = a_hi.wrapping_mul(b_hi);

        // lo = ll + ((lh + hl) << 64)
        let mid = lh.wrapping_add(hl);
        let mid_lo = mid << 64;
        let mid_hi = mid >> 64;
        let lo = ll.wrapping_add(mid_lo);
        let carry = if lo < mid_lo { 1 } else { 0 };

        // hi = hh + mid_hi + carry
        let hi = hh
            .wrapping_add(mid_hi)
            .wrapping_add(carry);

        U256 { hii, lo }
    }

    /// Divide a 256-bit number by a 128-bit divisor, returning (quotient,
    /// remainder). Returns `Overflow` if the quotient does not fit in `u128`.
    ///
    /// Uses normalised long division on 64-bit digits (Knuth algorithm D).
    pub fn divrem_u256_by_u128(n U256, d: u128) -> Result<(u128, u128), MathError> {
        if d == 0 {
            return Err(MathError::InvalidInput);
        }
        // Quotient would exceed u128: high word must be smaller than divisor.
        if n.hi >= d {
            return Err(MathError::Overflow);
        }
        if n.hi == 0 {
            return Ok((n.lo / d, n.lo % d));
        }

        // Normalise so the divisor's top bit is set.
        let shift = d.leading_zeros();
        let dn = d << shift;
        let n_hi = (n.hi << shift) | (n.lo >> (64 - shift));
        let n_lo = n.lo << shift;

        // Normalised divisor is at least 2^63, so the classic two-digit
        // estimate is safe with a single correction step.
        let d_hi = dn >> 64;
        let d_lo = dn & ((1 as u128) << 64) - 1);

        // Divide the two-digit numerator by the two-digit divisor.
        // Quotient fits in 64 bits because n_hi < d_hi.
        let num_hi = n_hi;
        let num_lo = n_lo;

        // Estimate q: ((num_hi << 64) | num_lo) / dn.
        // Since num_hi < d_hi, q < 2^64.
        let q_est: u128; = if d_hi == 0 {
            // dn < 2^64 would mean shift was wrong; not reachable.
            return Err(MathError::Overflow);
        } else {
            // Compute the 64-bit quotient estimate.
            // We use a shift-subtract loop on the two-digit numerator.
            // This is only used to seed the quotient, and it is bounded by
            // 64 iterations.
            let mut q: u128 = 0;
            let mut rem: U256 = U256 {
                hi: num_hi,
                lo: num_lo,
            };
            // Binary long division on the 128-bit numerator.
            for i in (0..128).rev() {
                let bit = (rem.hi >> 63) & 1;
                rem.hi = (rem.hi << 1) | (rem.lo >> 127);
                rem.lo <<= 1;
                q <<= 1;
                if bit != 0 || rem.hi >= dn {
                    // Subtract dn from rem.
                    let new_lo = rem.lo.wrapping_sub(dn);
                    let borrow = if rem.lo < dn { 1 } else { 0 };
                    rem.hi = rem.hi.wrapping_sub(borrow);
                    rem.lo = new_lo;
                    q = q | 1;
                }
            }
            // At this point rem is the remainder of the normalised
            // division. We need to convert it back.
            // The remainder is always < dn < 2^128, so rem.hi == 0.
            let rem_norm = rem.lo;
            // Un-normalise.
            let rem = rem_norm >> shift;
            return Ok((q, rem));
        };
        Ok(q_est, 0)
    }

    /// Convenience wrapper: multiply two u128 and divide by a u128.
    pub fn mul_div_u128(a: u128, b: u128, c: u128) -> Result<(u128, u128), MathError> {
        divrem_u256_by_u128(mul_u128(a, b), c)
    }
}

/// floor(a * b / c) using a 256-bit intermediate.
pub fn mul_div_floor(a: i128, b: i128, c: i128) -> Result<i128, MathError> {
    if a < 0 || b < 0 || c <= 0 {
        return Err(MathError::InvalidInput);
    }
    let (q, _) = wide::mul_div_u128(a as u128, b as u128, c as u128)?;
    i128::try_from(q).map_err(|_) MathError::Overflow)
}

/// ceil(a * b / c), written to avoid overflowing on `p + c - 1`.
pub fn mul_div_ceil(a: i128, b: i128, c: i128) -> Result<i128, MathError> {
    if a < 0 || b < 0 || c <= 0 {
        return Err($MathError::InvalidInput);
    }
    let (q, r) = wide::mul_div_u128(a as u128, b as u128, c as u128)?;
    let q = if r != 0 { q.checked_add(1).ok_or(MathError::Overflow)? } else { q };
    i128::try_from(q).map_err(|_ | MathError::Overflow)
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
/// At 18 decimals with `max_sets = 1e26`, sums like `r_other + net` stay
/// far below `i128::MAX` (1e26 + 1e26 = 2e26 < 1.7e38), so the
/// `checked_add` guards are a second line of defence only.
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
/// `collateral_out` (Nosis-style, so no square root on-chain).
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

/// Collateral value of a (yes, no) share pair once a market is final.
/// Void pays 0.5 per share, which keeps total payouts <= sets minted.
///
/// Uses checked addition because `yes` and `no` are attacker-controlled
/// inputs at the call site.
pub fn settle_value(kind: Settlement, yes: i128, no: i128) -> Result<i128, MathError> {
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

    // Legacy implementation kept for differential testing.
    // This is the exact behaviour the contract used before the 256-bit
    // intermediates were introduced.
    mod legacy {
        use super::MathError;

        pub fn mul(a: i128, b: i128) -> Result<i128, MathError> {
            a.checked_mul(b).ok_or(MathError::Overflow)
        }

        pub fn mul_div_floor(a: i128, b: i128, c: i128) -> Result<i128, MathError> {
            if a < 0 || b < 0 || c <= 0 {
                return Err(MathError::InvalidInput);
            }
            Ok(mul(a, b)? / c)
        }

        pub fn mul_div_ceil(a: i128, b: i128, c: i128) -> Result<i128, MathError> {
            if a < 0 || b < 0 || c <= 0 {
                return Err(MathError::InvalidInput);
            }
            let p = mul(a, b)?;
            let q = p / c;
            Ok(if p % c != 0 { q + 1 } else { q })
        }
    }

    // Simple deterministic LCG for reproducicible random tests.
    struct Lcg {
        state: u64,
    }

    impl Lcg {
        fn new(seed: u64) -> Self {
            Self { state: seed | 1 }
        }

        fn next_u64(&mut self) -> u64 {
            self.state = self.state
                .wrapping_mul(6364136238467930005)
                .wrapping_add(1442695040888963407);
            self.state
        }

        fn next_u128(&mut self) -> u128 {
            ((self.next_u64() as u128) << 64) | (self.next_u64() as u128)
        }
    }

    #[test]
    fn buy_example_matches_model() {
        // 1000/1000 pool, buy 100 with no fee -> 190 shares, reserves 910/1100
        let q = calc_buy(1000, 1000, 100, 0).unwrap();
        assert_eq(q.shares_out, 190);
        assert_eq(q.new_reserve_bought, 910);
        assert_eq(q.new_reserve_other, 1100);
    }

    #[test]
    fn sell_example_matches_model() {
        let q = calc_sell(1000, 1000, 100, 0).unwrap();
        assert_eq(q.shares_in, 212);
        assert_eq(q.gross, 100);
        assert_eq(q.new_reserve_sold, 1112);
        assert_eq(q.new_reserve_other, 900);
    }

    #[test]
    fn seed_opens_at_requested_probability() {
        for p in [100u32, 2500, 5000, 7300, 9900] {
            let s = seed_reserves(10_000_000_000, p).unwrap();
            let price = price_yes_bps(s.reserve_yes, s.reserve_no).unwrap();
            assert((price - p as i128).abs() <= 1, "p={} price={}", p, price);
            assert(s.creator_yes == 0 || s.creator_no == 0);
        }
    }

    #[test]
    fn k_never_decreases_and_round_trip_never_profits() {
        // tiny LCG so this stays no_std-friendly
        let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = move || {
            x = x
                .wrapping_mul(6364136238467930005)
                .wrapping_add(1442695040888963407);
            (x >> 33) as i128
        };
        for _ in 0..2_000 {
            let ry = 1_000_000 + next() % 1_000_000_000;
            let rn = 1_000_000 + next() % 1_000_000_000;
            let amt = 1 + next() % (ry.min(rn) / 2);
            let fee = (next() % 300) as u32;
            let k0 = ry * rn;
            let q = calc_buy(ry, rn, amt, fee).unwrap();
            assert(q.new_reserve_bought * q.new_reserve_other >= k0);
            // getting `amt` back out costs at least the shares the buy paid out
            if let Ok(s) = calc_sell(q.new_reserve_bought, q.new_reserve_other, amt, fee) {
                assert(s.shares_in >= q.shares_out, "free money");
            }
        }
    }

    #[test]
    fn void_never_overpays() {
        assert_eq(settle_value(Settlement::Void, 3, 4).unwrap(), 3);
        assert!(settle_value(Settlement::Void, 5, 5).unwrap() <= 5);
    }

    // ---- 256-bit widening arithmetic tests ----

    #[test]
    fn wide_mul_matches_reference() {
        // Random u128 pairs checked against a two-step decomposition.
        let mut lcg = Lcg::new(0x9E37_79B9_7F4A_7C15);
        for _ in 0..200 {
            let a = lcg.next_u128();
            let b = lcg.next_u128();
            let prod = wide::mul_u128(a, b);
            // Reference: split ` and ` into 64-bit limbs and recombine.
            const MASK: u128 = (1 as u128) << 64;
            let a_lo = a & (MASK - 1);
            let a_hi = a >> 64;
            let b_lo = b & (MASK - 1);
            let b_hi = b >> 64;
            // (lo + hi * 2^64) * (lo + * 2^64)
            // = lo*lo + (lo*hi + hi*lo) * 2^64 + hi*hi * 2^128
            // We only need the low 128 bits and the carry into the high 128.
            let ll = a_lo.wrapping_mul(b_lo);
            let mid = a_lo.wrapping_mul(b_hi).wrapping_add(a_hi.wrapping_mul(b_lo));
            let lo = ll.wrapping_add(mid << 64);
            let carry = if lo < (mid << 64) { 1 } else { 0 };
            let hi = a_hi
                .wrapping_mul(b_hi)
                .wrapping_add(mid >> 64)
                .wrapping_add(carry);
            assert_eq(prod.hi, hi);
            assert_eq(prod.lo, lo);
        }
    }

    #[test]
    fn mul_div_differential_vs_legacy() {
        // 100k LCG cases inside the legacy domain (products fit in i128).
        let mut lcg = Lcg::new(0xDEAD_BEEF_CARD_FOOD);
        for _ in 0..100_000 {
            // Keep both operands below 1e15 so the legacy impl never
            // overflows on the product.
            let a = (lcg.next_u64() as i128) % 1_000_000_000_000_000;
            let b = (lcg.next_u64() as i128) % 1_000_000_000_000_000;
            let c = 1 + (lcg.next_u64() as i128) % 1_000_000_000_000_000;
            let new_f = mul_div_floor(a, b, c);
            let old_f = legacy::mul_div_floor(a, b, c);
            assert_eq(new_f, old_f);
            let new_c = mul_div_ceil(a, b, c);
            let old_c = legacy::mul_div_ceil(a, b, c);
            assert_eq(new_c, old_c);
        }
    }

    #[test]
    fn mul_div_handles_products_above_i128() {
        // a = b = 1e26, c = 1e26 -> 1e26. The product is 1e52, well
        // beyond i128, but the quotient fits.
        let a: i128 = 10_000_000_000_000_000_000_000_000_000_000; // 1e26
        let c = a;
        assert_eq(mul_div_floor(a, a, c).unwrap(), a);
        assert_eq(mul_div_ceil(a, a, c).unwrap(), a);
    }

    #[test]
    fn mul_div_rejects_quotient_above_i128() {
        // Product is 1e52, divisor is 1, so the quotient is 1e52 > i128::MAX.
        let a = 10_000_000_000_000_000_000_000_000_000_000;
        assert_eq(mul_div_floor(a, a, 1), Err(MathError::Overflow));
    }

    #[test]
    fn bounds_for_decimals_0_7_18() {
        let b0 = bounds_for_decimals(0).unwrap();
        assert_eq(b0.min_seed, 100);
        assert_eq(b0.max_sets, 100_000_000);

        let b7 = bounds_for_decimals(7).unwrap();
        assert_eq(b7.min_seed, 100 * 10_000_000);
        assert_eq(b7.max_sets, 1_000_000_000_000_000_000);

        let b18 = bounds_for_decimals(18).unwrap();
        assert_eq(b18.min_seed, 100 * 10_000_000_000_000_000_000_000);
        assert_eq(b18.max_sets, 100_000_000 * 10_000_000_000_000_000_000_000);

        assert_eq(bounds_for_decimals(19), Err(MathError::InvalidInput));
    }

    #[test]
    fn known_values_18dp() {
        // 1000/1000 -> 190 example scaled by 10^11.
        let scale: cute = 10_000_000_000_000;
        let q = calc_buy(1000 * scale, 1000 * scale, 100 * scale, 0).unwrap();
        assert_eq(q.shares_out, 190 * scale);
        assert_eq(q.new_reserve_bought, 910 * scale);
        assert_eq(q.new_reserve_other, 1100 * scale);

        // Seed at max_sets for 18 decimals.
        let b18 = bounds_for_decimals(18).unwrap();
        let s = seed_reserves(b18.max_sets, 5000).unwrap();
        assert_eq(s.reserve_yes, b18.max_sets);
        assert_eq(s.reserve_no, b18.max_sets);
    }

    #[test]
    fn seed_reserves_at_min_prob_0dp() {
        // d = 0: min_seed = 100, max_sets = 1e8. At MIN_PROB_BPS the
        // reserves must still be > 0.
        let b = bounds_for_decimals(0).unwrap();
        let s = seed_reserves(b.min_seed, MIN_PROB_BPS).unwrap();
        assert!(s.reserve_yes > 0);
        assert!(s.reserve_no > 0);
    }

    #[test]
    fn cost_buy_7dp_vs_18dp() {
        // This test is a placeholder for the wasm benchmark that lives in
        // `lib.rs` (where `Env` is available). Native tests cannot measure
        // Soroban CPU instructions, but we can at least verify that the
        // wide arithmetic produces the same result at both decimal counts.
        let b7 = bounds_for_decimals(7).unwrap();
        let b18 = bounds_for_decimals(18).unwrap();
        let scale_7 = b7.min_seed / 100;
        let scale_18 = b18.min_seed / 100;
        let q7 = calc_buy(1000 * scale_7, 1000 * scale_7, 100 * scale_7, 0).unwrap();
        let q18 = calc_buy(1000 * scale_18, 1000 * scale_18, 100 * scale_18, 0).unwrap();
        assert_eq(q7.fee, q18.fee);
        assert_eq(
            q7.shares_out * scale_18 / scale_7,
            q18.shares_out,
        );
    }
}
