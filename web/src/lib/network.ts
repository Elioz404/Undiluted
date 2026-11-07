/**
 * What the browser reads from NEAR itself rather than through the relay: the
 * published heads, and the enclave key each company registered. The site also
 * pins the enclave's signing key, so a relay cannot put another enclave, or
 * itself, in the middle. All three settings come from the build.
 */

import { base58ToHex } from "./base58.ts";

export interface NetworkSettings {
  rpcUrl?: string;
  anchorContract?: string;
  enclaveSigningKey?: string;
  enclaveWasmHash?: string;
  registerContract?: string;
}

/** From the build; scripts outside Vite set them with `useNetwork`. */
let settings: NetworkSettings = {
  rpcUrl: import.meta.env?.VITE_NEAR_RPC_URL,
  anchorContract: import.meta.env?.VITE_ANCHOR_CONTRACT,
  enclaveSigningKey: import.meta.env?.VITE_ENCLAVE_SIGNING_KEY,
  enclaveWasmHash: import.meta.env?.VITE_ENCLAVE_WASM_SHA256,
  registerContract: import.meta.env?.VITE_OUTLAYER_REGISTER_CONTRACT,
};

export function useNetwork(network: NetworkSettings): void {
  settings = network;
}

function setting(name: string, value: string | undefined): string {
  if (!value?.trim()) {
    throw new Error(`${name} is not set for this build; see web/.env.example`);
  }
  return value.trim();
}

const rpcUrl = () => setting("VITE_NEAR_RPC_URL", settings.rpcUrl);
const anchorContract = () => setting("VITE_ANCHOR_CONTRACT", settings.anchorContract);

/** The enclave key this site trusts, as hex. */
export const trustedEnclaveKey = () =>
  setting("VITE_ENCLAVE_SIGNING_KEY", settings.enclaveSigningKey).toLowerCase();

/** The sha256 of the enclave wasm this site was built against, as hex. */
export const trustedEnclaveWasm = () =>
  setting("VITE_ENCLAVE_WASM_SHA256", settings.enclaveWasmHash).toLowerCase();

const registerContract = () => setting("VITE_OUTLAYER_REGISTER_CONTRACT", settings.registerContract);

async function rpc<T>(method: string, params: unknown): Promise<T> {
  const response = await fetch(rpcUrl(), {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: "undiluted", method, params }),
  });
  const body = await response.json();
  if (!response.ok || body.error || body.result?.error) {
    throw new Error("NEAR did not answer; try again");
  }
  return body.result as T;
}

async function view<T>(contract: string, method: string, args: unknown): Promise<T> {
  const result = await rpc<{ result: number[] }>("query", {
    request_type: "call_function",
    finality: "final",
    account_id: contract,
    method_name: method,
    args_base64: btoa(JSON.stringify(args)),
  });
  return JSON.parse(new TextDecoder().decode(new Uint8Array(result.result))) as T;
}

/** The head the anchor contract holds for `sequence`, as hex, or null. */
export async function publishedHead(company: string, sequence: number): Promise<string | null> {
  const head = await view<string | null>(anchorContract(), "head_at", { company_id: company, sequence });
  return head ? base58ToHex(head) : null;
}

/** What each receipt of a final transaction returned, base64. */
export async function transactionOutputs(transaction: string, sender: string): Promise<string[]> {
  const result = await rpc<{ receipts_outcome: { outcome: { status: { SuccessValue?: string } } }[] }>(
    "EXPERIMENTAL_tx_status",
    { tx_hash: transaction, sender_account_id: sender, wait_until: "FINAL" },
  );
  return result.receipts_outcome
    .map((receipt) => receipt.outcome.status.SuccessValue)
    .filter((value): value is string => Boolean(value));
}

/** Intel's collateral and the approved worker measurements, as OutLayer keeps them. */
export async function outlayerTrustAnchors(): Promise<{ approved: unknown[]; collaterals: string[] }> {
  const [approved, collaterals] = await Promise.all([
    view<unknown[]>(registerContract(), "get_approved_measurements", {}),
    view<string[]>(registerContract(), "get_collaterals", {}),
  ]);
  return { approved, collaterals };
}

/** The enclave key `company` registered on chain, as hex, or null. */
export async function registeredEnclaveKey(company: string): Promise<string | null> {
  const registered = await view<{ enclave_key: string } | null>(anchorContract(), "company", { company_id: company });
  if (!registered) {
    return null;
  }
  return base58ToHex(registered.enclave_key.replace(/^ed25519:/, ""));
}
