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
}

export class Enclave {
  private identity: Promise<unknown> | null = null;

  constructor(
    private readonly near: Near,
    private readonly settings: EnclaveSettings,
  ) {}

  /**
   * The enclave's public keys never change, so they are asked for once. Callers
   * arriving together share the one run; a failed run is forgotten.
   */
  getIdentity(): Promise<unknown> {
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
    return this.run({ operation: "request", envelope });
  }

  private async run(input: unknown): Promise<unknown> {
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
    return JSON.parse(outcome.value);
  }
}
