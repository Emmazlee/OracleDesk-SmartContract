"""Python mirror of contracts/market-core/src/math.rs.

Kept in lockstep with the Rust module by hand (both are short and
self-contained) and exercised by the property tests at the bottom of this
file, which run with no dependencies beyond the standard library:

    python3 model/fpmm_model.py

This is also where the off-chain inverse for "sell N shares" lives
(`gross_for_shares`): the contract's `sell` takes an exact collateral amount
(no on-chain sqrt), so a client that wants to sell a specific share count
computes the matching collateral_out here, then calls `sell` with a
`max_shares_in` slippage bound. `agents/core` ports this same function to
TypeScript (BigInt) for the trading agent; both must agree with this model.

Conventions match math.rs: all amounts are non-negative ints in collateral
base units (whatever `decimals()` the collateral token reports; the contract
supports 0..=18). Rounding always favours the pool.
"""

from dataclasses import dataclass
from math import isqrt
import random

BPS = 10_000
# Deprecated 7-decimal alias, kept only so existing callers/tests keep working.
# Contract logic derives its bounds from `bounds_for_decimals` instead.
MAX_SETS = 1_000_000_000_000_000  # 1e15 = 100M USDC (7 dp)
MIN_PROB_BPS = 100
MAX_PROB_BPS = 9_900
I128_MAX = (1 << 127) - 1
MAX_SUPPORTED_DECIMALS = 18


class MathError(Exception):
    pass


@dataclass(frozen=True)
class Bounds:
    min_seed: int
    max_sets: int


def bounds_for_decimals(d: int) -> Bounds:
    """Mirrors math.rs::bounds_for_decimals: min_seed = 100 * 10^d,
    max_sets = 1e8 * 10^d, for d in 0..=18."""
    if d < 0 or d > MAX_SUPPORTED_DECIMALS:
        raise MathError("unsupported decimals")
    scale = 10**d
    return Bounds(min_seed=100 * scale, max_sets=100_000_000 * scale)


def _fits_i128(x: int) -> bool:
    return 0 <= x <= I128_MAX


def mul_div_floor(a: int, b: int, c: int) -> int:
    if a < 0 or b < 0 or c <= 0:
        raise MathError("invalid input")
    q = (a * b) // c
    if not _fits_i128(q):
        raise MathError("overflow")
    return q


def mul_div_ceil(a: int, b: int, c: int) -> int:
    if a < 0 or b < 0 or c <= 0:
        raise MathError("invalid input")
    p = a * b
    q = p // c
    q = q + 1 if p % c != 0 else q
    if not _fits_i128(q):
        raise MathError("overflow")
    return q


def price_yes_bps(reserve_yes: int, reserve_no: int) -> int:
    total = reserve_yes + reserve_no
    if total <= 0:
        raise MathError("insufficient liquidity")
    return mul_div_floor(reserve_no, BPS, total)


@dataclass(frozen=True)
class SeedReserves:
    reserve_yes: int
    reserve_no: int
    creator_yes: int
    creator_no: int


def seed_reserves(seed: int, yes_bps: int) -> SeedReserves:
    if seed <= 0 or yes_bps < MIN_PROB_BPS or yes_bps > MAX_PROB_BPS:
        raise MathError("invalid input")
    p = yes_bps
    if p >= BPS // 2:
        reserve_yes, reserve_no = mul_div_floor(seed, BPS - p, p), seed
    else:
        reserve_yes, reserve_no = seed, mul_div_floor(seed, p, BPS - p)
    if reserve_yes <= 0 or reserve_no <= 0:
        raise MathError("invalid input")
    return SeedReserves(reserve_yes, reserve_no, seed - reserve_yes, seed - reserve_no)


@dataclass(frozen=True)
class BuyQuote:
    shares_out: int
    fee: int
    net: int
    new_reserve_bought: int
    new_reserve_other: int


def calc_buy(r_bought: int, r_other: int, collateral_in: int, fee_bps: int) -> BuyQuote:
    if collateral_in <= 0 or r_bought <= 0 or r_other <= 0:
        raise MathError("invalid input")
    fee = mul_div_ceil(collateral_in, fee_bps, BPS)
    net = collateral_in - fee
    if net <= 0:
        raise MathError("invalid input")
    new_other = r_other + net
    new_bought = mul_div_ceil(r_bought, r_other, new_other)
    shares_out = r_bought + net - new_bought
    if shares_out <= 0:
        raise MathError("invalid input")
    return BuyQuote(shares_out, fee, net, new_bought, new_other)


@dataclass(frozen=True)
class SellQuote:
    shares_in: int
    fee: int
    gross: int
    new_reserve_sold: int
    new_reserve_other: int


def calc_sell(r_sold: int, r_other: int, collateral_out: int, fee_bps: int) -> SellQuote:
    if collateral_out <= 0 or r_sold <= 0 or r_other <= 0 or fee_bps >= BPS:
        raise MathError("invalid input")
    gross = mul_div_ceil(collateral_out, BPS, BPS - fee_bps)
    fee = gross - collateral_out
    if gross >= r_other:
        raise MathError("insufficient liquidity")
    new_other = r_other - gross
    new_sold = mul_div_ceil(r_sold, r_other, new_other)
    shares_in = new_sold + gross - r_sold
    if shares_in <= 0:
        raise MathError("invalid input")
    return SellQuote(shares_in, fee, gross, new_sold, new_other)


def settle_value(kind: str, yes: int, no: int) -> int:
    if kind == "yes":
        return yes
    if kind == "no":
        return no
    if kind == "void":
        return (yes + no) // 2
    raise ValueError(f"unknown settlement kind {kind!r}")


def gross_for_shares(r_sold: int, r_other: int, shares_in: int, fee_bps: int) -> int:
    """Off-chain inverse of calc_sell: how many complete sets (`gross`) the
    pool must burn so that selling exactly `shares_in` shares balances the
    constant-product invariant, ignoring the contract's integer rounding.

    Solves (shares_in + r_sold - x)(r_other - x) = r_sold * r_other for the
    smaller root of x (the pool never burns more than r_other).
    """
    if shares_in <= 0 or r_sold <= 0 or r_other <= 0 or fee_bps >= BPS:
        raise MathError("invalid input")
    a = shares_in + r_sold
    b = a + r_other
    disc = b * b - 4 * r_other * shares_in
    if disc < 0:
        raise MathError("insufficient liquidity")
    root = isqrt(disc)
    gross = (b - root) // 2
    if gross <= 0 or gross >= r_other:
        raise MathError("insufficient liquidity")
    return gross


def collateral_out_for_shares(r_sold: int, r_other: int, shares_in: int, fee_bps: int) -> int:
    """Estimated `collateral_out` for selling `shares_in` shares, rounded
    down (pool-favoring) so the real on-chain `sell` call — given this value
    and a `max_shares_in` slippage bound of `shares_in` — should not need
    more shares than the caller intended to sell."""
    gross = gross_for_shares(r_sold, r_other, shares_in, fee_bps)
    return mul_div_floor(gross, BPS - fee_bps, BPS)


def price_after(outcome_is_yes: bool, r_after: int, r_other_after: int) -> int:
    """YES price in bps of the pool **after** applying an outcome-relative
    reserve update (the `new_reserve_*` pair of a BuyQuote/SellQuote).
    Mirrors math.rs::price_after: pass `outcome_is_yes=False` for a NO trade,
    where `r_after` is the NO reserve."""
    if outcome_is_yes:
        return price_yes_bps(r_after, r_other_after)
    return price_yes_bps(r_other_after, r_after)


def price_after_buy(taken_is_yes: bool, quote: BuyQuote) -> int:
    """Post-trade YES price of a `calc_buy` quote for the outcome the buyer
    receives (`taken_is_yes=False` for a NO buy). This is what the
    per-market impact breaker measures against the pre-trade price."""
    return price_after(taken_is_yes, quote.new_reserve_bought, quote.new_reserve_other)


def price_after_sell(taken_is_yes: bool, quote: SellQuote) -> int:
    """Post-trade YES price of a `calc_sell` quote for the outcome the seller
    gave up (`taken_is_yes=False` for a NO sell)."""
    return price_after(taken_is_yes, quote.new_reserve_sold, quote.new_reserve_other)


def buy_within_limit(collateral_in: int, shares_out: int, limit_bps: int) -> bool:
    """True when a buy's average execution price `collateral_in / shares_out`
    (fees included) is at most `limit_bps` per share. Compared by
    cross-multiplication so rounding cannot flip the decision; exact
    equality passes. Mirrors math.rs::buy_within_limit."""
    if collateral_in < 0 or shares_out <= 0:
        raise MathError("invalid input")
    return collateral_in * BPS <= shares_out * limit_bps


def sell_within_limit(collateral_out: int, shares_in: int, limit_bps: int) -> bool:
    """True when a sell's average execution price `collateral_out / shares_in`
    is at least `limit_bps` per share. Exact equality passes; a zero limit is
    trivially satisfied, which buys the caller no protection. Mirrors
    math.rs::sell_within_limit."""
    if collateral_out < 0 or shares_in <= 0:
        raise MathError("invalid input")
    return collateral_out * BPS >= shares_in * limit_bps


# ---------------------------------------------------------------------------
# Property tests. No pytest/hypothesis dependency: run directly.
# ---------------------------------------------------------------------------


def _check_known_values():
    """Pins the same worked examples as math.rs's unit tests, so a porting
    mistake between the two languages is caught immediately."""
    q = calc_buy(1000, 1000, 100, 0)
    assert (q.shares_out, q.new_reserve_bought, q.new_reserve_other) == (190, 910, 1100), q

    q = calc_sell(1000, 1000, 100, 0)
    assert (q.shares_in, q.gross, q.new_reserve_sold, q.new_reserve_other) == (
        212,
        100,
        1112,
        900,
    ), q


def _check_known_values_18dp():
    """18-decimal fixtures matching the Rust `known_values_18dp` test.

    The 1000/1000 -> 190 worked example scaled by 10**11 (7dp -> 18dp), plus
    a seed at `max_sets` for d=18 to prove the wider intermediates are exact.
    """
    scale = 10**11
    q = calc_buy(1000 * scale, 1000 * scale, 100 * scale, 0)
    assert (q.shares_out, q.new_reserve_bought, q.new_reserve_other) == (
        190 * scale,
        910 * scale,
        1100 * scale,
    ), q

    b = bounds_for_decimals(18)
    assert b.min_seed == 100 * 10**18
    assert b.max_sets == 10**26
    s = seed_reserves(b.max_sets, 5000)
    assert s.reserve_yes == b.max_sets and s.reserve_no == b.max_sets
    assert price_yes_bps(s.reserve_yes, s.reserve_no) == 5000


def _check_bounds_for_decimals():
    assert bounds_for_decimals(0) == Bounds(min_seed=100, max_sets=100_000_000)
    assert bounds_for_decimals(7) == Bounds(
        min_seed=100 * 10_000_000, max_sets=1_000_000_000_000_000
    )
    assert bounds_for_decimals(18) == Bounds(min_seed=100 * 10**18, max_sets=10**26)
    for bad in (-1, 19, 100):
        try:
            bounds_for_decimals(bad)
        except MathError:
            pass
        else:
            raise AssertionError(f"bounds_for_decimals({bad}) should reject")


def _check_mul_div_rejects_overflow():
    # Product fits in i128 but the quotient does not: must raise, matching
    # the contract's Error::Overflow rather than silently returning a value.
    try:
        mul_div_floor(I128_MAX, I128_MAX, 1)
    except MathError:
        pass
    else:
        raise AssertionError("mul_div_floor should reject quotient > i128::MAX")
    try:
        mul_div_ceil(I128_MAX, I128_MAX, 1)
    except MathError:
        pass
    else:
        raise AssertionError("mul_div_ceil should reject quotient > i128::MAX")
    # A product above i128 but a quotient that fits must still succeed.
    assert mul_div_floor(10**26, 10**26, 10**26) == 10**26


def _check_seed_opens_at_requested_probability():
    for p in (100, 2500, 5000, 7300, 9900):
        s = seed_reserves(10_000_000_000, p)
        price = price_yes_bps(s.reserve_yes, s.reserve_no)
        assert abs(price - p) <= 1, (p, price)
        assert s.creator_yes == 0 or s.creator_no == 0


def _check_k_never_decreases_and_round_trip_never_profits(trials: int = 2_000, seed: int = 7):
    rng = random.Random(seed)
    for _ in range(trials):
        ry = rng.randint(1_000_000, 1_000_000_000)
        rn = rng.randint(1_000_000, 1_000_000_000)
        amt = rng.randint(1, max(1, min(ry, rn) // 2))
        fee_bps = rng.randint(0, 299)
        k0 = ry * rn
        q = calc_buy(ry, rn, amt, fee_bps)
        assert q.new_reserve_bought * q.new_reserve_other >= k0
        try:
            s = calc_sell(q.new_reserve_bought, q.new_reserve_other, amt, fee_bps)
        except MathError:
            continue
        assert s.shares_in >= q.shares_out, "free money"


def _check_gross_for_shares_inverts_calc_sell(trials: int = 2_000, seed: int = 11):
    """For a shares_in target derived from a real calc_sell call, the
    estimated collateral_out must round-trip through the *real* calc_sell to
    a shares_in within a one-unit rounding tolerance of the original request.

    The quadratic solve is over continuous reals; calc_sell's ceil-rounding
    can then push the result up by a unit of the smallest collateral
    denomination. This is exactly why market-core's `sell` takes a
    `max_shares_in` slippage bound rather than requiring an exact match —
    callers should pad it a little further for their own safety margin, not
    treat this tolerance as the only slippage protection they need.
    """
    rng = random.Random(seed)
    checked = 0
    for _ in range(trials):
        r_sold = rng.randint(1_000_000, 1_000_000_000)
        r_other = rng.randint(1_000_000, 1_000_000_000)
        fee_bps = rng.randint(0, 299)
        collateral_out = rng.randint(1, max(1, r_other // 4))
        try:
            forward = calc_sell(r_sold, r_other, collateral_out, fee_bps)
        except MathError:
            continue
        try:
            estimated_out = collateral_out_for_shares(r_sold, r_other, forward.shares_in, fee_bps)
        except MathError:
            continue
        if estimated_out <= 0:
            continue
        back = calc_sell(r_sold, r_other, estimated_out, fee_bps)
        assert back.shares_in <= forward.shares_in + 1, (forward, back)
        checked += 1
    assert checked > trials // 2, "too many trials skipped, weakens the property check"


def _check_void_never_overpays():
    assert settle_value("void", 3, 4) == 3
    assert settle_value("void", 5, 5) <= 5


def _check_price_after_maps_both_sides():
    """Mirrors math.rs's price_after tests: outcome_is_yes selects which of
    the two post-trade reserves is the YES reserve."""
    for r, other in ((1000, 1000), (910, 1100), (1, 9999), (5893, 4107)):
        assert price_after(True, r, other) == price_yes_bps(r, other)
        assert price_after(False, r, other) == price_yes_bps(other, r)

    # Worked example shared by all three mirrors: buying 10 USDC of YES in a
    # 100-sets pool at 50% with a 1% fee moves YES from 5000 to 5470 bps.
    q = calc_buy(1_000_000_000, 1_000_000_000, 100_000_000, 100)
    assert price_yes_bps(1_000_000_000, 1_000_000_000) == 5000
    assert price_after_buy(True, q) == 5470
    # Selling right back must not land above the pre-trade price: two fees
    # and pool-favoring rounding always leave the seller worse off.
    s = calc_sell(q.new_reserve_bought, q.new_reserve_other, 100_000_000, 100)
    assert price_after_sell(True, s) < 5000


def _check_limits_boundary_and_symmetry():
    # Exact equality passes, one unit over fails, in both directions.
    assert buy_within_limit(100, 100, 10_000)
    assert buy_within_limit(100, 101, 10_000)
    assert not buy_within_limit(101, 100, 10_000)
    assert buy_within_limit(100, 200, 5_000)
    assert not buy_within_limit(101, 200, 5_000)
    assert sell_within_limit(100, 100, 10_000)
    assert not sell_within_limit(99, 100, 10_000)
    assert sell_within_limit(100, 200, 5_000)
    assert not sell_within_limit(100, 201, 5_000)
    # A zero limit is trivially satisfied for sells (documented as "no
    # protection") and only binds a buy at a zero average price.
    assert sell_within_limit(0, 1, 0)
    assert not buy_within_limit(1, 1, 0)


def run_all():
    _check_known_values()
    _check_known_values_18dp()
    _check_bounds_for_decimals()
    _check_mul_div_rejects_overflow()
    _check_seed_opens_at_requested_probability()
    _check_k_never_decreases_and_round_trip_never_profits()
    _check_gross_for_shares_inverts_calc_sell()
    _check_void_never_overpays()
    _check_price_after_maps_both_sides()
    _check_limits_boundary_and_symmetry()
    print("fpmm_model: all property checks passed")


if __name__ == "__main__":
    run_all()
