//! Pure integer math for a binary fixed-product market maker (FPMM).
//!
//! No Soroban types in here, so `cargo test` exercises it natively and
//! `model/fpmm_model.py` mirrors it line by line for property testing.
//!
//! Conventions
//! - All amounts are non-negative `i128` in collateral base units
//!   (raw token units; the collateral's `decimals()` is read at deploy time
//!   and the economic bounds are derived from it, see `bounds_for_decimals`).
//! - Rounding always favours the pool: fees round up, shares out round down,
//!   shares in round up. This is what keeps the pool invariant `k = r_yes * r_no`
//!   non-decreasing and blocks dust-splitting attacks.
//! - `mul_div_floor` / `mul_div_ceil` use 256-bit intermediates (`mod wide`),
//!   so a result that fits in `i128` is always returned exactly, regardless of
//!   the size of the `a * b` product. Correctness no longer depends on any
//!   headroom argument about the operands.
//! - `bounds_for_decimals` derives `min_seed` and `max_sets` from the
//!   collateral's decimals (supported range `0..=18`). At `d = 18`,
//!   `max_sets = 1e26`, so products of two quantities can reach `1e52`,
//!   which is why the widening helpers are required.

pub const BPS: i128 = 10_000;
#[deprecated(note = "7-decimal alias for tests only; use bounds_for_decimals")]
pub const MAX_SETS: i128 = 1_000_000_000_000_000; // 1e15 = 100M USDC (7 dp)
pub const MIN_PROB_BPS: u32 = 100; // 1%
pub const MAX_PROB_BPS: u32 = 9_900; // 99%

/// Largest collateral `decimals()` value supported by `bounds_for_decimals`.
pub const MAX_DECIMALS: u32 = 18;

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

/// Economic bounds derived from the collateral's `decimals()`.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Bounds {
    pub min_seed: i128,
    pub max_sets: i128,
}

fn mul(a: i128, b: i128) -> Result<i128, MathError> {
    a.checked_mul(b).ok_or(MathError::Overflow)
}

/// floor(a * b / c)
pub fn mul_div_floor(a: i128, b: i128, c: i128) -> Result<i128, MathError> {
    if a < 0 || b < 0 || c <= 0 {
        return Err(MathError::InvalidInput);
    }
    let (q, _) = wide::divrem(wide::mul(a as u128, b as u128), c as u128)?;
    i128::try_from(q).map_err(|_| MathError::Overflow)
}

/// ceil(a * b / c), written to avoid overflowing on `p + c - 1`.
pub fn mul_div_ceil(a: i128, b: i128, c: i128) -> Result<i128, MathError> {
    if a < 0 || b < 0 || c <= 0 {
        return Err(MathError::InvalidInput);
    }
    let (q, r) = wide::divrem(wide::mul(a as u128, b as u128), c as u128)?;
    let q = if r != 0 {
        q.checked_add(1).ok_or(MathError::Overflow)?
    } else {
        q
    };
    i128::try_from(q).map_err(|_| MathError::Overflow)
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

/// Derive `min_seed` and `max_sets` from the collateral's `decimals()`.
///
/// `min_seed = 100 * 10^d`, `max_sets = 1e8 * 10^d`. Supported `d` is
/// `0..=18`. At `d = 18`, `max_sets = 1e26`; products of two such values
/// reach `1e52` and are handled by the widening helpers above.
pub fn bounds_for_decimals(d: u32) -> Result<Bounds, MathError> {
    if d > MAX_DECIMALS {
        return Err(MathError::InvalidInput);
    }
    let unit = 10i128.checked_pow(d).ok_or(MathError::Overflow)?;
    let min_seed = 100i128.checked_mul(unit).ok_or(MathError::Overflow)?;
    let max_sets = 100_000_000i128
        .checked_mul(unit)
        .ok_or(MathError::Overflow)?;
    Ok(Bounds { min_seed, max_sets })
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
/// The protocol portion rounds **up** so `protocol + lp == fee` exactly (no
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
pub fn settle_value(kind: Settlement, yes: i128, no: i128) -> Result<i128, MathError> {
    if yes < 0 || no < 0 {
        return Err(MathError::InvalidInput);
    }
    Ok(match kind {
        Settlement::Yes => yes,
        Settlement::No => no,
        Settlement::Void => yes.checked_add(no).ok_or(MathError::Overflow)? / 2,
    })
}

/// 256-bit widening multiply and divide, Env-free.
///
/// `mul` is schoolbook on 64-bit limbs; `divrem` normalises and uses a
/// shift-subtract long division on 64-bit digits. `divrem` returns
/// `Overflow` when the quotient does not fit in `u128` (i.e. `n.hi >= d`).
mod wide {
    use super::MathError;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct U256 {
        pub hi: u128,
        pub lo: u128,
    }

    /// Full 128x128 -> 256 multiply via 64-bit limbs.
    pub fn mul(a: u128, b: u128) -> U256 {
        let a_lo = a as u64 as u128;
        let a_hi = (a >> 64) as u64 as u128;
        let b_lo = b as u64 as u128;
        let b_hi = (b >> 64) as u64 as u128;

        let p0 = a_lo * b_lo;
        let p1 = a_lo * b_hi;
        let p2 = a_hi * b_lo;
        let p3 = a_hi * b_hi;

        // lo = p0 + ((p1 + p2) << 64), tracking carries into hi.
        let mid = p1 + p2; // <= 2 * (2^64-1)^2 < 2^129, fits u128
        let mid_lo = mid << 64;
        let mid_hi = mid >> 64;

        let lo = p0.wrapping_add(mid_lo);
        let carry = (lo < p0) as u128;
        let hi = p3 + mid_hi + carry;
        U256 { hi, lo }
    }

    /// Divide a 256-bit numerator by a 128-bit divisor.
    /// Returns `(quotient, remainder)`; `Overflow` if the quotient > u128::MAX.
    pub fn divrem(n: U256, d: u128) -> Result<(u128, u128), MathError> {
        if d == 0 {
            return Err(MathError::InvalidInput);
        }
        if n.hi >= d {
            return Err(MathError::Overflow);
        }
        if n.hi == 0 {
            return Ok((n.lo / d, n.lo % d));
        }

        // Shift-subtract long division over 128 bits. `n.hi < d` guarantees
        // the quotient fits in u128.
        let mut q: u128 = 0;
        let mut rem: u128 = n.hi;
        let mut i: u32 = 128;
        while i > 0 {
            i -= 1;
            // rem = rem * 2 + bit_i(n.lo); rem < d <= u128::MAX so rem*2 may
            // overflow only when rem >= 2^127, in which case rem*2 >= d and
            // the subtraction below still yields a valid remainder.
            let bit = (n.lo >> i) & 1;
            let (shifted, carry) = rem.overflowing_shl(1);
            rem = shifted | bit;
            q <<= 1;
            if carry || rem >= d {
                rem = rem.wrapping_sub(d);
                q |= 1;
            }
        }
        Ok((q, rem))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        for p in [100u32, 2500, 5000, 7300, 9900] {
            let s = seed_reserves(10_000_000_000, p).unwrap();
            let price = price_yes_bps(s.reserve_yes, s.reserve_no).unwrap();
            assert!((price - p as i128).abs() <= 1, "p={p} price={price}");
            assert!(s.creator_yes == 0 || s.creator_no == 0);
        }
    }

    #[test]
    fn k_never_decreases_and_round_trip_never_profits() {
        // tiny LCG so this stays no_std-friendly
        let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = move || {
            x = x
                .wrapping_mul(6364136223846793005)
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
    fn split_fee_sums_exactly() {
        let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = move || {
            x = x
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (x >> 33) as i128
        };
        for _ in 0..2_000 {
            let fee = next() % 1_000_000_000_000;
            let share = (next() % (BPS + 1)) as u32;
            let (protocol, lp) = split_fee(fee, share).unwrap();
            assert_eq!(protocol + lp, fee, "fee={fee} share={share}");
            assert!(protocol >= 0 && lp >= 0);
            // protocol rounds up: never below the exact proportional share
            assert!(protocol * BPS >= fee * share as i128);
        }
    }

    #[test]
    fn split_fee_default_is_all_protocol() {
        assert_eq!(split_fee(12_345, 10_000).unwrap(), (12_345, 0));
        assert_eq!(split_fee(12_345, 0).unwrap(), (0, 12_345));
        assert_eq!(split_fee(0, 3_333).unwrap(), (0, 0));
    }

    #[test]
    fn buy_within_limit_boundaries() {
        // avg price exactly 1.0: passing costs the full limit, passes
        assert!(buy_within_limit(100, 100, 10_000).unwrap());
        // cheaper than the limit
        assert!(buy_within_limit(100, 101, 10_000).unwrap());
        // one unit above the limit fails
        assert!(!buy_within_limit(101, 100, 10_000).unwrap());
        // 100 in -> 200 shares = 0.5 per share, exactly at 5_000 bps
        assert!(buy_within_limit(100, 200, 5_000).unwrap());
        assert!(!buy_within_limit(101, 200, 5_000).unwrap());
    }

    #[test]
    fn sell_within_limit_boundaries() {
        assert!(sell_within_limit(100, 100, 10_000).unwrap());
        assert!(!sell_within_limit(99, 100, 10_000).unwrap());
        // 100 out from 200 shares = 0.5 per share, exactly at 5_000 bps
        assert!(sell_within_limit(100, 200, 5_000).unwrap());
        assert!(!sell_within_limit(100, 201, 5_000).unwrap());
        // a zero limit is trivially satisfied
        assert!(sell_within_limit(0, 1, 0).unwrap());
    }

    #[test]
    fn price_after_maps_both_sides() {
        assert_eq!(price_after(true, 1_000, 1_000).unwrap(), 5_000);
        assert_eq!(price_after(false, 1_000, 1_000).unwrap(), 5_000);
        // NO buy pushing NO reserve to 910 with 1100 YES: YES price is
        // 1100 / (1100 + 910), i.e. price_yes_bps(yes, no)
        assert_eq!(
            price_after(false, 910, 1_100).unwrap(),
            price_yes_bps(1_100, 910).unwrap()
        );
    }

    #[test]
    fn wide_mul_matches_reference() {
        // Random u128 pairs checked against a two-step decomposition.
        let mut x: u64 = 0xDEAD_BEEF_CAFE_F00D;
        let mut next = move || {
            x = x
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((x as u128) << 64) | (x.rotate_left(17) as u128)
        };
        for _ in 0..10_000 {
            let a = next();
            let b = next();
            let p = wide::mul(a, b);
            // Reconstruct via u128::widening_mul semantics: split a into
            // (a_hi, a_lo) 64-bit halves and accumulate.
            let a_lo = a as u64 as u128;
            let a_hi = a >> 64;
            let lo_part = a_lo * b;
            let hi_part = a_hi * b;
            let lo = lo_part.wrapping_add(hi_part << 64);
            let carry = (lo < lo_part) as u128;
            let hi = (hi_part >> 64) + carry;
            assert_eq!(p.lo, lo);
            assert_eq!(p.hi, hi);
        }
    }

    #[test]
    fn mul_div_handles_products_above_i128() {
        // a = b = 1e26, c = 1e26 -> 1e26, even though a*b = 1e52 > i128::MAX.
        let big: i128 = 100_000_000_000_000_000_000_000_000;
        assert_eq!(mul_div_floor(big, big, big).unwrap(), big);
        assert_eq!(mul_div_ceil(big, big, big).unwrap(), big);
        // ceil rounds up on a non-zero remainder.
        assert_eq!(mul_div_ceil(big, big, big + 1).unwrap(), big);
        assert_eq!(mul_div_floor(big, big, big + 1).unwrap(), big - 1);
    }

    #[test]
    fn mul_div_differential_vs_legacy() {
        // 100k LCG cases inside the legacy (7-decimal) domain.
        let mut x: u64 = 0x1234_5678_9ABC_DEF0;
        let mut next = move || {
            x = x
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (x >> 33) as i128
        };
        for _ in 0..100_000 {
            let a = next() % 1_000_000_000_000_000;
            let b = next() % 1_000_000_000_000_000;
            let c = 1 + next() % 1_000_000_000_000_000;
            let legacy_floor = (a * b) / c;
            let legacy_ceil = {
                let p = a * b;
                let q = p / c;
                if p % c != 0 { q + 1 } else { q }
            };
            assert_eq!(mul_div_floor(a, b, c).unwrap(), legacy_floor);
            assert_eq!(mul_div_ceil(a, b, c).unwrap(), legacy_ceil);
        }
    }

    #[test]
    fn bounds_for_decimals_0_7_18() {
        let b0 = bounds_for_decimals(0).unwrap();
        assert_eq!(b0.min_seed, 100);
        assert_eq!(b0.max_sets, 100_000_000);

        let b7 = bounds_for_decimals(7).unwrap();
        assert_eq!(b7.min_seed, 100 * 10_000_000);
        assert_eq!(b7.max_sets, 1_000_000_000_000_000);

        let b18 = bounds_for_decimals(18).unwrap();
        assert_eq!(b18.min_seed, 100_000_000_000_000_000_000);
        assert_eq!(b18.max_sets, 100_000_000_000_000_000_000_000_000);

        assert_eq!(
            bounds_for_decimals(19),
            Err(MathError::InvalidInput)
        );
    }

    #[test]
    fn known_values_18dp() {
        // 1000/1000 -> 190 example scaled by 10^11 (18 dp vs 7 dp).
        let scale: i128 = 100_000_000_000;
        let q = calc_buy(1000 * scale, 1000 * scale, 100 * scale, 0).unwrap();
        assert_eq!(q.shares_out, 190 * scale);
        assert_eq!(q.new_reserve_bought, 910 * scale);
        assert_eq!(q.new_reserve_other, 1100 * scale);
        // Seed at max_sets for 18 dp opens at the requested probability.
        let b = bounds_for_decimals(18).unwrap();
        let s = seed_reserves(b.max_sets, 5_000).unwrap();
        let price = price_yes_bps(s.reserve_yes, s.reserve_no).unwrap();
        assert!((price - 5_000).abs() <= 1);
    }
}
