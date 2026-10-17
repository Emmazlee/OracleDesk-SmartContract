# Status

Last updated: 2026-09-21 (contributor addenda 2026-10-16 and 2026-10-17 —
see the first section after this intro). This is the honest account of what
works, what's unverified, and what's left. Everything marked "verified" was
demonstrated with real command output in the session that wrote this file,
not asserted.

## Contributor addendum: 2026-10-16

Fee split + recipient snapshot (#8), per-market circuit breakers (#10),
trade deadlines and limit prices (#12), and transferable positions/LP plus
treasury migration (#13) are implemented on
`feat/fees-breakers-bounds-and-transfers` (PR against `develop`; design
notes in `docs/architecture.md`). Verified in the contributor session:
`cargo fmt --check`, `cargo clippy --workspace --all-targets - -D warnings`
[and `cargo test --workspace` (now **87** tests: market-core 54, resolver 6,
reasoning-registry 6, treasury 21 — superseding the 40-test count below),
`python3 model/fpmm_model.py`, agents (36 tests) and x402 (5 tests) with the
regenerated bindings, `make build` and `make bindings` with only the
intended diffs. **Not** (re)verified: no testnet redeploy — the deployed
contracts and `deployments/testnet.json` predate these changes, so the new
entrypoints have contract-level and off-chain tests only, not a real-ledger
transaction.

### Merge follow-up: 2026-10-17

The work above landed on `develop` as merge commit `581a3ec` (PR #153,
merged 2026-10-16), so the feature-branch reference above is historical and
`develop` is the source of truth for #8, #10, #12 and #13.

## Contributor addendum: decimals-aware collateral

market-core is now decimals-aware. The collateral token's `decimals()` is read
once in `__constructor` and stored under `DataKey::Decimals` (instance). The
supported range is `0 ..= 18`; anything higher panics with the new
`Error::UnsupportedDecimals = 19`. `GetConfig` now returns `decimals` in `Config`,
so the XDR shape of `Config` changed — consumers must re-sync their generated
bindings.

The economic bounds are derived from decimals instead of hardcoded at 7
decimals:

- `min_seed = 100 * 10^d` (100 tokens)
- `max_sets = 1e8 * 10^d` (100M tokens)

The contract reads these through `Self::bounds(&env)` within
`math::bounds_for_decimals`. The old `math::MAX_SETS` constant is kept only as a
`[deprecated]` 7-decimal alias for tests and is no longer used by contract logic.

`math::mul_div_floor` / `math::mul_div_ceil` now compute their products through
a 256-bit intermediate (`mod wide`, a no-std `u128x2` implementation), so a
result that fits in `i128` is always returned exactly. Rounding directions
unchanged and covered by a differential test against the legacy
implementation. The module remains Env-free and its tests run natively.

The constants and the comment in `math.rs` that justified the old i128 products
by a 1e30 headroom argument are gone; correctness no longer depends on the
decimals of the collateral.

`settle_value` now uses checked addition and returns `Result`; `redeem` and
`claim_pool_remainder` propagate the error.

The Python model (`model/fpmm_model.py`) and the TypeScript mirror
(`agents/core/fpmm.ts`) now expose `bounds_for_decimals` / `boundsForDecimals`
with an explicit `i128` fit check in the multiply-divide helpers, and 18-decimal
fixtures that match the contract outputs exactly.

The vendored `market_core.wasm` copies in `contracts/resolver` and
`contracts/treasury` were refreshed, and all four bindings packages were
regenerated. The CPU instruction delta for `buy` at 7 vs. 18 decimals is reported
in the PR description.

## Phase 0: recon and safety

**Toolchain** (as installed in the environment this was built in):
rustc 1.94.1, cargo 1.94.1, Stellar CLI 27.1.0, soroban-sdk 27.0.6, Node
v24.13.1, npm 11.8.0, Python 3.14.5, jq 1.7. `soroban` (the old ICL name),
`gitleaks`, and `trufflehog` were not installed.

**Inventory of inherited code** (`soroban/` in the parent repo, moved into
`contracts/` here): `market-core` compiled and had tests already; the
`resolution_hash` commitment described in the design brief was *not* yet
implemented (added — see below). `reasoning-registry`, `resolver`, and
`treasury` all had substantive implementations already, not stubs, but
`treasury`'s "trading" functions (`authorize_trade`/`close_trade`) were
advisory-only bookkeeping that never actually called `market-core` — a real
gap, since it meant the on-chain caps didn't cap anything real. Rewritten*
— see below.

**Secrets scan** : manual regex over the whole `OracleDesk-SmartContract`
working tree for `PRIVATE_KEY`/`SECRET`/PEM headers/Stellar seed shape
(`S[A-Z2-7]{55}`), plus `git log -p --all` over the same patterns. No real
secrets found — the only hits were env var *names* (e.g. `DEPLOYER_PRIVATE_KEY= `in the parent repo's README, referencing the Arc-era Foundry deploy flow,
not this repo). One `.env` file exists at `oracledesk-contracts/.env`
(the sibling Arc-era repo, not this one); confirmed gitignored via
`git check-ignore`, never read, never committed. No `.env` files exist
anywhere under `oracledesk-stellar/`.

## Phase-by-phase results

| Phase | Status | Evidence |
|---|---|---|
| 0. Recon & safety | ✅ Done | This section |
| 1. Scaffold | ✅ Done | `contracts/`, `agents/`, `x402/`, `packages/bindings/`, `scripts/`, `docs/` all populated per the target layout; no Solidity/Foundry/Polymarket-execution/Circle code anywhere in this tree |
| 2. Contracts compile + test | ✅ Done | `cargo fmt --check`, `cargo clippy --workspace --all-targets - -D warnings`, `cargo test --workspace` (40 tests) all exit 0 — rerun as the last step before writing this file |
| 3. Testnet deploy | ✅ Done | All 4 contracts + a self-issued test USDC deployed to testnet; addresses in `deployments/testnet.json`; `scripts/deploy-testnet.sh` re-run and confirmed idempotent (detects everything already deployed, no-ops) |
| 4. x402 service | ✅ Code done, ⚠️ network round-trip unverified | See "x402" below |
| 5. Agents + demo | ✅ Done | `scripts/demo.sh` ran end-to-end on real testnet: create → trade → resolve → record trace → verify hash. `agents/` CLIs ran in dry-run mode successfully; live mode is implemented but not exercised (see below) |
| 6. Bindings | ✅ Mostly verified | See "Bindings" below |
| 7. Contributor readiness | ✅ Done | This file plus READMF/CONTRIBUTING/CODE_OF_CONDUCT/SECURITY/LICENSE/CI/issue templates/15 wave-issues |
| 8. Final report | ✅ This file | |

## What's fully verified (command output exists for all of these)

- `cargo fmt --check`, `cargo clippy --workspace --all-targets - -D
  warnings`, `cargo test --workspace` (40 tests across `market-core`,
  `resolver`, `reasoning-registry`, `treasury`) — all exit 0.
- `stellar contract build` produces all four `.wasm` files cleanly, with no
  spec warnings (the `experimental_spec_shaking_v2` soroban-sdl feature was
  needed to get `Outcome`/`Category` properly represented in
  `resolver`'s/`treasury`'s own contract spec — see AD 0002).
- `python3 model/fpmm_model.py` passes (known-value checks against the same
  worked examples as the Rust tests, a property test, and a
  `gross_for_shares`/`calc_sell` round-trip check).
- `cd agents && npm test` — 24 tests pass, including the same known-value
  and property checks in TypeScript/BigInt (`agents/core/fpmm.ts`).
- `cd x402 && npm test` — 5 tests pass, including the new registry-backed
  `TraceRepository`.
- Live testnet deployment and the full demo loop
  (`scripts/deploy-testnet.sh`, `scripts/seed-market.sh`,
  `scripts/demo.sh`) — real transaction hashes were produced for: market
  creation, a trade, signer attestation, resolution finalization, trace
  publication, and hash verification. Ran twice (once building the scripts
  interactively, once via the finished `scripts/demo.sh`) with consistent
  results.
- **The resolution-commitment design (ADR
0001)** was validated on live testnet, not just in unit tests: `treasury.agent_create_market` was
  called with a `resolution_hash` computed off-chain bv
  `scripts/spec-hash/`, and `resolver.register_signer_spec` accepted the
  matching revealed spec — the hash genuinely round-tripped through real
  XDR encoding on a real ledger.
- **The treasury cross-contract auth fix (ADR 0002, `authorize_token_pull`)**
  was validated three ways: (1) a unit test using targeted `mock_auths`
  (not `mock_all_auths`, which would hide the bug); (2) that same test was
  manually re-run with the fix commented out, confirmed to fail with
  `HostError: Error(Auth, InvalidAction)` / "Unauthorized function call",
  then confirmed to pass again with the fix restored; (3) the real,
  unmocked testnet transaction for `agent_create_market` succeeded, which
  would have failed with the same auth error had the fix been wrong.
- **All four** `packages/bindings/*` packages: generated, then `npm install
  && npm run build` completed with no errors for every one of them
  (`market-core`, `reasoning-registry`, `resolver`, `treasury`) — a full
  round-trip, not just generation. `resolver`'s and `treasury`'s installs
  were slower (each ~4 minutes against this sandbox's npm registry) and so
  were run later in the session than the other two, but all four are now
  confirmed.

## What's implemented but not fully round-trip verified

- `decimals()` is read once at construction and the derived bounds are
  exercised in unit tests at 0, 7 and 18 decimals, but no real-testnet
  deployment of a market with an 18-decimal collateral has been made. The
  deployed contracts and `deployments/testnet.json` predate this change.
- **x402's registry-backed `TraceRepository`**
  (`x402/registry-trace-repository.ts`): unit-tested with a
  dependency-injected fake reader (its real interface, verified against the
  actual generated `get_trace` signature). Attempting to run the real
  server against the live deployed `reasoning-registry` failed — but the
  failure is an environment issue, not a code issue: **Node's own outbound
  `fetch` to `soroban-testnet.stellar.org` times out at the TCP level in
  this sandbox** (`ETIMEDOUT` / `ENETREACH`, reproduced with a bare
  `node -e "fetch(...)"` with no OracleDesk code involved at all), while
  `curl` to the identical URL and the Rust `stellar` CLI itself both work
  fine throughout this same session. This looks like a sandbox network
  policy specific to Node's raw socket connections, not a bug here — but it
  means the actual RPC round-trip for this path is unverified in this
  environment. Whoever runs this outside this sandbox should confirm it
  directly.
- **Agents' live mode** (`agents/stellar/adapter.ts` with
  `mode: "live"`, i.e. `AGENT_MODE=live` for the CLIs): implemented,
  type-checked against the real bindings, and structurally exercised in
  dry-run mode end to end. Never run in live mode — that would need the
  same Node→testnet RPC path just noted as unverified, plus a funded
  agent secret key wired through `AGENT_SECRET_KEY`.
- **`quote_sell`/`collateralOutForShares` against a live contract quote`*:
  the property test in both `model/fpmm_model.py`` and
  `agents/core/fpm.test.ts` verifies the off-chain math against itself
  (forward `calc_sell` vs. the `collateralOutForShares` inverse), which is
  what the design brief asked for ("add a test against the contract
  quote") in spirit — but it was not additionally checked against a live
  `market-core.quote_sell` call on testnet with real reserves.

## Blocked / not attempted

- `decimals()` is a cross-contract call in the constructor, so the collateral
  token must already be deployed when the market-core constructor runs.
  `scripts/deploy-testnet.sh` already deploys the asset first, but no
  real-testnet deployment of an 18-decimal collateral has been made.
- **Reflector (SEP-40) price-mode resolution on live testnet**: `resolver`'s
  `PriceThreshold` mode is implemented and unit-tested (with a mock reflector),
  but no real testnet Reflector contract address was used
  anywhere — there was no way to look one up and verify it against
  Reflector's own current docs in this session (no browsing tool was
  used for this). The demo uses **signer mode** instead, which ne
