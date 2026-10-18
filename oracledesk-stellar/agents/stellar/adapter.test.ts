import assert from "node:assert/strict";
import test from "node:test";
import { StellarAdapter, type Deployments } from "./adapter.js";

const deployments: Deployments = {
  deployer: "GDEPLOYER",
  agent: "GAGENT",
  contracts: {
    usdc: "CUSDC",
    market_core: "CMARKETCORE",
    resolver: "CRESOLVER",
    treasury: "CTREASURY",
    reasoning_registry: "CREGISTRY",
  },
};

test("dry-run mode never requires an agent secret", () => {
  assert.doesNotThrow(() => {
    new StellarAdapter({
      mode: "dry-run",
      networkPassphrase: "Test SDF Network ; September 2015",
      rpcUrl: "https://soroban-testnet.stellar.org",
      deployments,
    });
  });
});

test("live mode without an agent secret throws immediately", () => {
  assert.throws(() => {
    new StellarAdapter({
      mode: "live",
      networkPassphrase: "Test SDF Network ; September 2015",
      rpcUrl: "https://soroban-testnet.stellar.org",
      deployments,
    });
  }, /agentSecret/);
});

test("adapter rejects unknown mode", () => {
  assert.throws(() => {
    new StellarAdapter({
      mode: "yolo" as unknown as "dry-run",
      networkPassphrase: "Test SDF Network ; September 2015",
      rpcUrl: "https://soroban-testnet.stellar.org",
      deployments,
    });
  }, /unknown mode/);
});

test("dry-run agentBuy describes the call without touching the network", async () => {
  const adapter = new StellarAdapter({
    mode: "dry-run",
    networkPassphrase: "Test SDF Network ; September 2015",
    rpcUrl: "https://soroban-testnet.stellar.org",
    deployments,
  });
  const result = await adapter.agentBuy({
    market_id: 0n,
    outcome: { tag: "Yes", values: undefined },
    collateral_in: 10_000_000n,
    min_shares_out: 0n,
  });
  assert.equal(result.dryRun, true);
  if (result.dryRun) {
    assert.equal(result.contractId, "CTREASURY");
    assert.equal(result.method, "agent_buy");
  }
});

test("dry-run agentBuyV2 describes the bounded call without touching the network", async () => {
  const adapter = new StellarAdapter({
    mode: "dry-run",
    networkPassphrase: "Test SDF Network ; September 2015",
    rpcUrl: "https://soroban-testnet.stellar.org",
    deployments,
  });
  const result = await adapter.agentBuyV2({
    market_id: 0n,
    outcome: { tag: "Yes", values: undefined },
    collateral_in: 10_000_000n,
    min_shares_out: 9_500_000n,
    limit_price_bps: 6000,
    deadline: 1_000_060n,
  });
  assert.equal(result.dryRun, true);
  if (result.dryRun) {
    assert.equal(result.contractId, "CTREASURY");
    assert.equal(result.method, "agent_buy_v2");
  }
});

test("dry-run agentSellV2 describes the bounded call without touching the network", async () => {
  const adapter = new StellarAdapter({
    mode: "dry-run",
    networkPassphrase: "Test SDF Network ; September 2015",
    rpcUrl: "https://soroban-testnet.stellar.org",
    deployments,
  });
  const result = await adapter.agentSellV2({
    market_id: 0n,
    outcome: { tag: "No", values: undefined },
    collateral_out: 5_000_000n,
    max_shares_in: 6_000_000n,
    limit_price_bps: 4000,
    deadline: 1_000_060n,
  });
  assert.equal(result.dryRun, true);
  if (result.dryRun) {
    assert.equal(result.contractId, "CTREASURY");
    assert.equal(result.method, "agent_sell_v2");
  }
});
