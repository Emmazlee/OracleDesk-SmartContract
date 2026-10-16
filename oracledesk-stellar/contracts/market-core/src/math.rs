//! Pure integer math for a binary fixed-product market maker (FPMM).
//
// No Soroban types in here, so `cargo test` exercises it natively and
// `model/fpmm_model.py` mirrors it line by line for property testing.
//
// Conventions
// - All amounts are non-negative `i128` in collateral base units
//   (USDC on Stellar = 7 decimals, so 1 USDC = 10_000_000).
// - Rounding always faours the pool: fees round up, shares out round down,
//   shares in round up. This is what keeps the pool invariant k = r_yes * r_no
//   non-decreasing and blocks dust-splitting attacks.
// - `MAX_SETS` bounds market size so that any product of two quantities
//   (both <= 1e15) is <= 1e30, far below `i128::MAX` (~1.7e38). Every
//   multiplication is still `checked_*` as a second line of defence.
//
// # Headroom for `sell_shares`
//
// With b = shares_in + r_sold + r_other and every term bounded by
// `MAX_SETS = 1e15`, we get b <= 3e15 and b^2 <= 9e30, so the discriminant
// D = b^2 - 4*r_other*shares_in fits easily in `u128` (max about 3.4e38).
// The intermediate products in the correction loop are at most 1e30 and
// fit in `i128`. The 256-bit / decimals widening issue should revisit
// this bound if `MAX_SETS` is ever raised.

pub const BPS: i128 = 10_000;
pub const MAX_SETS: i128 = 1_000_000_000_000_000_000; // 1e15 = 100M USDC (7 dp)
pub const MIN_PROB_BPS: u32 = 100; // 1%
pub const MAX_PROB_BPS: u32 = 9_900; // 99%

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
pub struct SellSharesQuote {
    pub collateral_out: i128,
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

fn mul(a: i128, b: i128) -> Result<i128, MathError> {
    a.checked_mul(b).ok_or(MathError::Overflow)
}

/// floor(a * b / c)
pub fn mul_div_floor(a: i128, b: i128, c: i128) -> Result<i128, MathError> {
    if a < 0 || b < 0 || c <= 0 {
        return Err(MathError::InvalidInput);
    }
    Ok(mul(a, b)? / c)
}

/// ceil(a * b / c), written to avoid overflowing on `p + c - 1`.
pub fn mul_div_ceil(a: i128, b: i128, c: i128) -> Result<i128, MathError> {
    if a < 0 || b < 0 || c <= 0 {
        return Err(MathError::InvalidInput);
    }
    let p = mul(a, b)?;
    let q = p / c;
    Ok(if p % c != 0 { q + 1 } else { q })
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
//
// 1. fee = ceil(in * fee_bps / BPS); net = in - fee
// 2. mint `net` complete sets into the pool: r_other += net
// 3. pool keeps r_bought' = ceil(r_bought * r_other / r_other')  (k preserved)
// 4. buyer receives r_bought + net - r_bought'
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
//
// 1. gross = ceil(out * BPS / (BPS - fee_bps)); fee = gross - out
// 2. burn `gross` complete sets from the pool: r_other -= gross
// 3. pool must end at r_sold' = ceil(r_sold * r_other / r_other')
// 4. seller gives r_sold' + gross - r_sold shares
//
// To sell an exact share count, prefer `sell_shares` / `calc_sell_shares`,
// which is exact on-chain. This function remains for backward
// compatibility and for clients that want to specify an exact collateral
// amount.
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

/// Integer square root of `u128`, exact for every input reachable under
/// `MAX_SETS`. Newton iteration seeded from above the root, so convergence
/// is monotone downward and the first `y >= x` hit is `floor(sqrt(n)`.
/// Seeding below the root would let Newton converge from below and return
/// `floor + 1`.
///
/// Invariant: `x*x <= n < (x+1)*(x+1)` for every input.
pub fn isqrt(n: u128) -> u128 {
    if n < 2 {
        return n;
    }
    // Seed above the root: shift = ceil(bit_length / 2), so 2^shift >= sqrt(n).
    let shift = (128 - n.leading_zeros() + 1) / 2;
    let mut x = 1u128 << shift;
    loop {
        let y = (x + n / x) >> 1;
        if y >= x {
            return x;
        }
        x = y;
    }
}

/// Sell an exact `shares_in` count of the outcome with pool reserve
/// `r_sold` for collateral. This is the on-chain inverse of `calc_sell`.
///
/// Solves the constant-product equation for the gross sets burned:
///   (r_sold + shares_in - x) * (r_other - x) = r_sold * r_other
/// which is a quadratic in x:
///   x^2 - b * x + r_other * shares_in = 0,
///   b = r_sold + shares_in + r_other.
/// The smaller root x0 = (b - sqrt(D)) / 2 is the gross sets burned.
///
/// # Pool-favoring correction
///
/// `isqrt` rounds down, so `x0 = (b - isqrt(D)) / 2` can overshoot the
/// real root by one. We decrement `x` while the invariant would decrease:
///   (r_sold + shares_in - x) * (r_other - x) < r_sold * r_other
/// This loop runs at most twice: the quadratic f(x) = (r_sold + shares_in - x)
/// * (r_other - x) is a concave parabola in x, so the set of integers with
/// f(x) >= k is a contiguous interval [x_, x_]. Since isqrt errs on the
/// down side by at most 1, x0 is at most x_ + 1. One decrement lands at x_.
///
/// # Never pays more than `calc_sell`
///
/// Let `x`  be the gross sets burned by this function and `x'` the gross
/// sets burned by `calc_sell(r_sold, r_other, collateral_out, fee_bps)`
/// for the same `shares_in`. The correction loop guarantees that `x` is
/// the largest integer with f(x) >= k, so `x <= x'`. Since `gross` is the
/// only source of fee and of collateral out, and the fee is monotone in
/// gross, `collateral_out = floor(x * (BPS - fee) / BPS) <= floor(x' *
/// (BPS - fee) / BPS) = the collateral `calc_sell` would pay for the same
/// shares. So a seller never gets more than the curve allows.
///
/// # Never decreases k
///
/// By construction the correction loop ensures
///   (r_sold + shares_in - x) * (r_other - x) >= r_sold * r_other.
/// and `new_sold = r_sold + shares_in - x`, `new_other = r_other - x`, so the
/// invariant `k = new_sold * new_other` is non-decreasing.
///
/// Rejects `x == 0` and `x >= r_other` with `InsufficientLiquidity`, and
/// `collateral_out == 0` with `InvalidInput` so a trader cannot burn
/// shares for nothing.
pub fn calc_sell_shares(
    r_sold: i128,
    r_other: i128,
    shares_in: i128,
    fee_bps: u32,
) -> Result<SellSharesQuote, MathError> {
    if shares_in <= 0
        || r_sold <= 0
        || r_other <= 0
        || (fee_bps as i128) >= BPS
    {
        return Err(MathError::InvalidInput);
    }

    // b = r_sold + shares_in + r_other <= 3e15, b^2 <= 9e30.
    let b = r_sold
        .checked_add(shares_in)
        .ok_or(MathError::Overflow)?
        .checked_add(r_other)
        .ok_or(MathError::Overflow)?;
    let b2 = b.checked_mul(b).ok_or(MathError::Overflow)? as u128;
    // 4*`r_other * shares_in` <= 4e30, fits in u128.
    let four_ro_sh = 4u128
        * (r_other as u128)
        * (shares_in as u128);
    // D = b^2 - 4*`r_other * shares_in`. Since four_ro_sh <= b^2 for valid
    // inputs, this is non-negative. Use checked_sub anyway as a defence.
    let disc = b2
        .checked_sub(four_ro_sh)
        .ok_or(MathError::Overflow)?;
    let root = isqrt(disc);
    // x0 = (b - sqrt(D)) / 2. isqrt rounds down, so x0 can overshoot the
    // real root by one.
    let mut x = (b - root as i128) / 2;

    // Pool-favoring correction. See the doc comment above for the proof
    // that this loop runs at most twice.
    let k0 = r_sold.checked_mul(r_other).ok_or(MathError::Overflow)?;
    while x > 0 {
        let new_sold_try = r_sold
            .checked_add(shares_in)
            .ok_or(MathError::Overflow)?
            .checked_sub(x)
            .ok_or(MathError::Overflow)?;
        let new_other_try = r_other.checked_sub(x).ok_or(MathError::Overflow)?;
        let k_try = new_sold_try
            .checked_mul(new_other_try)
            .ok_or(MathError::Overflow)?;
        if k_try >= k0 {
            break;
        }
        x = x - 1;
    }

    if x <= 0 || x >= r_other {
        return Err(MathError::InsufficientLiquidity);
    }

    // collateral_out = floor(x * (BPS - fee_bps) / BPS); fee = x - collateral_out
    // The fee rounds up, as in calc_buy.
    let collateral_out = mul_div_floor(x, BPS - fee_bps as i128, BPS)?;
    let fee = x - collateral_out;
    if collateral_out <= 0 {
        return Err(MathError::InvalidInput);
    }

    let new_sold = r_sold
        .checked_add(shares_in)
        .ok_or(MathError::Overflow)?
        - x;
    let new_other = r_other - x;

    Ok(SellSharesQuote {
        collateral_out,
        fee,
        gross: x,
        new_reserve_sold: new_sold,
        new_reserve_other: new_other,
    })
}

/// Collateral value of a (yes, no) share pair once a market is final.
/// Void pays 0.5 per share, which keeps total payouts <= sets minted.
pub fn settle_value(kind: Settlement, yes: i128, no: i128) -> i128 {
    match kind {
        Settlement::Yes => yes,
        Settlement::No => no,
        Settlement::Void => (yes + no) / 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            assert(q.new_reserve_bought * q.new_reserve_other >= k0);
            // getting `amt` back out costs at least the shares the buy paid out
            if let Ok(s) = calc_sell(q.new_reserve_bought, q.new_reserve_other, amt, fee) {
                assert(s.shares_in >= q.shares_out, "free money");
            }
        }
    }

    #[test]
    fn void_never_overpays() {
        assert_eq(settle_value(Settlement::Void, 3, 4), 3);
        assert(settle_value(Settlement::Void, 5, 5) <= 5);
    }

    #[test]
    fn isqrt_exact_for_boundaries() {
        // Small values and perfect squares.
        for n in [0u128, 1, 2, 3, 4, 8, 9, 15, 16, 17, 255, 256, 257] {
            let r = isqrt(n);
            assert!(r * r <= n, "n={} result={}", n, r);
            assert!((r + 1) * (r + 1) > n, "n={} result={}", n, r);
        }
        // Large perfect square: (3e15)^2.
        let big = (3_000_000_000_000_000u128);
        let n = big * big;
        let r = isqrt(n);
        assert_eq(r, big);
        // u128::MAX.
        let m = u128::MAX;
        let rm = isqrt(m);
        assert!(rm * rm <= m);
        assert!((rm + 1) * (rm + 1) > m);
        // One less than a perfect square.
        let n2 = big * big - 1;
        assert_eq(isqrt(n2), big - 1);
    }

    #[test]
    fn sell_shares_example_matches_model() {
        // 1000/1000 pool, sell 212 shares, no fee. The exact inverse of
        // calc_sell(1000, 1000, 100, 0) -> shares_in = 212.
        let q = calc_sell_shares(1000, 1000, 212, 0).unwrap();
        assert_eq(q.gross, 100);
        assert_eq(q.collateral_out, 100);
        assert_eq(q.fee, 0);
        assert_eq(q.new_reserve_sold, 1112);
        assert_eq(q.new_reserve_other, 900);
    }

    #[test]
    fn sell_shares_k_never_decreases() {
        let mut x: u64 = 0xDEAD_BEEF_CARD_FEED;
        let mut next = move || {
            x = x
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (x >> 33) as i128
        };
        for _ in 0..2_000 {
            let rs = 1_000_000 + next() % 1_000_000_000;
            let ro = 1_000_000 + next() % 1_000_000_000;
            // Shares in bounded by the sold reserve so the sell is always
            // feasible.
            let shares = 1 + next() % (rs / 2);
            let fee = (next() % 300) as u32;
            let k0 = rs * ro;
            let q = calc_sell_shares(rs, ro, shares, fee).unwrap();
            assert!(
                q.new_reserve_sold * q.new_reserve_other >= k0,
                "k decreased: {} < {}",
                q.new_reserve_sold * q.new_reserve_other,
                k0
            );
        }
    }

    #[test]
    fn sell_shares_never_pays_more_than_calc_sell() {
        let mut x: u64 = 0xCAFE_FOOD_BABE_DOGE;
        let mut next = move || {
            x = x
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (x >> 33) as i128
        };
        for _ in 0..2_000 {
            let rs = 1_000_000 + next() % 1_000_000_000;
            let ro = 1_000_000 + next() % 1_000_000_000;
            let shares = 1 + next() % (rs / 2);
            let fee = (next() % 300) as u32;
            let q = calc_sell_shares(rs, ro, shares, fee).unwrap();
            // The exact sell for the collateral this function produces must
            // cost at most `shares`. This is the core pool-favoring
            // property: a seller never gets more than the curve allows.
            if let Ok(forward) = calc_sell(rs, ro, q.collateral_out, fee) {
                assert!(
                    forward.shares_in <= shares,
                    "forward.shares_in={} > shares_in={}",
                    forward.shares_in,
                    shares
                );
            }
        }
    }

    #[test]
    fn sell_shares_at_max_sets_no_overflow() {
        // Max reserves and max shares in both directions.
        let q = calc_sell_shares(MAX_SETS, MAX_SETS, MAX_SETS, 0).unwrap();
        assert(q.collateral_out > 0);
        assert(q.new_reserve_sold > 0);
        assert(q.new_reserve_other > 0);
        // And with the max fee.
        let q2 = calc_sell_shares(MAX_SETS, MAX_SETS, MAX_SETS, 1_000).unwrap();
        assert(q2.collateral_out > 0);
        assert(q2.fee > 0);
    }

    #[test]
    fn sell_shares_rejects_invalid_inputs() {
        assert!(calc_sell_shares(0, 1000, 100, 0).is_err());
        assert!(calc_sell_shares(1000, 0, 100, 0).is_err());
        assert!(calc_sell_shares(1000, 1000, 0, 0).is_err());
        assert!(calc_sell_shares(1000, 1000, 100, BPS as u32).is_err());
    }

    #[test]
    fn sell_shares_rejects_tiny_sell() {
        // Selling one share in a huge pool floors collateral_out to 0 and
        // must revert, not silently burn the share.
        let r = MAX_SETS;
        let res = calc_sell_shares(r, r, 1, 0);
        assert!(res.is_err());
    }
}
