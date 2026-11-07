/** Calls to the relay. It forwards ciphertext and relays what the contracts verify. */

import type { Anchor, EnclaveOutput, Instruction, RelayConfig, Settlement } from "./types.ts";

export class RelayError extends Error {
  constructor(
    message: string,
    readonly refused: boolean,
  ) {
    super(message);
  }
}

/** Where the relay is. Empty means the same origin as the page. */
let base = "";

export function useRelayAt(url: string): void {
  base = url.replace(/\/$/, "");
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${base}/api${path}`, {
    ...init,
    headers: { "content-type": "application/json", ...init?.headers },
  });
  const body = await response.json().catch(() => ({}));
  if (!response.ok) {
    throw new RelayError(body.error ?? `the relay answered ${response.status}`, body.refused === true);
  }
  return body as T;
}

const post = <T>(path: string, body: unknown) =>
  request<T>(path, { method: "POST", body: JSON.stringify(body) });

export const relay = {
  config: () => request<RelayConfig>("/config"),
  identity: () => request<EnclaveOutput>("/identity"),
  attestation: () => request<{ transaction: string; sender: string; attestation: unknown }>("/attestation"),
  enclave: (envelope: string) => post<EnclaveOutput>("/enclave", { envelope }),
  registerCompany: (company: string) => post("/companies", { company }),
  publishAnchor: (anchor: Anchor) => post("/anchors", anchor),
  pay: (micros: string) => post<Settlement>("/payments", { micros }),
  paySubscription: (micros: string, memo: string) => post<Settlement>("/payments/subscription", { micros, memo }),
  openPayouts: (company: string, totalMicros: string) => post("/payouts/open", { company, totalMicros }),
  payOut: (company: string, instruction: Instruction) => post("/payouts/pay", { company, instruction }),
};
