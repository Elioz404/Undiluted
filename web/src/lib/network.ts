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
}

/** From the build; scripts outside Vite set them with `useNetwork`. */
let settings: NetworkSettings = {
  rpcUrl: import.meta.env?.VITE_NEAR_RPC_URL,
  anchorContract: import.meta.env?.VITE_ANCHOR_CONTRACT,
  enclaveSigningKey: import.meta.env?.VITE_ENCLAVE_SIGNING_KEY,
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

async function view<T>(method: string, args: unknown): Promise<T> {
  const response = await fetch(rpcUrl(), {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      jsonrpc: "2.0",
      id: "undiluted",
      method: "query",
      params: {
        request_type: "call_function",
        finality: "final",
        account_id: anchorContract(),
        method_name: method,
        args_base64: btoa(JSON.stringify(args)),
      },
    }),
  });
  const body = await response.json();
  if (!response.ok || body.error || body.result?.error) {
    throw new Error("NEAR did not answer; try again");
  }
  return JSON.parse(new TextDecoder().decode(new Uint8Array(body.result.result))) as T;
}

/** The head the anchor contract holds for `sequence`, as hex, or null. */
export async function publishedHead(company: string, sequence: number): Promise<string | null> {
  const head = await view<string | null>("head_at", { company_id: company, sequence });
  return head ? base58ToHex(head) : null;
}

/** The enclave key `company` registered on chain, as hex, or null. */
export async function registeredEnclaveKey(company: string): Promise<string | null> {
  const registered = await view<{ enclave_key: string } | null>("company", { company_id: company });
  if (!registered) {
    return null;
  }
  return base58ToHex(registered.enclave_key.replace(/^ed25519:/, ""));
}
