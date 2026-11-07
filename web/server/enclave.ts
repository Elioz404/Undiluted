/**
 * Runs the enclave on OutLayer through an on-chain request. The relay only
 * passes envelopes through: it cannot read them, and it cannot forge a reply,
 * because the browser checks the enclave's signature on every one.
 */

import { isHex } from "./encoding.ts";
import type { Near } from "./near.ts";

const REQUEST_GAS = 300_000_000_000_000n;
const RESOURCE_LIMITS = {
  max_instructions: 1_000_000_000,
  max_memory_mb: 128,
  max_execution_seconds: 10,
};

export interface EnclaveSettings {
  contract: string;
  project: string;
  secretsAccount: string;
  secretsProfile: string;
  /** OutLayer's API, which publishes an attestation for every run. */
  attestationApi: string;
}

/** Where to find the attested run that produced the enclave's identity. */
export interface IdentityRun {
  transaction: string;
  sender: string;
  attestation: unknown;
}

const ATTESTATION_ATTEMPTS = 10;
const ATTESTATION_WAIT_MS = 3000;

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

export class Enclave {
  private identity: Promise<{ output: unknown; transaction: string }> | null = null;

  constructor(
    private readonly near: Near,
    private readonly settings: EnclaveSettings,
  ) {}

  /**
   * The enclave's public keys never change, so they are asked for once. Callers
   * arriving together share the one run; a failed run is forgotten.
   */
  async getIdentity(): Promise<unknown> {
    return (await this.identityRun()).output;
  }

  /**
   * The attestation of the run that produced the identity. The relay only
   * fetches it; the browser checks it against Intel and NEAR itself.
   */
  async identityAttestation(): Promise<IdentityRun> {
    const { transaction } = await this.identityRun();
    const url = `${this.settings.attestationApi.replace(/\/$/, "")}/attestations/by-tx/${transaction}`;
    for (let attempt = 1; ; attempt += 1) {
      const response = await fetch(url);
      if (response.ok) {
        return { transaction, sender: this.near.accountId, attestation: await response.json() };
      }
      // OutLayer publishes the attestation a moment after the run settles.
      if (response.status !== 404 || attempt >= ATTESTATION_ATTEMPTS) {
        throw new Error(`no attestation for transaction ${transaction} (${response.status})`);
      }
      await sleep(ATTESTATION_WAIT_MS);
    }
  }

  private identityRun(): Promise<{ output: unknown; transaction: string }> {
    this.identity ??= this.run({ operation: "identity" }).catch((error: unknown) => {
      this.identity = null;
      throw error;
    });
    return this.identity;
  }

  /** The key the enclave signs with, as hex. */
  async signingKey(): Promise<string> {
    const identity = (await this.getIdentity()) as { signing_key?: unknown };
    if (!isHex(identity.signing_key, 32)) {
      throw new Error("the enclave did not return its signing key");
    }
    return identity.signing_key;
  }

  /** Forwards one encrypted request and returns the enclave's output as it is. */
  async request(envelope: string): Promise<unknown> {
    return (await this.run({ operation: "request", envelope })).output;
  }

  private async run(input: unknown): Promise<{ output: unknown; transaction: string }> {
    const inputData = JSON.stringify(input);
    const cost = await this.near.view<string>(this.settings.contract, "estimate_execution_cost", {
      resource_limits: RESOURCE_LIMITS,
      input_len: inputData.length,
    });
    const outcome = await this.near.call({
      contract: this.settings.contract,
      method: "request_execution",
      args: {
        source: { Project: { project_id: this.settings.project, version_key: null } },
        resource_limits: RESOURCE_LIMITS,
        input_data: inputData,
        secrets_ref: { profile: this.settings.secretsProfile, account_id: this.settings.secretsAccount },
        response_format: "Json",
        payer_account_id: null,
        params: null,
      },
      gas: REQUEST_GAS,
      deposit: BigInt(String(cost).replaceAll('"', "")),
    });
    if (outcome.value === null) {
      throw new Error(`the enclave returned nothing (transaction ${outcome.hash})`);
    }
    return { output: JSON.parse(outcome.value), transaction: outcome.hash };
  }
}
