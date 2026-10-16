# Frontend integration

The frontend itself lives in a separate repo (out of scope here). This is
what it needs to know to talk to these contracts.

## Wallet

Use [Freeighter](https://www.freeighter.app/) (or any [SEP-43](https://github.com/stellar/stellar-protocol/blob/master/ecosystem/sep-0043.md)-compatible
wallet) to get the user's public key and sign transactions. The generated
bindings' `AssembledTransaction` already does the simulate → sign → submit
dance; you only need to plug in a `signTransaction` callback:

```ts
import { Client as MarketCoreClient } from "@oracledesk/market-core-bindings";
import { isConnected, getAddress, signTransaction } from "@stellar/freeighter-api";

const contract = new MarketCoreClient({
  contractId: "<MARKET_CORE_CONTRACT_ID>", // from deployments/testnet.json
  networkPassphrase: "Test DF Network ; September 2015",
  rpcUrl: "https://soroban-testnet.stellar.org",
});
```

## Generating / updating the bindings

```bash
./scripts/gen-bindings.sh   # writes packages/bindings/<contract-name>/
```

Regenerate and commit the result after any contract interface change — CI
checks the committed bindings are not stale (`.github/workflows/ci.yml`).
Each package's own `README.md` (written by the `stellar` CLI) has the
generic usage notes; this doc covers the OracleDesk-specific parts.

## Calling `buy` / `sell`

`Outcome` and `Category` are generated as tagged unions matching the
contract's Rust enums:

```ts
type Outcome = { tag: "Yes"; values: void } | { tag: "No"; values: void };
```

```ts
const tx = await contract.buy({
  trader: userAddress,
  market_id: 0n,
  outcome: { tag: "Yes", values: undefined },
  collateral_in: 10_000_000n, // 1 USDC, 7 decimals
  min_shares_out: 0n,         // see slippage note below
});
const { result } = await tx.signAndSend({ signTransaction });
```

## Slippage on `buy`

`quote_buy(market_id, outcome, collateral_in)` returns the exact shares a
buy would receive *right now*. Call it immediately before building the
`buy` transaction, then pass a `min_shares_out` a few bps below that quote
(prices move between simulation and submission).

## Selling an exact share count

Call `sell_shares` to sell an exact number of shares. The contract inverts
the constant-product curve on-chain with an exact integer square root, so the
client never has to read reserves, invert off-chain and pad
`max_shares_in`. The rounding is provably pool-favoring: the seller can
never get more collateral than the curve allows, and `k = r_yes * r_no`
never decreases.

```ts
const tx = await contract.sell_shares({
  trader: userAddress,
  market_id: 0n,
  outcome: { tag: "Yes", values: undefined },
  shares_in: 375_000_00n,       // 37.5 YES shares
  min_collateral_out: 0n,       // see slippage note below
});
const { result } = await tx.signAndSend({ signTransaction });
```

To close an entire position, pass the full held balance as `shares_in`.
Selling the whole position removes the `Position` storage entry when the
other side is also zero, so "Sell all" leaves no dust.

## Slippage on `sell_shares`

`quote_sell_shares(market_id, outcome, shares_in)` returns the exact
collateral a sale would pay *right now*. Call it immediately before
building the `sell_shares` transaction, then pass a `min_collateral_out` a
few bps below that quote. Like `quote_sell`, `quote_sell_shares` does not check
market tradability, so it can be called after a market closes.

The older `sell` entrypoint still takes an exact `collateral_out` and
returns the shares it costs. It remains for backward compatibility, but new
integrations should prefer `sell_shares`.

## Category enum

`Category` is a plain enum (`Crypto | Macro | Geopolitics | Sports | Culture
| Other`) generated from the contract spec — build any category picker off
the bindings' `Category` type directly rather than hand-maintaining a
separate list, so the UI can never drift from what the contract accepts.

## Reading trace data

`reasoning-registry 's `get_trace(trace_id)` returns
`{ market_id, agent, action, trace_hash, ipfs_cid, published_at }`. To show
a verified reasoning trace in the UI:

1. Fetch the trace content from `ipfs_cid` (via a gateway, or through the
   x402 service in `x402/` if it's gated behind payment).
2. Hash the exact bytes received (sha256) and compare to `trace_hash`.
3. Only render the content if the hashes match — never trust `ipfs_cid` content without this check, since IPFS gateways are not authenticated.

`x402/trace-verification.ts` has the reference hash-check implementation the
frontend's check should match.
