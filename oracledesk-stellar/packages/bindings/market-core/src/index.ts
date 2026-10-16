import { Buffer } from "buffer";
import { Address } from "@stellar/stellar-sdk";
import {
  AssembledTransaction,
  Client as ContractClient,
  ClientOptions as ContractClientOptions,
  MethodOptions,
  Result,
  Spec as ContractSpec,
} from "@stellar/stellar-sdk/contract";
import type {
  u32,
  i32,
  u64,
  i64,
  u128,
  i128,
  u256,
  i256,
  Option,
  Timepoint,
  Duration,
} from "@stellar/stellar-sdk/contract";
export * from "@stellar/stellar-sdk";
export * as contract from "@stellar/stellar-sdk/contract";
export * as rpc from "@stellar/stellar-sdk/rpc";

if (typeof window !== "undefined") {
  //@ts-ignore Buffer exists
  window.Buffer = window.Buffer || Buffer;
}





export const Errors = {
  1: {message:"NotInitialized"},
  2: {message:"Paused"},
  3: {message:"InvalidFee"},
  4: {message:"InvalidAmount"},
  5: {message:"InvalidProbability"},
  6: {message:"InvalidCloseTime"},
  7: {message:"MarketNotFound"},
  8: {message:"MarketNotOpen"},
  9: {message:"MarketClosed"},
  10: {message:"NotClosedYet"},
  11: {message:"NotFinal"},
  12: {message:"SlippageExceeded"},
  13: {message:"InsufficientShares"},
  14: {message:"InsufficientLiquidity"},
  15: {message:"MarketTooLarge"},
  16: {message:"NothingToRedeem"},
  17: {message:"AlreadyClaimed"},
  18: {message:"Overflow"}
}



export interface Config {
  admin: string;
  collateral: string;
  default_fee_bps: u32;
  paused: boolean;
  resolver: string;
  treasury: string;
}


export interface Market {
  category: Category;
  /**
 * Unix seconds (ledger timestamp). Trading stops at this time.
 */
  close_time: u64;
  creator: string;
  /**
 * Snapshot of the fee at creation so later config changes never move it.
 */
  fee_bps: u32;
  /**
 * Trading fees waiting for `sweep_fees`.
 */
  fees_accrued: i128;
  /**
 * Creator has withdrawn the pool remainder after finality.
 */
  lp_claimed: boolean;
  /**
 * IPFS CID / URI of the question metadata.
 */
  meta_uri: string;
  /**
 * sha256 of the canonical question JSON stored off-chain (IPFS), so a
 * client can prove the text it shows is the text that was committed.
 */
  question_hash: Buffer;
  reserve_no: i128;
  reserve_yes: i128;
  /**
 * Commitment to how this market will be resolved, fixed at creation so
 * the creator cannot pick favorable resolution rules after seeing
 * trading activity. The resolver contract verifies a revealed spec
 * against this hash before configuring itself for the market. See
 * `docs/adr/0001-resolution-commitment.md`.
 */
  resolution_hash: Buffer;
  /**
 * Complete sets in existence == collateral locked for this market.
 */
  sets_minted: i128;
  status: MarketStatus;
}

export type DataKey = {tag: "Admin", values: void} | {tag: "Collateral", values: void} | {tag: "Treasury", values: void} | {tag: "Resolver", values: void} | {tag: "DefaultFeeBps", values: void} | {tag: "Paused", values: void} | {tag: "NextId", values: void} | {tag: "Market", values: readonly [u64]} | {tag: "Position", values: readonly [u64, string]};

export type Outcome = {tag: "Yes", values: void} | {tag: "No", values: void};

export type Category = {tag: "Crypto", values: void} | {tag: "Macro", values: void} | {tag: "Geopolitics", values: void} | {tag: "Sports", values: void} | {tag: "Culture", values: void} | {tag: "Other", values: void};


export interface Position {
  no: i128;
  yes: i128;
}



export type MarketStatus = {tag: "Open", values: void} | {tag: "Resolved", values: readonly [Outcome]} | {tag: "Void", values: void};



export interface Client {
  /**
   * Construct and simulate a buy transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Spend `collateral_in` to buy `outcome` shares. Returns shares received.
   * Reverts if fewer than `min_shares_out` would be received.
   */
  buy: ({trader, market_id, outcome, collateral_in, min_shares_out}: {trader: string, market_id: u64, outcome: Outcome, collateral_in: i128, min_shares_out: i128}, options?: MethodOptions) => Promise<AssembledTransaction<Result<i128>>>

  /**
   * Construct and simulate a sell transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Receive exactly `collateral_out` by selling `outcome` shares. Returns
   * shares sold. Reverts if more than `max_shares_in` would be needed.
   * Clients turn "sell N shares" into `collateral_out` off-chain
   * (see `math::calc_sell` docs) and add a slippage margin to `max_shares_in`.
   */
  sell: ({trader, market_id, outcome, collateral_out, max_shares_in}: {trader: string, market_id: u64, outcome: Outcome, collateral_out: i128, max_shares_in: i128}, options?: MethodOptions) => Promise<AssembledTransaction<Result<i128>>>

  /**
   * Construct and simulate a sell_shares transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Sell exactly `shares_in` of `outcome` and receive the collateral the
   * curve allows. Returns `collateral_out`. Reverts if less than
   * `min_collateral_out` would be received. Rounding is pool-favoring:
   * `k = r_yes * r_no` never decreases and the payout never exceeds what
   * `sell` would charge the same share count for.
   */
  sell_shares: ({trader, market_id, outcome, shares_in, min_collateral_out}: {trader: string, market_id: u64, outcome: Outcome, shares_in: i128, min_collateral_out: i128}, options?: MethodOptions) => Promise<AssembledTransaction<Result<i128>>>

  /**
   * Construct and simulate a redeem transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Pays out `holder`'s position for a final market and clears it.
   * Losing positions clear with a payout of 0.
   */
  redeem: ({holder, market_id}: {holder: string, market_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Result<i128>>>

  /**
   * Construct and simulate a resolve transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  resolve: ({market_id, outcome}: {market_id: u64, outcome: Outcome}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>

  /**
   * Construct and simulate a get_price transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Spot price of `outcome` in bps (0..=10_000). YES + NO ~ 10_000.
   */
  get_price: ({market_id, outcome}: {market_id: u64, outcome: Outcome}, options?: MethodOptions) => Promise<AssembledTransaction<Result<u32>>>

  /**
   * Construct and simulate a quote_buy transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Shares a `buy` of `collateral_in` would return right now.
   */
  quote_buy: ({market_id, outcome, collateral_in}: {market_id: u64, outcome: Outcome, collateral_in: i128}, options?: MethodOptions) => Promise<AssembledTransaction<Result<i128>>>

  /**
   * Construct and simulate a get_config transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_config: (options?: MethodOptions) => Promise<AssembledTransaction<Result<Config>>>

  /**
   * Construct and simulate a get_market transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_market: ({market_id}: {market_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Result<Market>>>

  /**
   * Construct and simulate a quote_sell transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Shares a `sell` for exactly `collateral_out` would consume right now.
   */
  quote_sell: ({market_id, outcome, collateral_out}: {market_id: u64, outcome: Outcome, collateral_out: i128}, options?: MethodOptions) => Promise<AssembledTransaction<Result<i128>>>

  /**
   * Construct and simulate a quote_sell_shares transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Collateral a `sell_shares` of exactly `shares_in` would return right
   * now. Does not check tradability, consistent with `quote_sell`.
   */
  quote_sell_shares: ({market_id, outcome, shares_in}: {market_id: u64, outcome: Outcome, shares_in: i128}, options?: MethodOptions) => Promise<AssembledTransaction<Result<i128>>>

  /**
   * Construct and simulate a set_paused transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  set_paused: ({paused}: {paused: boolean}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a sweep_fees transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Anyone can move accrued fees to the treasury.
   */
  sweep_fees: ({market_id}: {market_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Result<i128>>>

  /**
   * Construct and simulate a void_market transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Invalid or unresolvable market. Allowed any time before finality.
   */
  void_market: ({market_id}: {market_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a get_position transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_position: ({market_id, holder}: {market_id: u64, holder: string}, options?: MethodOptions) => Promise<AssembledTransaction<Position>>

  /**
   * Construct and simulate a market_count transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  market_count: (options?: MethodOptions) => Promise<AssembledTransaction<u64>>

  /**
   * Construct and simulate a set_resolver transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  set_resolver: ({resolver}: {resolver: string}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a set_treasury transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  set_treasury: ({treasury}: {treasury: string}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a set_fee transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  set_fee: ({fee_bps}: {fee_bps: u32}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a set_admin transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  set_admin: ({admin}: {admin: string}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a claim_lp transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  claim_lp: ({market_id}: {market_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Result<i128>>>

  /**
   * Construct and simulate a create_market transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  create_market: ({creator, question_hash, meta_uri, close_time, category, resolution_hash, fee_bps}: {creator: string, question_hash: Buffer, meta_uri: string, close_time: u64, category: Category, resolution_hash: Buffer, fee_bps: Option<u32>}, options?: MethodOptions) => Promise<AssembledTransaction<Result<u64>>>

  /**
   * Construct and simulate a initialize transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  initialize: ({admin, collateral, treasury, resolver, default_fee_bps}: {admin: string, collateral: string, treasury: string, resolver: string, default_fee_bps: u32}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>
}

export class Client extends ContractClient {
  readonly options: ContractClientOptions;

  constructor(options: ContractClientOptions) {
    super(new ContractSpec(metadata));
    this.options = options;
  }

  buy = async ({trader, market_id, outcome, collateral_in, min_shares_out}: {trader: string, market_id: u64, outcome: Outcome, collateral_in: i128, min_shares_out: i128}, options?: MethodOptions): Promise<AssembledTransaction<Result<i128>>> => {
    return this.call("buy", {trader, market_id, outcome, collateral_in, min_shares_out}, options);
  }

  sell = async ({trader, market_id, outcome, collateral_out, max_shares_in}: {trader: string, market_id: u64, outcome: Outcome, collateral_out: i128, max_shares_in: i128}, options?: MethodOptions): Promise<AssembledTransaction<Result<i128>>> => {
    return this.call("sell", {trader, market_id, outcome, collateral_out, max_shares_in}, options);
  }

  sell_shares = async ({trader, market_id, outcome, shares_in, min_collateral_out}: {trader: string, market_id: u64, outcome: Outcome, shares_in: i128, min_collateral_out: i128}, options?: MethodOptions): Promise<AssembledTransaction<Result<i128>>> => {
    return this.call("sell_shares", {trader, market_id, outcome, shares_in, min_collateral_out}, options);
  }

  redeem = async ({holder, market_id}: {holder: string, market_id: u64}, options?: MethodOptions): Promise<AssembledTransaction<Result<i128>>> => {
    return this.call("redeem", {holder, market_id}, options);
  }

  resolve = async ({market_id, outcome}: {market_id: u64, outcome: Outcome}, options?: MethodOptions): Promise<AssembledTransaction<Result<void>>> => {
    return this.call("resolve", {market_id, outcome}, options);
  }

  get_price = async ({market_id, outcome}: {market_id: u64, outcome: Outcome}, options?: MethodOptions): Promise<AssembledTransaction<Result<u32>>> => {
    return this.call("get_price", {market_id, outcome}, options);
  }

  quote_buy = async ({market_id, outcome, collateral_in}: {market_id: u64, outcome: Outcome, collateral_in: i128}, options?: MethodOptions): Promise<AssembledTransaction<Result<i128>>> => {
    return this.call("quote_buy", {market_id, outcome, collateral_in}, options);
  }

  get_config = async (options?: MethodOptions): Promise<AssembledTransaction<Result<Config>>> => {
    return this.call("get_config", {}, options);
  }

  get_market = async ({market_id}: {market_id: u64}, options?: MethodOptions): Promise<AssembledTransaction<Result<Market>>> => {
    return this.call("get_market", {market_id}, options);
  }

  quote_sell = async ({market_id, outcome, collateral_out}: {market_id: u64, outcome: Outcome, collateral_out: i128}, options?: MethodOptions): Promise<AssembledTransaction<Result<i128>>> => {
    return this.call("quote_sell", {market_id, outcome, collateral_out}, options);
  }

  quote_sell_shares = async ({market_id, outcome, shares_in}: {market_id: u64, outcome: Outcome, shares_in: i128}, options?: MethodOptions): Promise<AssembledTransaction<Result<i128>>> => {
    return this.call("quote_sell_shares", {market_id, outcome, shares_in}, options);
  }

  set_paused = async ({paused}: {paused: boolean}, options?: MethodOptions): Promise<AssembledTransaction<Result<void>>> => {
    return this.call("set_paused", {paused}, options);
  }

  sweep_fees = async ({market_id}: {market_id: u64}, options?: MethodOptions): Promise<AssembledTransaction<Result<i128>>> => {
    return this.call("sweep_fees", {market_id}, options);
  }

  void_market = async ({market_id}: {market_id: u64}, options?: MethodOptions): Promise<AssembledTransaction<Result<void>>> => {
    return this.call("void_market", {market_id}, options);
  }

  get_position = async ({market_id, holder}: {market_id: u64, holder: string}, options?: MethodOptions): Promise<AssembledTransaction<Position>> => {
    return this.call("get_position", {market_id, holder}, options);
  }

  market_count = async (options?: MethodOptions): Promise<AssembledTransaction<u64>> => {
    return this.call("market_count", {}, options);
  }

  set_resolver = async ({resolver}: {resolver: string}, options?: MethodOptions): Promise<AssembledTransaction<Result<void>>> => {
    return this.call("set_resolver", {resolver}, options);
  }

  set_treasury = async ({treasury}: {treasury: string}, options?: MethodOptions): Promise<AssembledTransaction<Result<void>>> => {
    return this.call("set_treasury", {treasury}, options);
  }

  set_fee = async ({fee_bps}: {fee_bps: u32}, options?: MethodOptions): Promise<AssembledTransaction<Result<void>>> => {
    return this.call("set_fee", {fee_bps}, options);
  }

  set_admin = async ({admin}: {admin: string}, options?: MethodOptions): Promise<AssembledTransaction<Result<void>>> => {
    return this.call("set_admin", {admin}, options);
  }

  claim_lp = async ({market_id}: {market_id: u64}, options?: MethodOptions): Promise<AssembledTransaction<Result<i128>>> => {
    return this.call("claim_lp", {market_id}, options);
  }

  create_market = async ({creator, question_hash, meta_uri, close_time, category, resolution_hash, fee_bps}: {creator: string, question_hash: Buffer, meta_uri: string, close_time: u64, category: Category, resolution_hash: Buffer, fee_bps: Option<u32>}, options?: MethodOptions): Promise<AssembledTransaction<Result<u64>>> => {
    return this.call("create_market", {creator, question_hash, meta_uri, close_time, category, resolution_hash, fee_bps}, options);
  }

  initialize = async ({admin, collateral, treasury, resolver, default_fee_bps}: {admin: string, collateral: string, treasury: string, resolver: string, default_fee_bps: u32}, options?: MethodOptions): Promise<AssembledTransaction<Result<void>>> => {
    return this.call("initialize", {admin, collateral, treasury, resolver, default_fee_bps}, options);
  }
}

export const networks = {
  testnet: {
    networkPassphrase: "Test S CF Network ; September 2015",
    contractId: "",
  },
  futurenet: {
    networkPassphrase: "Test S CF Network ; September 2015",
    contractId: "",
  },
  mainnet: {
    networkPassphrase: "Public Global Key Network ; September 2015",
    contractId: "",
  },
}

export const metadata = {
  "entries": [
    {
      "type": "function",
      "name": "buy",
      "inputs": [
        {
          "name": "trader",
          "type": {
            "type": "address"
          }
        },
        {
          "name": "market_id",
          "type": {
            "type": "u64"
          }
        },
        {
          "name": "outcome",
          "type": {
            "type": "custom",
            "name": "Outcome"
          }
        },
        {
          "name": "collateral_in",
          "type": {
            "type": "i128"
          }
        },
        {
          "name": "min_shares_out",
          "type": {
            "type": "i128"
          }
        }
      ],
      "outputs": [
        {
          "type": "result",
          "value": {
            "type": "i128"
          }
        }
      ]
    },
    {
      "type": "function",
      "name": "sell",
      "inputs": [
        {
          "name": "trader",
          "type": {
            "type": "address"
          }
        },
        {
          "name": "market_id",
          "type": {
            "type": "u64"
          }
        },
        {
          "name": "outcome",
          "type": {
            "type": "custom",
            "name": "Outcome"
          }
        },
        {
          "name": "collateral_out",
          "type": {
            "type": "i128"
          }
        },
        {
          "name": "max_shares_in",
          "type": {
            "type": "i128"
          }
        }
      ],
      "outputs": [
        {
          "type": "result",
          "value": {
            "type": "i128"
          }
        }
      ]
    },
    {
      "type": "function",
      "name": "sell_shares",
      "inputs": [
        {
          "name": "trader",
          "type": {
            "type": "address"
          }
        },
        {
          "name": "market_id",
          "type": {
            "type": "u64"
          }
        },
        {
          "name": "outcome",
          "type": {
            "type": "custom",
            "name": "Outcome"
          }
        },
        {
          "name": "shares_in",
          "type": {
            "type": "i128"
          }
        },
        {
          "name": "min_collateral_out",
          "type": {
            "type": "i128"
          }
        }
      ],
      "outputs": [
        {
          "type": "result",
          "value": {
            "type": "i128"
          }
        }
      ]
    },
    {
      "type": "function",
      "name": "redeem",
      "inputs": [
        {
          "name": "holder",
          "type": {
            "type": "address"
          }
        },
        {
          "name": "market_id",
          "type": {
            "type": "u64"
          }
        }
      ],
      "outputs": [
        {
          "type": "result",
          "value": {
            "type": "i128"
          }
        }
      ]
    },
    {
      "type": "function",
      "name": "resolve",
      "inputs": [
        {
          "name": "market_id",
          "type": {
            "type": "u64"
          }
        },
        {
          "name": "outcome",
          "type": {
            "type": "custom",
            "name": "Outcome"
          }
        }
      ],
      "outputs": [
        {
          "type": "result",
          "value": {
            "type": "void"
          }
        }
      ]
    },
    {
      "type": "function",
      "name": "get_price",
      "inputs": [
        {
          "name": "market_id",
          "type": {
            "type": "u64"
          }
        },
        {
          "name": "outcome",
          "type": {
            "type": "custom",
            "name": "Outcome"
          }
        }
      ],
      "outputs": [
        {
          "type": "result",
          "value": {
            "type": "u32"
          }
        }
      ]
    },
    {
      "type": "function",
      "name": "quote_buy",
      "inputs": [
        {
          "name": "market_id",
          "type": {
            "type": "u64"
          }
        },
        {
          "name": "outcome",
          "type": {
            "type": "custom",
            "name": "Outcome"
          }
        },
        {
          "name": "collateral_in",
          "type": {
            "type": "i128"
          }
        }
      ],
      "outputs": [
        {
          "type": "result",
          "value": {
            "type": "i128"
          }
        }
      ]
    },
    {
      "type": "function",
      "name": "get_config",
      "inputs": [],
      "outputs": [
        {
          "type": "result",
          "value": {
            "type": "custom",
            "name": "Config"
          }
        }
      ]
    },
    {
      "type": "function",
      "name": "get_market",
      "inputs": [
        {
          "name": "market_id",
          "type": {
            "type": "u64"
          }
        }
      ],
      "outputs": [
        {
          "type": "result",
          "value": {
            "type": "custom",
            "name": "Market"
          }
        }
      ]
    },
    {
      "type": "function",
      "name": "quote_sell",
      "inputs": [
        {
          "name": "market_id",
          "type": {
            "type": "u64"
          }
        },
        {
          "name": "outcome",
          "type": {
            "type": "custom",
            "name": "Outcome"
          }
        },
        {
          "name": "collateral_out",
          "type": {
            "type": "i128"
          }
        }
      ],
      "outputs": [
        {
          "type": "result",
          "value": {
            "type": "i128"
          }
        }
      ]
    },
    {
      "type": "function",
      "name": "quote_sell_shares",
      "inputs": [
        {
          "name": "market_id",
          "type": {
            "type": "u64"
          }
        },
        {
          "name": "outcome",
          "type": {
            "type": "custom",
            "name": "Outcome"
          }
        },
        {
          "name": "shares_in",
          "type": {
            "type": "i128"
          }
        }
      ],
      "outputs": [
        {
          "type": "result",
          "value": {
            "type": "i128"
          }
        }
      ]
    },
    {
      "type": "function",
      "name": "set_paused",
      "inputs": [
        {
          "name": "paused",
          "type": {
            "type": "boolean"
          }
        }
      ],
      "outputs": [
        {
          "type": "result",
          "value": {
            "type": "void"
          }
        }
      ]
    },
    {
      "type": "function",
      "name": "sweep_fees",
      "inputs": [
        {
          "name": "market_id",
          "type": {
            "type": "u64"
          }
        }
      ],
      "outputs": [
        {
          "type": "result",
          "value": {
            "type": "i128"
          }
        }
      ]
    },
    {
      "type": "function",
      "name": "void_market",
      "inputs": [
        {
          "name": "market_id",
          "type": {
            "type": "u64"
          }
        }
      ],
      "outputs": [
        {
          "type": "result",
          "value": {
            "type": "void"
          }
        }
      ]
    },
    {
      "type": "function",
      "name": "get_position",
      "inputs": [
        {
          "name": "market_id",
          "type": {
            "type": "u64"
          }
        },
        {
          "name": "holder",
          "type": {
            "type": "address"
          }
        }
      ],
      "outputs": [
        {
          "type": "custom",
          "name": "Position"
        }
      ]
    },
    {
      "type": "function",
      "name": "market_count",
      "inputs": [],
      "outputs": [
        {
          "type": "u64"
        }
      ]
    },
    {
      "type": "function",
      "name": "set_resolver",
      "inputs": [
        {
          "name": "resolver",
          "type": {
            "type": "address"
          }
        }
      ],
      "outputs": [
        {
          "type": "result",
          "value": {
            "type": "void"
          }
        }
      ]
    },
    {
      "type": "function",
      "name": "set_treasury",
      "inputs": [
        {
          "name": "treasury",
          "type": {
            "type": "address"
          }
        }
      ],
      "outputs": [
        {
          "type": "result",
          "value": {
            "type": "void"
          }
        }
      ]
    },
    {
      "type": "function",
      "name": "set_fee",
      "inputs": [
        {
          "name": "fee_bps",
          "type": {
            "type": "u32"
          }
        }
      ],
      "outputs": [
        {
          "type": "result",
          "value": {
            "type": "void"
          }
        }
      ]
    },
    {
      "type": "function",
      "name": "set_admin",
      "inputs": [
        {
          "name": "admin",
          "type": {
            "type": "address"
          }
        }
      ],
      "outputs": [
        {
          "type": "result",
          "value": {
            "type": "void"
          }
        }
      ]
    },
    {
      "type": "function",
      "name": "claim_lp",
      "inputs": [
        {
          "name": "market_id",
          "type": {
            "type": "u64"
          }
        }
      ],
      "outputs": [
        {
          "type": "result",
          "value": {
            "type": "i128"
          }
        }
      ]
    },
    {
      "type": "function",
      "name": "create_market",
      "inputs": [
        {
          "name": "creator",
          "type": {
            "type": "address"
          }
        },
        {
          "name": "question_hash",
          "type": {
            "type": "bytes"
          }
        },
        {
          "name": "meta_uri",
          "type": "string"
        },
        {
          "name": "close_time",
          "type": {
            "type": "u64"
          }
        },
        {
          "name": "category",
          "type": {
            "type": "custom",
            "name": "Category"
          }
        },
        {
          "name": "resolution_hash",
          "type": {
            "type": "bytes"
          }
        },
        {
          "name": "fee_bps",
          "type": {
            "type": "option",
            "value": {
              "type": "u32"
            }
          }
        }
      ],
      "outputs": [
        {
          "type": "result",
          "value": {
            "type": "u64"
          }
        }
      ]
    },
    {
      "type": "function",
      "name": "initialize",
      "inputs": [
        {
          "name": "admin",
          "type": {
            "type": "address"
          }
        },
        {
          "name": "collateral",
          "type": {
            "type": "address"
          }
        },
        {
          "name": "treasury",
          "type": {
            "type": "address"
          }
        },
        {
          "name": "resolver",
          "type": {
            "type": "address"
          }
        },
        {
          "name": "default_fee_bps",
          "type": {
            "type": "u32"
          }
        }
      ],
      "outputs": [
        {
          "type": "result",
          "value": {
            "type": "void"
          }
        }
      ]
    },
    {
      "type": "event",
      "name": "Trade",
      "inputs": [
        {
          "name": "trader",
          "type": {
            "type": "address"
          }
        },
        {
          "name": "market_id",
          "type": {
            "type": "u64"
          }
        },
        {
          "name": "outcome",
          "type": {
            "type": "custom",
            "name": "Outcome"
          }
        },
        {
          "name": "is_buy",
          "type": {
            "type": "boolean"
          }
        },
        {
          "name": "shares",
          "type": {
            "type": "i128"
          }
        },
        {
          "name": "collateral",
          "type": {
            "type": "i128"
          }
        },
        {
          "name": "fee",
          "type": {
            "type": "i128"
          }
        }
      ]
    },
    {
      "type": "enum",
      "name": "Outcome",
      "cases": [
        {
          "name": "Yes"
        },
        {
          "name": "No"
        }
      ]
    },
    {
      "type": "enum",
      "name": "Category",
      "cases": [
        {
          "name": "Crypto"
        },
        {
          "name": "Macro"
        },
        {
          "name": "Geopolitics"
        },
        {
          "name": "Sports"
        },
        {
          "name": "Culture"
        },
        {
          "name": "Other"
        }
      ]
    },
    {
      "type": "struct",
      "name": "Config",
      "fields": [
        {
          "name": "admin",
          "type": {
            "type": "address"
          }
        },
        {
          "name": "collateral",
          "type": {
            "type": "address"
          }
        },
        {
          "name": "default_fee_bps",
          "type": {
            "type": "u32"
          }
        },
        {
          "name": "paused",
          "type": {
            "type": "boolean"
          }
        },
        {
          "name": "resolver",
          "type": {
            "type": "address"
          }
        },
        {
          "name": "treasury",
          "type": {
            "type": "address"
          }
        }
      ]
    },
    {
      "type": "struct",
      "name": "Market",
      "fields": [
        {
          "name": "category",
          "type": {
            "type": "custom",
            "name": "Category"
          }
        },
        {
          "name": "close_time",
          "type": {
            "type": "u64"
          }
        },
        {
          "name": "creator",
          "type": {
            "type": "address"
          }
        },
        {
          "name": "fee_bps",
          "type": {
            "type": "u32"
          }
        },
        {
          "name": "fees_accrued",
          "type": {
            "type": "i128"
          }
        },
        {
          "name": "lp_claimed",
          "type": {
            "type": "boolean"
          }
        },
        {
          "name": "meta_uri",
          "type": {
            "type": "string"
          }
        },
        {
          "name": "question_hash",
          "type": {
            "type": "bytes"
          }
        },
        {
          "name": "reserve_no",
          "type": {
            "type": "i128"
          }
        },
        {
          "name": "reserve_yes",
          "type": {
            "type": "i128"
          }
        },
        {
          "name": "resolution_hash",
          "type": {
            "type": "bytes"
          }
        },
        {
          "name": "sets_minted",
          "type": {
            "type": "i128"
          }
        },
        {
          "name": "status",
          "type": {
            "type": "custom",
            "name": "MarketStatus"
          }
        }
      ]
    },
    {
      "type": "struct",
      "name": "Position",
      "fields": [
        {
          "name": "no",
          "type": {
            "type": "i128"
          }
        },
        {
          "name": "yes",
          "type": {
            "type": "i128"
          }
        }
      ]
    },
    {
      "type": "enum",
      "name": "MarketStatus",
      "cases": [
        {
          "name": "Open"
        },
        {
          "name": "Resolved",
          "cases": [
            {
              "name": "__0",
              "type": {
                "type": "custom",
                "name": "Outcome"
              }
            }
          ]
        },
        {
          "name": "Void"
        }
      ]
    },
    {
      "type": "error",
      "name": "Error",
      "cases": [
        {
          "name": "NotInitialized",
          "value": 1
        },
        {
          "name": "Paused",
          "value": 2
        },
        {
          "name": "InvalidFee",
          "value": 3
        },
        {
          "name": "InvalidAmount",
          "value": 4
        },
        {
          "name": "InvalidProbability",
          "value": 5
        },
        {
          "name": "InvalidCloseTime",
          "value": 6
        },
        {
          "name": "MarketNotFound",
          "value": 7
        },
        {
          "name": "MarketNotOpen",
          "value": 8
        },
        {
          "name": "MarketClosed",
          "value": 9
        },
        {
          "name": "NotClosedYet",
          "value": 10
        },
        {
          "name": "NotFinal",
          "value": 11
        },
        {
          "name": "SlippageExceeded",
          "value": 12
        },
        {
          "name": "InsufficientShares",
          "value": 13
        },
        {
          "name": "InsufficientLiquidity",
          "value": 14
        },
        {
          "name": "MarketTooLarge",
          "value": 15
        },
        {
          "name": "NothingToRedeem",
          "value": 16
        },
        {
          "name": "AlreadyClaimed",
          "value": 17
        },
        {
          "name": "Overflow",
          "value": 18
        }
      ]
    }
  ]
}