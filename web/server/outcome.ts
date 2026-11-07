/**
 * Reading a transaction's result. Kept free of any network code so the rules
 * that decide "succeeded", "refused" and "don't know yet" can be tested.
 */

/** What our contracts log just before refusing a call. */
const REFUSAL_MARKER = "undiluted refused: ";

export interface Outcome {
  hash: string;
  succeeded: boolean;
  /** The transaction's return value, decoded from base64, if it had one. */
  value: string | null;
  /** The reason a contract gave for refusing, if it did. */
  refusal: string | null;
  logs: string[];
}

interface Receipt {
  outcome?: { logs?: string[]; status?: Record<string, unknown> };
}

interface TransactionResponse {
  status?: Record<string, unknown>;
  receipts_outcome?: Receipt[];
}

function decodeBase64(value: string): string {
  return Buffer.from(value, "base64").toString("utf8");
}

export function readOutcome(hash: string, response: unknown): Outcome {
  const transaction = response as TransactionResponse;
  const status = transaction.status ?? {};
  const logs = (transaction.receipts_outcome ?? []).flatMap((receipt) => receipt.outcome?.logs ?? []);
  const refusal = logs
    .map((log) => log.split(REFUSAL_MARKER)[1])
    .find((reason): reason is string => reason !== undefined) ?? null;
  const succeeded = "SuccessValue" in status && refusal === null;
  const encoded = typeof status.SuccessValue === "string" ? status.SuccessValue : "";
  return {
    hash,
    succeeded,
    value: encoded ? decodeBase64(encoded) : null,
    refusal: refusal?.trim() ?? null,
    logs,
  };
}

/** Whether an RPC error only means the answer took too long, not that it failed. */
export function isTimeout(error: unknown): boolean {
  const text = error instanceof Error ? `${error.name} ${error.message}` : String(error);
  return /timeout|timed out|TIMEOUT_ERROR|NOT_OBSERVED|UNKNOWN_TRANSACTION|fetch failed/i.test(text);
}
