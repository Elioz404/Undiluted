/**
 * Checks that the enclave this site talks to is the code in this repository,
 * running in a genuine Intel TDX enclave. The relay only points at the run that
 * produced the enclave's identity; everything else comes from NEAR, and the
 * verdict comes from the same Rust code the CLI runs, compiled to wasm.
 */

import { verifyEnclaveRun } from "../wasm/undiluted_web.js";
import { outlayerTrustAnchors, transactionOutputs, trustedEnclaveKey, trustedEnclaveWasm } from "./network.ts";
import { ready } from "./protocol.ts";
import { relay } from "./relay.ts";

export interface EnclaveFindings {
  tcb_status: string;
  measurements: { mrtd: string; rtmr0: string; rtmr1: string; rtmr2: string; rtmr3: string };
  wasm_hash: string;
  output_hash: string;
  signing_key: string;
  encryption_key: string;
  transaction: string;
}

async function sha256Hex(base64: string): Promise<string> {
  const bytes = Uint8Array.from(atob(base64), (character) => character.charCodeAt(0));
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, "0")).join("");
}

export async function checkEnclave(): Promise<EnclaveFindings> {
  await ready();
  const run = await relay.attestation();
  const attestation = run.attestation as { output_hash?: string };
  const outputs = await transactionOutputs(run.transaction, run.sender);
  const hashes = await Promise.all(outputs.map(sha256Hex));
  const output = outputs[hashes.indexOf(String(attestation.output_hash).toLowerCase())];
  if (!output) {
    throw new Error("the run on chain did not return the output the enclave attested");
  }
  const { approved, collaterals } = await outlayerTrustAnchors();

  const findings = JSON.parse(
    verifyEnclaveRun(
      JSON.stringify({
        attestation: run.attestation,
        output,
        wasm_hash: trustedEnclaveWasm(),
        approved,
        collaterals,
        now_secs: Math.floor(Date.now() / 1000),
      }),
    ),
  ) as Omit<EnclaveFindings, "transaction">;
  if (findings.signing_key !== trustedEnclaveKey()) {
    throw new Error("the attested enclave is not the one this site uses");
  }
  return { ...findings, transaction: run.transaction };
}
