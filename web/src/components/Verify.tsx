import { useState } from "react";

import { shortHex } from "../lib/format.ts";
import { publishedHead, registeredEnclaveKey, trustedEnclaveKey } from "../lib/network.ts";

type Result = "match" | "mismatch" | "missing" | "foreign" | "failed";

/**
 * Compares the head the enclave showed this person with the one published on
 * chain, read from NEAR directly. If they match, and the company's history can
 * only be extended by the enclave this site trusts, it is the same history
 * everyone else sees.
 */
export function Verify({ company, head, entries }: { company: string; head: string; entries: number }) {
  const [result, setResult] = useState<Result | null>(null);
  const [checking, setChecking] = useState(false);

  async function check() {
    setChecking(true);
    try {
      const [registered, published] = await Promise.all([
        registeredEnclaveKey(company),
        publishedHead(company, entries - 1),
      ]);
      if (registered !== trustedEnclaveKey()) {
        setResult("foreign");
      } else {
        setResult(published === null ? "missing" : published === head ? "match" : "mismatch");
      }
    } catch {
      setResult("failed");
    } finally {
      setChecking(false);
    }
  }

  return (
    <footer className="sheet-foot check-chain">
      <span className="head">
        History of {entries} {entries === 1 ? "entry" : "entries"}, head <span className="id">{shortHex(head)}</span>
      </span>
      {result === "match" ? (
        <span className="stamp">Matches NEAR</span>
      ) : (
        <button type="button" className="btn" onClick={check} disabled={checking || entries === 0}>
          {checking ? "Checking…" : "Check against NEAR"}
        </button>
      )}
      <p className="check-result" role="status">
        {result === "match" && "The same head is published on chain, so nobody can be shown a different history."}
        {result === "missing" && "That head is not on chain yet. Try again in a moment."}
        {result === "failed" && "The chain could not be reached. Try again in a moment."}
      </p>
      {result === "foreign" && (
        <p className="check-result bad" role="alert">
          This company's history on chain is not signed by the enclave this site trusts. Do not trust this view.
        </p>
      )}
      {result === "mismatch" && (
        <p className="check-result bad" role="alert">
          This does not match the head published on chain. Do not trust this view.
        </p>
      )}
    </footer>
  );
}
