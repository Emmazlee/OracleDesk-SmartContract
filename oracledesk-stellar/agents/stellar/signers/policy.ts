import { createHash } from "node:crypto";
import { Address, Operation, Transaction, TransactionBuilder, xdr } from "@stellar/stellar-sdk";
import type { SignerPolicy } from "./types.js";

const IDENTIFIER = /^[A-Za-z_][A-Za-z0-9_]*$/;
const HASH = /^[0-9a-f]{64}$/;

export function hashInvocationArgs(args: readonly xdr.ScVal[]): string {
  const length = Buffer.alloc(4);
  length.writeUInt32BE(args.length);
  const serialized = args.map((arg) => arg.toXDR());
  return createHash("sha256").update(length).update(Buffer.concat(serialized)).digest("hex");
}

export interface ValidatedInvocation {
  transaction: Transaction;
  transactionHash: string;
  contractId: string;
  functionName: string;
  argsHash: string;
}

export function validateTransaction(
  transactionXdr: string,
  networkPassphrase: string,
  policy: SignerPolicy,
): ValidatedInvocation {
  if (networkPassphrase !== policy.networkPassphrase) {
    throw new Error("network passphrase does not match signer policy");
  }

  let parsed: ReturnType<typeof TransactionBuilder.fromXDR>;
  try {
    parsed = TransactionBuilder.fromXDR(transactionXdr, networkPassphrase);
  } catch {
    throw new Error("transaction XDR is invalid or unsupported");
  }
  if (!(parsed instanceof Transaction)) {
    throw new Error("fee-bump and non-classic transaction envelopes are unsupported");
  }

  const tx = parsed;
  if (!policy.allowedSourceAddresses.includes(tx.source)) {
    throw new Error("transaction source is not allowed by signer policy");
  }
  if (tx.operations.length !== 1) {
    throw new Error("only single-operation transactions are supported");
  }

  const operation = tx.operations[0];
  if (operation.type !== "invokeHostFunction") {
    throw new Error("only Soroban contract invocation transactions are supported");
  }
  if (operation.source && operation.source !== tx.source) {
    throw new Error("operation-level source overrides are unsupported");
  }
  if (operation.auth && operation.auth.length > 0) {
    throw new Error("Soroban authorization entries are unsupported");
  }

  const hostFunction = operation.func;
  if (hostFunction.switch() !== xdr.HostFunctionType.hostFunctionTypeInvokeContract()) {
    throw new Error("contract creation, WASM upload, and other host functions are unsupported");
  }

  const invoke = hostFunction.invokeContract();
  const contractId = Address.fromScVal(xdr.ScVal.scvAddress(invoke.contractAddress())).toString();
  const rawFunctionName = invoke.functionName();
  const functionName = Buffer.isBuffer(rawFunctionName) ? rawFunctionName.toString("utf8") : rawFunctionName;
  if (
    typeof functionName !== "string" ||
    !IDENTIFIER.test(functionName) ||
    (Buffer.isBuffer(rawFunctionName) && !Buffer.from(functionName, "utf8").equals(rawFunctionName))
  ) {
    throw new Error("contract function name has an unsupported shape");
  }
  const argsHash = hashInvocationArgs(invoke.args());
  const invocationAllowed = policy.allowedInvocations.some(
    (entry) =>
      entry.contractId === contractId &&
      entry.functionName === functionName &&
      HASH.test(entry.argsHash) &&
      entry.argsHash === argsHash,
  );
  if (!invocationAllowed) {
    throw new Error("contract invocation is not explicitly allowed by signer policy");
  }

  return {
    transaction: tx,
    transactionHash: tx.hash().toString("hex"),
    contractId,
    functionName,
    argsHash,
  };
}

export function assertSigningRequest(
  request: { networkPassphrase?: string; address?: string; submit?: boolean; submitUrl?: string },
  signerAddress: string,
  policy: SignerPolicy,
): void {
  if (request.networkPassphrase && request.networkPassphrase !== policy.networkPassphrase) {
    throw new Error("requested network passphrase does not match signer policy");
  }
  if (request.address && request.address !== signerAddress) {
    throw new Error("requested signer address does not match signer identity");
  }
  if (request.submit === true || request.submitUrl !== undefined) {
    throw new Error("signer callbacks must not submit transactions");
  }
}
