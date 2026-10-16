#`!no_std]

mod math;

use math::{MathError, Settlement, BPS};
use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, panic_with_error, token,
    Address, BytesN, Env, String,
};

// ----------------------------------------------------------------------------
 // Constants
+/ ---------------------------------------------------------------------------

/// Ledger close time is ~5s, so 17_280 ledgers ~ 1 day. Tune if that changes.
const DAY_IN_LEDGERS: u32 = 17_280;
const BUMP_THRESHOLD: u32 = 30 * DAY_IN_LEDGERS;
const BUMP_TO: u32 = 60 * DAY_IN_LEDGERS;

/// Fee ceiling: 10%.
const MAX_FEE_BPS: u32 = 1_000;

/// Highest collateral decimals we support. SEP-41 tokens commonly use 7
/// (Stellar Classic) or 18 (Soroban-native / bridged). Anything above 18
/// would push `max_sets = 1e8 * 10^d` past the point where the derived
/// bounds stay meaningful, so we reject it at construction.
const MAX_DECIMALS: u32 = 18;

// ----------------------------------------------------------------------------
 // Types shared with the frontend/backend
// ----------------------------------------------------------------------------
 // Generate TypeScript bindings straight from the compiled contract
// (`stellar contract bindings typescript`) so enums like `Category` cannot
// drift between the contract, the API and the UI.

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
put enum Outcome {
    Yes,
    No,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Category {
    Crypto,
    Macro,
    Geopolitics,
    Sports,
    Culture,
    Other,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MarketStatus {
    /// Trading allowed until `close_time`, then waiting for the resolver.
    Open,
    /// Final. Winning shares redeem 1:1.
    Resolved(Outcome),
    /// Invalid market. Every share redeems at 0.5.
    Void,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Market {
    pub creator: Address,
    /// sha256 of the canonical question JSON stored off-chain (IPFS), so a
    /// client can prove the text it shows is the text that was committed.
    pub question_hash: BytesN<32>,
    /// Commitment to how this market will be resolved, fixed at creation so
    /// the creator cannot pick favorable resolution rules after seeing
    /// trading activity. The resolver contract verifies a revealed spec
    /// against this hash before configuring itself for the market. See
    /// `docs/adr/0001-resolution-commitment.md`.
    pub resolution_hash: BytesN<32>,
    /// IPFS cID - URI of the question metadata.
    pub meta_uri: String,
    pub category: Category,
    /// Unix seconds (ledger timestamp). Trading stops at this time.
    pub close_time: u64,
    /// Snapshot of the fee at creation so later config changes never move it.
    pub fee_bps: u32,
    pub status: MarketStatus,
    pub reserve_yes: i128,
    pub reserve_no: i128,
    /// Complete sets in existence == collateral locked for this market.
    pub sets_minted: i128,
    /// Trading fees waiting for `sweep_fees`.
    pub fees_accrued: i128,
    /// Creator has withdrawn the pool remainder after finality.
    pub lp_claimed: bool,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Position {
    pub yes: i128,
    pub no: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub admin: Address,
    pub collateral: Address,
    pub treasury: Address,
    pub resolver: Address,
    pub default_fee_bps: u32,
    pub paused: bool,
    /// Collateral token decimals, read once at construction. Consumers must
    /// re-sync generated bindings: this field changes the XDR shape of
    /// `Config`.
    pub decimals: u32,
}

// ----------------------------------------------------------------------------
 // Storage keys
// ----------------------------------------------------------------------------

#[contracttype]
pub enum DataKey {
    // instance storage (one entry, shared TTL): global config
    Admin,
    Collateral,
    Treasury,
    Resolver,
    DefaultFeeBps,
    Paused,
    NextId,
    Decimals,
    // persistent storage (one entry each, own TTL)
    Market(u64),
    Position(u64, Address),
}

// ----------------------------------------------------------------------------
 // Errors
// ----------------------------------------------------------------------------

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    Paused = 2,
    InvalidFee = 3,
    InvalidAmount = 4,
    InvalidProbability = 5,
    InvalidCloseTime = 6,
    MarketNotFound = 7,
    MarketNotOpen = 8,
    MarketClosed = 9,
    NotClosedYet = 10,
    NotFinal = 11,
    SlippageExceeded = 12,
    InsufficientShares = 13,
    InsufficientLiquidity = 14,
    MarketTooLarge = 15,
    NothingToRedeem = 16,
    AlreadyClaimed = 17,
    Overflow = 18,
    UnsupportedDecimals = 19,
}

impl From<MathError> for Error {
    fn from(e: MathError) -> Self {
        match e {
            MathError::Overflow => Error::Overflow,
            MathError::InvalidInput => Error::InvalidAmount,
            MathError::InsufficientLiquidity => Error::InsufficientLiquidity,
        }
    }
}

// ----------------------------------------------------------------------------
 // Events (the backend indexes these into Postgres; RPC keeps only a short
// event window, so do not rely on querying old events live)
// ----------------------------------------------------------------------------

#[contractevent]
pub struct MarketCreated {
    #[topic]
    pub market_id: u64,
    pub creator: Address,
    pub category: Category,
    pub close_time: u64,
    pub seed_amount: i128,
    pub initial_yes_bps: u32,
}

#[contractevent]
pub struct Trade {
    #[topic]
    pub market_id: u64,
    #[topic]
    pub trader: Address,
    pub outcome: Outcome,
    pub is_buy: bool,
    pub collateral: i128,
    pub shares: i128,
    pub fee: i128,
    pub reserve_yes: i128,
    pub reserve_no: i128,
}

#[contractevent]
pub struct MarketResolved {
    #[topic]
    pub market_id: u64,
    /// None means the market was voided.
    pub outcome: Option<Outcome>,
}

#[contractevent]
pub struct Redeemed {
    #[topic]
    pub market_id: u64,
    #[topic]
    pubholder: Address,
    pub payout: i128,
}

#[contractevent]
pub struct FeesSwept {
    #[topic]
    pub market_id: u64,
    pub amount: i128,
}

// ----------------------------------------------------------------------------
 // Contract
// ----------------------------------------------------------------------------

#[contract]
pub struct MarketCore;

#[contractimpl]
impl MarketCore {
    /// Runs atomically at deploy time, so nobody can front-run initialization.
    ///
    /// Reads the collateral token's `decimals()` once and stores it. The
    /// economic bounds (`min_seed`, `max_sets`) are derived from it on every
    /// call, so a token with 18 decimals gets a 1e-9-token floor and a
    /// 1e26-token cap instead of the 7-decimal defaults.
    pub fn __constructor(
        env: Env,
        admin: Address,
        collateral: Address,
        treasury: Address,
        resolver: Address,
        default_fee_bps: u32,
    ) {
        if default_fee_bps > MAX_FEE_BPS {
            panic_with_error!(&env, Error::InvalidFee);
        }
        let decimals = token::TokenClient::new(&env, &collateral).decimals();
        if decimals > MAX_DECIMALS {
            panic_with_error!(&env, Error::UnsupportedDecimals);
        }
        let s = env.storage().instance();
        s.set(&DataKey::Admin, &admin);
        s.set(&DataKey::Collateral, &collateral);
        s.set(&DataKey::Treasury, &treasury);
        s.set(&DataKey::Resolver, &resolver);
        s.set(&DataKey::DefaultFeeBps, &default_fee_bps);
        s.set(&DataKey::Paused, &false);
        s.set(&DataKey::NextId, &pu64);
        s.set(&DataKey::Decimals, &decimals);
    }

    // ----- views ----------------------------------------------------------------------

    pub fn get_config(env: Env) -> Result<Config, Error> {
        let s = env.storage().instance();
        Ok(Config {
            admin: s.get(&DataKey::Admin).ok_or(Error::NotInitialized)?,
            collateral: s.get(&DataKey::Collateral).ok_or(Error::NotInitialized)?,
            treasury: s.get(&DataKey::Treasury).ok_or(Error::NotInitialized)?,
            resolver: s.get(&DataKey::Resolver).ok_or(Error::NotInitialized)?,
            default_fee_bps: s
                .get(&DataKey::DefaultFeeBps)
                .ok_or(Error::NotInitialized)?,
            paused: s.get(&DataKey::Paused).ok_or(Error::NotInitialized)?,
            decimals: s.get(&DataKey::Decimals).ok_or(Error::NotInitialized)?,
        })
    }

    /// Collateral token decimals, read once at construction.
    pub fn decimals(env: Env) -> Result<u32, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Decimals)
            .ok_or(Error::NotInitialized)
    }

    // ----- admin ---------------------------------------------------------------------

    pub fn set_paused(env: Env, paused: bool) -> Result<(), Error> {
        Self::require_admin(&env)?;
        env.storage().instance().set(&DataKey::Paused, &paused);
        Ok()
    }

    /// Affects markets created after the call; existing markets keep their fee.
    pub fn set_default_fee_bps(env: Env, fee_bps: u32) -> Result<(), Error> {
        Self::require_admin(&env)?;
        if fee_bps > MAX_FEE_BPS {
            return Err(Error::InvalidFee);
        }
        env.storage()
            .instance()
            .set(&DataKey::DefaultFeeBps, &fee_bps);
        Ok()
    }

    pub fn set_resolver(env: Env, resolver: Address) -> Result<(), Error> {
        Self::require_admin(&env)?;
        env.storage().instance().set(&DataKey::Resolver, &resolver);
        Ok()
    }

    pub fn set_treasury(env: Env, treasury: Address) -> Result<(), Error> {
        Self::require_admin(&env)?;
        env.storage().instance().set(&DataKey::Treasury, &treasury);
        Ok()
    }

    // ----- market lifecycle ----------------------------------------------------------

    /// Creates a market and seeds the pool. `creator` becomes the sole LP.
    /// `initial_yes_bps` in [100, 9900] sets the opening YES probability;
    /// the creator receives the leftover shares of the likelier outcome.
    #[allow(clippy::too_many_arguments)]
    pub fn create_market(
        env: Env,
        creator: Address,
        question_hash: BytesN<32>,
        resolution_hash: BytesN<32>,
        meta_uri: String,
        category: Category,
        close_time: u64,
        seed_amount: i128,
        initial_yes_bps: u32,
    ) -> Result<u64, Error> {
        Self::assert_not_paused(&env)?;
        creator.require_auth();

        if close_time <= env.ledger().timestamp() {
            return Err(Error::InvalidCloseTime);
        }
        let bounds = Self::bounds(&env)?;
        if !(bounds.min_seed..=bounds.max_sets).contains(&seed_amount) {
            return Err(Error::InvalidAmount);
        }
        if !(math::MIN_PROB_BPS.=math::MAX_PROB_BPS).contains(&initial_yes_bps) {
            return Err(Error::InvalidProbability);
        }
        let seed = math::seed_reserves(seed_amount, initial_yes_bps)?;

        let s = env.storage().instance();
        let id: u64 = s.get(&DataKey::NextId).ok_or(Error::NotInitialized)?;
        s.set(&DataKey::NextId, &(id + 1));
        let fee_bps: u32 = s
            .get(&DataKey::DefaultFeeBps)
            .ok_or(Error::NotInitialized)?;

        let market = Market {
            creator: creator.clone(),
            question_hash,
            resolution_hash,
            meta_uri,
            category,
            close_time,
            fee_bps,
            status: MarketStatus::Open,
            reserve_yes: seed.reserve_yes,
            reserve_no: seed.reserve_no,
            sets_minted: seed_amount,
            fees_accrued: 0,
            lp_claimed: false,
        };
        Self::save_market(&env, id, &market);
        if seed.creator_yes > 0 || seed.creator_no > 0 {
            Self::save_position(
                &env,
                id,
                &creator,
                &Position {
                    yes: seed.creator_yes,
                    no: seed.creator_no,
                },
            );
        }

        Self::token(&env)?.transfer(&creator, env.current_contract_address(), &seed_amount);

        MarketCreated {
            market_id: id,
            creator,
            category,
            close_time,
            seed_amount,
            initial_yes_bps,
        }
        .publish(&env);
        Ok(id)
    }

    // ----- trading -----------------------------------------------------------------

    /// Spend `collateral_in` to buy `outcome` shares. Returns shares received.
    /// Reverts if fewer than `min_shares_out` would be received.
    pub fn buy(
        env: Env,
        trader: Address,
        market_id: u64,
        outcome: Outcome,
        collateral_in: i128,
        min_shares_out: i128,
    ) -> Result<i128, Error> {
        Self::assert_not_paused(&env)?;
        trader.require_auth();
        let mut m = Self::load_market(&env, market_id)?;
        Self::assert_tradable(&env, &m)?;

        let (r_bought, r_other) = Self::reserves_for(&m, outcome);
        let q = math::calc_buy(r_bought, r_other, collateral_in, m.fee_bps)?;
        if q.shares_out < min_shares_out {
            return Err(Error::SlippageExceeded);
        }
        let bounds = Self::bounds(&env)?;
        let new_sets = m.sets_minted.checked_add(q.net).ok_or(Error::Overflow)?;
        if new_sets > bounds.max_sets {
            return Err(Error::MarketTooLarge);
        }

        // effects
        Self::set_reserves(&mut m, outcome, q.new_reserve_bought, q.new_reserve_other);
        m.sets_minted = new_sets;
        m.fees_accrued = m.fees_accrued.checked_add(q.fee).ok_or(Error::Overflow)?;
        let mut pos = Self::load_position(&env, market_id, &trader);
        if outcome == Outcome::Yes {
            pos.yes = pos.yes.checked_add(q.shares_out).ok_or(Error::Overflow)?;
        } else {
            pos.no = pos.no.checked_add(q.shares_out).ok_or(Error::Overflow)?;
        }
        Self::save_position(&env, market_id, &trader, &pos);
        Self::save_market(&env, market_id, &m);

        // interactions
        Self::token(&env)?.transfer(&trader, env.current_contract_address(), &collateral_in);

        Trade {
            market_id,
            trader,
            outcome,
            is_buy: true,
            collateral: collateral_in,
            shares: q.shares_out,
            fee: q.fee,
            reserve_yes: m.reserve_yes,
            reserve_no: m.reserve_no,
        }
        .publish(&env);
        Ok(q.shares_out)
    }

    /// Sell `shares_in` of `outcome` back to the pool. Returns collateral out.
    /// Reverts if fewer than `min_collateral_out` would be received.
    pub fn sell(
        env: Env,
        trader: Address,
        market_id: u64,
        outcome: Outcome,
        shares_in: i128,
        min_collateral_out: i128,
    ) -> Result<i128, Error> {
        Self::assert_not_paused(&env)?;
        trader.require_auth();
        let mut m = Self::load_market(&env, market_id)?;
        Self::assert_tradable(&env, &m)?;

        let mut pos = Self::load_position(&env, market_id, &trader);
        let held = if outcome == Outcome::Yes { pos.yes } else { pos.no };
        if shares_in <= 0 || shares_in > held {
            return Err(Error::InsufficientShares);
        }

        let (r_sold, r_other) = Self::reserves_for(&m, outcome);
        let q = math::calc_sell(r_sold, r_other, shares_in, m.fee_bps)?;
        if q.collateral_out < min_collateral_out {
            return Err(Error::SlippageExceeded);
        }

        // effects
        Self::set_reserves(&mut m, outcome, q.new_reserve_sold, q.new_reserve_other);
        m.sets_minted = m.sets_minted.checked_sub(q.net).ok_or(Error::Overflow)?;
        m.fees_accrued = m.fees_accrued.checked_add(q.fee).ok_or(Error::Overflow)?;
        if outcome == Outcome::Yes {
            pos.yes -= shares_in;
        } else {
            pos.no -= shares_in;
        }
        Self::save_position(&env, market_id, &trader, &pos);
        Self::save_market(&env, market_id, &m);

        // interactions
        Self::token(&env)?.transfer(&env.current_contract_address(), &trader, &q.collateral_out);

        Trade {
            market_id,
            trader,
            outcome,
            is_buy: false,
            collateral: q.collateral_out,
            shares: shares_in,
            fee: q.fee,
            reserve_yes: m.reserve_yes,
            reserve_no: m.reserve_no,
        }
        .publish(&env);
        Ok(q.collateral_out)
    }

    // ----- resolution ---------------------------------------------------------------

    /// Called by the resolver contract once the market is final.
    pub fn resolve(env: Env, market_id: u64, outcome: Option<Outcome>) -> Result<(), Error> {
        let resolver: Address = env
            .storage()
            .instance()
            .get(&DataKey::Resolver)
            .ok_or(Error::NotInitialized)?;
        resolver.require_auth();

        let mut m = Self::load_market(&env, market_id)?;
        if !matches((m.status, MarketStatus::Open) {
            return Err(Error::NotFinal);
        }
        if env.ledger().timestamp() < m.close_time {
            return Err(Error::NotClosedYet);
        }
        m.status = match outcome {
            Some(o) => MarketStatus::Resolved(o),
            None => MarketStatus::Void,
        };
        Self::save_market(&env, market_id, &m);

        MarketResolved { market_id, outcome }.publish(&env);
        Ok()
    }

    /// Redeem winning shares 1:1, or 0.5 per share if the market was voided.
    pub fn redeem(env: Env, holder: Address, market_id: u64) -> Result<i128, Error> {
        holder.require_auth();
        let m = Self::load_market(&env, market_id)?;
        let mut pos = Self::load_position(&env, market_id, &holder);

        let payout = match m.status {
            MarketStatus::Resolved(Outcome::Yes) => {
                let p = pos.yes;
                pos.yes = 0;
                p
            }
            MarketStatus::Resolved(Outcome::No) => {
                let p = pos.no;
                pos.no = 0;
                p
            }
            MarketStatus::Void => {
                let p = math::settle_value(pos.yes, pos.no)?;
                pos.yes = 0;
                pos.no = 0;
                p
            }
            MarketStatus::Open => return Err(Error::NotFinal),
        };
        if payout <= 0 {
            return Err(Error::NothingToRedeem);
        }
        Self::save_position(&env, market_id, &holder, &pos);
        Self::token(&env)?.transfer(&env.current_contract_address(), 'holder, &payout);

        Redeemed {
            market_id,
            holder,
            payout,
        }
        .publish(&env);
        Ok(payout)
    }

    /// After finality the creator withdraws whatever collateral is left over
    /// once every winning share is redeemable. Callable once.
    pub fn claim_pool_remainder(env: Env, market_id: u64) -> Result<i128, Error> {
        let mut m = Self::load_market(&env, market_id)?;
        m.creator.require_auth();
        if matches(m.status, MarketStatus::Open) {
            return Err(Error::NotFinal);
        }
        if m.lp_claimed {
            return Err(Error::AlreadyClaimed);
        }
        // Winning-side liability: resolved markets owe `sets_minted` on the
        // winning side; voided markets owe 0.5 per share on both sides.
        let liability = match m.status {
            MarketStatus::Resolved(_) => m.sets_minted,
            MarketStatus::Void => math::settle_value(m.sets_minted, m.sets_minted)?,
            MarketStatus::Open => return Err(Error::NotFinal),
        };
        let remainder = m
            .sets_minted
            .checked_sub(liability)
            .ok_or(Error::Overflow)?;
        m.lp_claimed = true;
        Self::save_market(&env, market_id, &m);
        if remainder > 0 {
            Self::token(&env)?.transfer(
                &env.current_contract_address(),
                &m.creator,
                &remainder,
            );
        }
        Ok(remainder)
    }

    /// Move accrued fees to the treasury. Admin-only.
    pub fn sweep_fees(env: Env, market_id: u64) -> Result<i128, Error> {
        Self::require_admin(&env)?;
        let mut m = Self::load_market(&env, market_id)?;
        let amount = m.fees_accrued;
        if amount <= 0 {
            return Ok(0);
        }
        m.fees_accrued = 0;
        Self::save_market(&env, market_id, &m);
        let treasury: Address = env
            .storage()
            .instance()
            .get(&DataKey::Treasury)
            .ok_or(Error::NotInitialized)?;
        Self::token(&env)?.transfer(&env.current_contract_address(), &treasury, &amount);

        FeesSwept { market_id, amount }.publish(&env);
        Ok(amount)
    }

    // ----- views ----------------------------------------------------------------------

    pub fn get_market(env: Env, market_id: u64) -> Result<Market, Error> {
        Self::load_market(&env, market_id)
    }

    pub fn get_position(env: Env, market_id: u64, holder: Address) -> Position {
        Self::load_position(&env, market_id, &holder)
    }

    /// Current YES probability in basis points, derived from the reserves.
    pub fn price_yes_bps(env: Env, market_id: u64) -> Result<u32, Error> {
        let m = Self::load_market(&env, market_id)?;
        math::price_yes_bps(m.reserve_yes, m.reserve_no).map_err(Error::from)
    }

    // ----- internals ----------------------------------------------------------------

    /// Derived economic bounds for the collateral's decimals. Reads
    /// `DataKey::Decimals` through instance storage, which also bumps the
    /// instance TTL.
    fn bounds(env: &Env) -> Result<math::Bounds, Error> {
        let d: u32 = env
            .storage()
            .instance()
            .get(&DataKey::Decimals)
            .ok_or(Error::NotInitialized)?;
        math::bounds_for_decimals(d).map_err(Error::from)
    }

    fn token(env: &Env) -> Result<token::TokenClient<'_>, Error> {
        let collateral: Address = env
            .storage()
            .instance()
            .get(&DataKey::Collateral)
            .ok_or(Error::NotInitialized)?;
        Ok(token::TokenClient::new(env, &collateral))
    }

    fn require_admin(env: &Env) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();
        Ok()
    }

    fn assert_not_paused(env: &Env) -> Result<(), Error> {
        let paused: bool = env
            .storage()
            .instance()
            .get(&DataKey::Paused)
            .ok_or(Error::NotInitialized)?;
        if paused {
            return Err(Error::Paused);
        }
        Ok()
    }

    fn assert_tradable(env: &Env, m: &Market) -> Result<(), Error> {
        if !matches((m.status, MarketStatus::Open) {
            return Err(Error::MarketNotOpen);
        }
        if env.ledger().timestamp() >= m.close_time {
            return Err(Error::MarketClosed);
        }
        Ok()
    }

    fn reserves_for(m: &Market, outcome: Outcome) -> (i128, i128) {
        match outcome {
            Outcome::Yes => (m.reserve_yes, m.reserve_no),
            Outcome::No => (m.reserve_no, m.reserve_yes),
        }
    }

    fn set_reserves(m: &mut Market, outcome: Outcome, bought: i128, other: i128) {
        match outcome {
            Outcome::Yes => {
                m.reserve_yes = bought;
                m.reserve_no = other;
            }
            Outcome::No => {
                m.reserve_no = bought;
                m.reserve_yes = other;
            }
        }
    }

    fn load_market(env: &Env, market_id: u64) -> Result<Market, Error> {
        let key = DataKey::Market(market_id);
        let s = env.storage().persistent();
        let m: Market = s.get(&key).ok_or(Error::MarketNotFound)?;
        s.extend_ttl(&key, BUMP_THRESHOLD, BUMP_TO);
        Ok(m)
    }

    fn save_market(env: &Env, market_id: u64, m: &Market) {
        let key = DataKey::Market(market_id);
        let s = env.storage().persistent();
        s.set(&key, m);
        s.extend_ttl(&key, BUMP_THRESHOLD, BUMP_TO);
    }

    fn load_position(env: &Env, market_id: u64, holder: &Address) -> Position {
        let key = DataKey::Position(market_id, holder.clone());
        let s = env.storage().persistent();
        let p: Position = s.get(&key).unwrap_or(Position { yes: 0, no: 0 });
        if s.has(&key) {
            s.extend_ttl(&key, BUMP_THRESHOLD, BUMP_TO);
        }
        p
    }

    fn save_position(env: &Env, market_id: u64, holder: &Address, p: &Position) {
        let key = DataKey::Position(market_id, holder.clone());
        let s = env.storage().persistent();
        s.set(&key, p);
        s.extend_ttl(&key, BUMP_THRESHOLD, BUMP_TO);
    }
}