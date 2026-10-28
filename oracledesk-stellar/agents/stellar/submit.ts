import { rpc, Transaction } from "@stellar/stellar-sdk";
import { createJournalRecord, type TransactionJournal } from "./journal.js";
import { classifySorobanError, SorobanPendingError, SorobanSubmissionError } from "./errors.js";

export interface SorobanSubmitOptions {
  id: string;
  transaction: Transaction;
  server: Pick<rpc.Server, "sendTransaction" | "getTransaction">;
  journal: TransactionJournal;
  /** Maximum number of broadcasts of this exact signed envelope. */
  maxAttempts?: number;
  /** Number of status polls after an accepted broadcast. */
  maxPollsPerAttempt?: number;
  retryDelayMs?: number;
  sleep?: (milliseconds: number) => Promise<void>;
}

export type SorobanSubmitResult = Extract<Awaited<ReturnType<rpc.Server["getTransaction"]>>, { status: "SUCCESS" }>;

const defaultSleep = (milliseconds: number): Promise<void> =>
  new Promise((resolve) => setTimeout(resolve, milliseconds));

function errorMessage(value: unknown): string {
  if (value instanceof Error) return value.message;
  return typeof value === "string" ? value : JSON.stringify(value);
}

function makeError(value: unknown, hash: string): SorobanSubmissionError {
  const classification = classifySorobanError(value);
  return new SorobanSubmissionError(classification.message, { ...classification, terminal: true }, {
    cause: value,
    transactionHash: hash,
  });
}

/**
 * Broadcasts a transaction that is already signed. Every retry serializes the
 * same immutable envelope; this function never rebuilds or re-signs it.
 */
export async function submitSorobanTransaction(options: SorobanSubmitOptions): Promise<SorobanSubmitResult> {
  const {
    id,
    transaction,
    server,
    journal,
    maxAttempts = 4,
    maxPollsPerAttempt = 4,
    retryDelayMs = 250,
    sleep = defaultSleep,
  } = options;
  if (!id) throw new TypeError("transaction journal id is required");
  if (!Number.isInteger(maxAttempts) || maxAttempts < 1) throw new RangeError("maxAttempts must be a positive integer");
  if (!Number.isInteger(maxPollsPerAttempt) || maxPollsPerAttempt < 0) {
    throw new RangeError("maxPollsPerAttempt must be a non-negative integer");
  }

  const envelopeXdr = transaction.toXDR();
  const hash = transaction.hash().toString("hex");
  const log = (stage: Parameters<TransactionJournal["append"]>[0]["stage"], attempt?: number, detail?: string) =>
    journal.append(createJournalRecord({ id, stage, hash, envelopeXdr, attempt, detail }));

  await log("signed");
  let lastError: unknown;

  for (let attempt = 1; attempt <= maxAttempts; attempt += 1) {
    await log("submitting", attempt);
    let sendResponse: Awaited<ReturnType<typeof server.sendTransaction>> | undefined;
    try {
      // A fresh SDK wrapper is not constructed here: the signed Transaction
      // supplied by the caller is reused for every broadcast attempt.
      sendResponse = await server.sendTransaction(transaction);
    } catch (error) {
      lastError = error;
      const final = await lookupFinalized(server, hash);
      if (final?.status === "SUCCESS") {
        await log("confirmed", attempt);
        return final;
      }
      if (final?.status === "FAILED") {
        await log("failed", attempt, "transaction finalized with FAILED status");
        throw makeError(final, hash);
      }
      if (attempt < maxAttempts) {
        await sleep(retryDelayMs);
        continue;
      }
      break;
    }

    if (sendResponse.status === "ERROR") {
      lastError = sendResponse.errorResult ?? sendResponse;
      const classification = classifySorobanError(lastError);
      await log("failed", attempt, classification.message);
      throw new SorobanSubmissionError(`Soroban RPC rejected transaction ${hash}: ${classification.message}`, classification, {
        cause: lastError,
        transactionHash: hash,
      });
    }

    if (sendResponse.status === "PENDING" || sendResponse.status === "DUPLICATE") {
      await log("submitted", attempt, sendResponse.status);
      for (let poll = 0; poll < maxPollsPerAttempt; poll += 1) {
        if (poll > 0 || retryDelayMs > 0) await sleep(retryDelayMs);
        try {
          const result = await server.getTransaction(hash);
          if (result.status === "SUCCESS") {
            await log("confirmed", attempt);
            return result;
          }
          if (result.status === "FAILED") {
            await log("failed", attempt, "transaction finalized with FAILED status");
            throw makeError(result, hash);
          }
        } catch (error) {
          if (error instanceof SorobanSubmissionError) throw error;
          lastError = error;
        }
      }
    } else {
      // TRY_AGAIN_LATER means this send was not accepted. Repeating the same
      // signed envelope is safe and preserves its transaction hash.
      lastError = new Error(`Soroban RPC returned ${sendResponse.status}`);
    }
    if (attempt < maxAttempts) await sleep(retryDelayMs);
  }

  await log("uncertain", maxAttempts, lastError === undefined ? undefined : errorMessage(lastError));
  throw new SorobanPendingError(
    `Transaction ${hash} was not confirmed after ${maxAttempts} bounded submission attempts; its signed envelope may still land. Resume by resending the same envelope or checking this hash.`,
    hash,
    { cause: lastError },
  );
}

/**
 * Resumes a journaled envelope after restart. It restores the exact signed
 * XDR, rather than rebuilding an equivalent transaction with a new sequence or
 * hash.
 */
export function transactionFromJournal(
  envelopeXdr: string,
  networkPassphrase: string,
): Transaction {
  return new Transaction(envelopeXdr, networkPassphrase);
}

async function lookupFinalized(
  server: Pick<rpc.Server, "getTransaction">,
  hash: string,
): Promise<Awaited<ReturnType<rpc.Server["getTransaction"]>> | undefined> {
  try {
    const result = await server.getTransaction(hash);
    return result.status === "NOT_FOUND" ? undefined : result;
  } catch {
    return undefined;
  }
}
