#!/usr/bin/env bash
# Idempotent testnet deploy for all four OracleDesk contracts.
#
# Safe to re-run: each step checks deployments/testnet.json first and skips
# anything already recorded there. Delete an entry (or the whole file) to
# force that piece to redeploy. Never touches mainnet — the network is
# hardcoded to `testnet` below, not read from the environment.
#
# Requires: stellar CLI, jq. Builds the four contracts first if their wasm
# isn't already built (`make build` does this, or run `stellar contract
# build` yourself first).
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

NETWORK=testnet
DEPLOYER_KEY=oracledesk-deployer
AGENT_KEY=oracledesk-agent
OUT=deployments/testnet.json
WASM_DIR=target/wasm32v1-none/release

# Collateral decimals. market-core reads the token's decimals() at deploy and
# derives its bounds (min_seed = 100 * 10^d, max_sets = 1e8 * 10^d), so the
# treasury risk caps below must be scaled to match. The self-issued test USDC
# is a Classic asset (7 decimals); set COLLATERAL_DECIMALS if you point
# `collateral` at a token with a different precision.
COLLATERAL_DECIMALS=${COLLATERAL_DECIMALS:-7}
if [ "$COLLATERAL_DECIMALS" -lt 0 ] || [ "$COLLATERAL_DECIMALS" -gt 18 ]; then
  echo "error: COLLATERAL_DECIMALS must be in 0..=18 (got $COLLATERAL_DECIMALS)" >&2
  exit 1
fi
UNIT=$(printf '1%0*d' "$COLLATERAL_DECIMALS" 0) # 10^d as a decimal string

# treasury's risk caps (raw collateral units). Must comfortably clear
# market-core's derived min_seed (100 tokens) since agent_create_market's seed
# amount is capped by max_trade. Override via env if you need different limits.
MAX_TRADE=${MAX_TRADE:-$((200 * UNIT))}   # 200 tokens
MAX_MARKET=${MAX_MARKET:-$((500 * UNIT))} # 500 tokens
MAX_DAILY=${MAX_DAILY:-$((1000 * UNIT))}  # 1,000 tokens
DEFAULT_FEE_BPS=${DEFAULT_FEE_BPS:-100} # 1%
# Portion of each fee kept by the protocol treasury (bps). The remainder
# accrues to the market creator, claimable before resolution. 10000 = the
# protocol keeps everything (pre-#8 behavior).
PROTOCOL_FEE_SHARE_BPS=${PROTOCOL_FEE_SHARE_BPS:-10000}

log() { echo "==> $*" >&2; }

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "error: '$1' is required but not installed" >&2
    exit 1
  }
}
require_cmd stellar
require_cmd jq

mkdir -p deployments
[ -f "$OUT" ] || echo '{}' >"$OUT"

json_get() { jq -r "$1 // empty" "$OUT"; }
json_set() {
  local tmp
  tmp=$(mktemp)
  jq "$@" "$OUT" >"$tmp" && mv "$tmp" "$OUT"
}

ensure_identity() {
  local name=$1
  if stellar keys address "$name" >/dev/null 2>&1; then
    log "identity '$name' already exists"
  else
    log "generating and funding identity '$name'"
    stellar keys generate "$name" --network "$NETWORK" --fund
  fi
}

# --- wasm must already be built -------------------------------------------
for f in market_core resolver reasoning_registry treasury; do
  if [ ! -f "$WASM_DIR/$f.wasm" ]; then
    log "missing $WASM_DIR/$f.wasm — building all contracts first"
    stellar contract build
    break
  fi
done

# contractimport! (resolver, treasury) reads a vendored copy of
# market-core's wasm at *their* compile time — keep it fresh before
# rebuilding them. See docs/adr/0002-cross-contract-calls.md.
cp "$WASM_DIR/market_core.wasm" contracts/resolver/market_core.wasm
cp "$WASM_DIR/market_core.wasm" contracts/treasury/market_core.wasm
stellar contract build

# --- identities -------------------------------------------------------------
ensure_identity "$DEPLOYER_KEY"
ensure_identity "$AGENT_KEY"
DEPLOYER=$(stellar keys address "$DEPLOYER_KEY")
AGENT=$(stellar keys address "$AGENT_KEY")
json_set --arg d "$DEPLOYER" --arg a "$AGENT" '.deployer = $d | .agent = $a'

# --- USDC test asset ---------------------------------------------------------
# Self-issued testnet "USDC": a real SEP-41 token (7 decimals by default,
# matching COLLATERAL_DECIMALS) with no dependency on guessing a real issuer's
# contract id. market-core reads decimals() from this token at construction.
# <!-- TODO(maintainer): point `collateral` at Circle's real testnet USDC
# issuer if/when this needs to interoperate with other testnet apps. -->
USDC=$(json_get .contracts.usdc)
if [ -z "$USDC" ]; then
  log "deploying self-issued testnet USDC (SEP-41)"
  USDC=$(stellar contract asset deploy \
    --asset "USDC:${DEPLOYER}" \
    --source-account "$DEPLOYER_KEY" \
    --network "$NETWORK")
  json_set --arg id "$USDC" '.contracts.usdc = $id'
else
  log "USDC already deployed: $USDC"
fi

# Verify the collateral's decimals match what this script assumes. market-core
# rejects decimals > 18 at construction, and the treasury caps above are sized
# from COLLATERAL_DECIMALS, so a mismatch would silently mis-scale every limit.
ACTUAL_DECIMALS=$(stellar contract invoke --id "$USDC" --source-account "$DEPLOYER_KEY" \
  --network "$NETWORK" -- decimals 2>/dev/null | tr -d '"[:space:]')
if [ -n "$ACTUAL_DECIMALS" ] && [ "$ACTUAL_DECIMALS" != "$COLLATERAL_DECIMALS" ]; then
  echo "error: collateral $USDC reports $ACTUAL_DECIMALS decimals, expected $COLLATERAL_DECIMALS" >&2
  echo "hint: re-run with COLLATERAL_DECIMALS=$ACTUAL_DECIMALS (and matching MAX_* caps)" >&2
  exit 1
fi

# --- market-core (placeholder resolver/treasury, wired up after) -----------
MARKET_CORE=$(json_get .contracts.market_core)
if [ -z "$MARKET_CORE" ]; then
  log "deploying market-core"
  MARKET_CORE=$(stellar contract deploy \
    --wasm "$WASM_DIR/market_core.wasm" \
    --source-account "$DEPLOYER_KEY" \
    --network "$NETWORK" \
    -- \
    --admin "$DEPLOYER" \
    --collateral "$USDC" \
    --decimals "$COLLATERAL_DECIMALS" \
    --treasury "$DEPLOYER" \
    --resolver "$DEPLOYER" \
    --default_fee_bps "$DEFAULT_FEE_BPS" \
    --protocol_fee_share_bps "$PROTOCOL_FEE_SHARE_BPS")
  json_set --arg id "$MARKET_CORE" '.contracts.market_core = $id'
else
  log "market-core already deployed: $MARKET_CORE"
  log "note: existing market-core was constructed with its own decimals(); delete .contracts.market_core to redeploy"
fi

# --- resolver ----------------------------------------------------------------
RESOLVER=$(json_get .contracts.resolver)
if [ -z "$RESOLVER" ]; then
  log "deploying resolver"
  RESOLVER=$(stellar contract deploy \
    --wasm "$WASM_DIR/resolver.wasm" \
    --source-account "$DEPLOYER_KEY" \
    --network "$NETWORK" \
    -- \
    --admin "$DEPLOYER" \
    --market_core "$MARKET_CORE")
  json_set --arg id "$RESOLVER" '.contracts.resolver = $id'
else
  log "resolver already deployed: $RESOLVER"
fi

# --- treasury ------------------------------------------------------------
TREASURY=$(json_get .contracts.treasury)
if [ -z "$TREASURY" ]; then
  log "deploying treasury"
  TREASURY=$(stellar contract deploy \
    --wasm "$WASM_DIR/treasury.wasm" \
    --source-account "$DEPLOYER_KEY" \
    --network "$NETWORK" \
    -- \
    --admin "$DEPLOYER" \
    --agent "$AGENT" \
    --collateral "$USDC" \
    --market_core "$MARKET_CORE" \
    --max_trade "$MAX_TRADE" \
    --max_market "$MAX_MARKET" \
    --max_daily "$MAX_DAILY")
  json_set --arg id "$TREASURY" '.contracts.treasury = $id'
  json_set '.needs_treasury_wiring = true'
else
  log "treasury already deployed: $TREASURY"
fi

# --- reasoning-registry ------------------------------------------------------
REGISTRY=$(json_get .contracts.reasoning_registry)
if [ -z "$REGISTRY" ]; then
  log "deploying reasoning-registry"
  REGISTRY=$(stellar contract deploy \
    --wasm "$WASM_DIR/reasoning_registry.wasm" \
    --source-account "$DEPLOYER_KEY" \
    --network "$NETWORK" \
    -- \
    --admin "$DEPLOYER")
  json_set --arg id "$REGISTRY" '.contracts.reasoning_registry = $id'
else
  log "reasoning-registry already deployed: $REGISTRY"
fi

# --- wire market-core to the real resolver/treasury -------------------------
if [ "$(json_get .needs_treasury_wiring)" = "true" ] || [ "$(json_get .wired)" != "true" ]; then
  log "wiring market-core.set_resolver / set_treasury"
  stellar contract invoke --id "$MARKET_CORE" --source-account "$DEPLOYER_KEY" \
    --network "$NETWORK" -- set_resolver --resolver "$RESOLVER"
  stellar contract invoke --id "$MARKET_CORE" --source-account "$DEPLOYER_KEY" \
    --network "$NETWORK" -- set_treasury --treasury "$TREASURY"
  json_set 'del(.needs_treasury_wiring) | .wired = true'
else
  log "market-core already wired to resolver/treasury"
fi

# --- register the agent with reasoning-registry ------------------------------
if [ "$(json_get .agent_registered)" != "true" ]; then
  log "registering agent with reasoning-registry"
  stellar contract invoke --id "$REGISTRY" --source-account "$DEPLOYER_KEY" \
    --network "$NETWORK" -- register_agent --agent "$AGENT"
  json_set '.agent_registered = true'
else
  log "agent already registered"
fi

# --- fund the treasury with test USDC ---------------------------------------
TREASURY_FUNDED=$(json_get .treasury_funded_amount)
FUND_AMOUNT=${TREASURY_FUND_AMOUNT:-$((1000000 * UNIT))} # 1,000,000 tokens
if [ "$TREASURY_FUNDED" != "$FUND_AMOUNT" ]; then
  log "minting $FUND_AMOUNT stroops of test USDC to treasury"
  stellar contract invoke --id "$USDC" --source-account "$DEPLOYER_KEY" \
    --network "$NETWORK" -- mint --to "$TREASURY" --amount "$FUND_AMOUNT"
  json_set --arg a "$FUND_AMOUNT" '.treasury_funded_amount = $a'
else
  log "treasury already funded with $FUND_AMOUNT stroops"
fi

json_set --arg ts "$(date -u +%Y-%m-%dT%H:%M:%SZ)" --argjson d "$COLLATERAL_DECIMALS" \
  '.last_deployed_at = $ts | .collateral_decimals = $d'

log "done. Deployment record: $OUT"
cat "$OUT"
