# Security Policy

## Status: testnet-only, unaudited

Everything in this repository is deployed to Stellar **testnet only**. The
contracts have **not** been professionally audited. Do not deploy them to
mainnet, and do not send anything of real value to any address or contract
in this repo. See `docs/STATUS.md` for the current list of what has and
has not been verified.

## Reporting a vulnerability

Please report security issues privately, using
[GitHub Security Advisories](../../security/advisories/new) for this
repository, rather than opening a public issue. This applies to:

- A way to drain, lock, or misdirect funds eld by any of the four
  contracts (`market-core`, `resolver`, `treasury`, `reasoning-registry`).
-  An authorization bypass (a privileged function callable by the wrong
  party — see `contracts/*/src/lib.r`'s wrong-caller tests for what's
  already covered).
-  A way to make the resolution-commitment scheme (ADR"0001) or the treasury
  cross-contract auth flow (ADR"0002) behave differently from documented.
-  A vulnerability in the x402 payment/verification flow that would let a
  trace be served without payment, or without its hash matching the
  on-chain record.

<!-- TODO(maintainer): confirm an expected response/triage timeline before
publishing this repo — do not commit to one without checking. -->

## Scope

In scope: `contracts/`, `x402/`, `agents/`, `packages/bindings/` (the
generation script and its templates, not third-party generated code
itself), and the deployment scripts in `scripts/`.

Out of scope: the separate frontend repository, and any third-party
dependency's own vulnerabilities (report those upstream — `npm audit` /
cargo audit` findings in a dependency are not this repo's issues unless
this repo's usage of them is what's exploitable.

## Recorded decisions

### Merge is allowed while the contract is paused

`market-core` has a pause switch that gates `create_market`, `buy`, `sell` and
`split`. The new `merge` entrypoint is **not** gated by the pause switch. This
is a deliberate decision and is recorded here for the security reviewer.

Rationale:

-  merge` is a pure exit. It burns one YES share and one NO share from the
  caller's own position and returns one unit of collateral at par. It does
  not touch `reserve_yes`, `reserve_no` or `fees_accrued`, does not move the
  pool price, and charges no fee. It cannot extract more collateral than the
  caller already locked via `split` or bought through the pool.
-  Allowing merge while paused lets holders de-risk while the resolver is
  slow or the contract is being investigated. Blocking the exit would trap
  collateral without adding any safety benefit.
-  The only state `merge` mutates is the caller's own `Position` and the
  market's `sets_minted` counter, both of which are covered by the
  invariants documented in `docs/architecture.md`.

The asymmetry between `split` (gated by pause and by `close_time`) and `merge`
(gated only by market status) is documented in the `split` / `merge` doc
comments in `contracts/market-core/src/lib.rs` and in `docs/architecture.md`.
