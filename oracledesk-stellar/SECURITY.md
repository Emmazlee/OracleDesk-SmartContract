# Security Policy

## Status: testnet-only, unaudited

Everything in this repository is deployed to Stellar **testnet only**. The
contracts have **not** been professionally audited. Do not deploy them to
mainnet, and do not send anything of real value to any address or contract
in this repo. See `docs/STATUS.md` for the current list of what has and has
not been verified.

## Reporting a vulnerability

Please report security issues privately, using
[GitHub Security Advisories](../../security/advisories/new) for this
repository, rather than opening a public issue. This applies to:

- A way to drain, lock, or misdirect funds held by any of the four
  contracts (`market-core`, `resolver`, `treasury`, `reasoning-registry`).
- An authorization bypass (a privileged function callable by the wrong
  party — see `contracts/*/src/lib.rs`'s wrong-caller tests for what's
  already covered).
- A way to make the resolution-commitment scheme (ADR 0001) or the treasury
  cross-contract auth flow (ADR 0002) behave differently from documented.
- A vulnerability in the x402 payment/verification flow that would let a
  trace be served without payment, or without its hash matching the
  on-chain record.

<!-- TODO(maintainer): confirm an expected response/triage timeline before
publishing this repo — do not commit to one without checking. -->

## Design decisions

- **`merge` is callable while the contract is paused.** `market-core`'s
  `split` entrypoint is gated by `assert_not_paused` (like `buy` / `sell`),
  but `merge` is intentionally not. Merging burns one YES plus one NO share
  and returns collateral at par; it never touches the FPMM pool's
  `reserve_yes` / `reserve_no` or `fees_accrued`, and it cannot move price or
  extract value from the pool. Allowing it while paused lets holders de-risk
  during an incident or a slow resolution without exposing the pool to new
  risk. A paused contract can therefore still pay out collateral via `merge`
  (and via `redeem`, which was already never paused). Security reviewers
  should treat this as an accepted, documented trade-off: pause is a
  mitigation for pool-moving operations, not a full freeze on exits.

- **`MAX_SETS` griefing via `split`.** An attacker can `split` up to
  `math::MAX_SETS` (1e15) and thereby push `sets_minted` to the cap, causing
  subsequent `buy` calls to fail with `Error::MarketTooLarge` (#15). This
  costs the attacker locked capital and is risk-free to unwind via `merge`,
  so it is a griefing vector, not a theft vector. The decision and its
  rationale are recorded in `docs/architecture.md`.

## Scope

In scope: `contracts/`, `x402/`, `agents/`, `packages/bindings/` (the
generation script and its templates, not third-party generated code
itself), and the deployment scripts in `scripts/`.

Out of scope: the separate frontend repository, and any third-party
dependency's own vulnerabilities (report those upstream — `npm audit` /
`cargo audit` findings in a dependency are not this repo's issues unless
this repo's usage of them is what's exploitable).
