import { useState } from "react";

import { checkEnclave, type EnclaveFindings } from "../lib/enclaveCheck.ts";
import { shortHex } from "../lib/format.ts";

/** Lets anyone check, in their own browser, that the enclave is genuine. */
export function EnclaveCheck() {
  const [findings, setFindings] = useState<EnclaveFindings | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);

  async function check() {
    setChecking(true);
    setProblem(null);
    try {
      setFindings(await checkEnclave());
    } catch (error) {
      setFindings(null);
      setProblem(error instanceof Error ? error.message : String(error));
    } finally {
      setChecking(false);
    }
  }

  return (
    <section className="enclave-check" aria-labelledby="enclave-check-title">
      <div className="enclave-check-intro">
        <h2 id="enclave-check-title">Check the enclave yourself</h2>
        <p>
          Your browser verifies Intel's signature on the enclave's attestation, the image OutLayer approved on NEAR,
          and that it runs this repository's code. The relay only says where to look.
        </p>
        {!findings && (
          <button type="button" className="btn" onClick={check} disabled={checking}>
            {checking ? "Checking with Intel and NEAR…" : "Verify the enclave"}
          </button>
        )}
        {problem && (
          <p className="check-result bad" role="alert">
            The enclave did not check out: {problem}
          </p>
        )}
      </div>

      {findings && (
        <div className="sheet enclave-findings">
          <dl className="findings">
            <div>
              <dt>Hardware</dt>
              <dd>
                Intel TDX quote signed by Intel, platform <span className="fig">{findings.tcb_status}</span>
              </dd>
            </div>
            <div>
              <dt>Image</dt>
              <dd>
                One OutLayer approved on NEAR, MRTD <span className="id">{shortHex(findings.measurements.mrtd)}</span>
              </dd>
            </div>
            <div>
              <dt>Code</dt>
              <dd>
                This repository's enclave, wasm <span className="id">{shortHex(findings.wasm_hash)}</span>
              </dd>
            </div>
            <div>
              <dt>Keys</dt>
              <dd>
                Signing key <span className="id">{shortHex(findings.signing_key)}</span>, the one this site talks to
              </dd>
            </div>
          </dl>
          <footer className="sheet-foot check-chain">
            <span className="head">
              Attested run <span className="id">{shortHex(findings.transaction)}</span>
            </span>
            <span className="stamp">Genuine enclave</span>
          </footer>
        </div>
      )}
    </section>
  );
}
