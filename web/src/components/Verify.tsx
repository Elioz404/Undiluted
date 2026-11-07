import { useState } from "react";

import { shortHex } from "../lib/format.ts";
import { relay } from "../lib/relay.ts";

type Result = "match" | "mismatch" | "missing" | "failed";

/**
 * Compares the head the enclave showed this person with the one published on
 * chain. If they match, the history behind it is the same one everyone sees.
 */
export function Verify({ company, head, entries }: { company: string; head: string; entries: number }) {
  const [result, setResult] = useState<Result | null>(null);
  const [checking, setChecking] = useState(false);

  async function check() {
    setChecking(true);
    try {
      const published = await relay.headAt(company, entries - 1);
      setResult(published.head === null ? "missing" : published.head === head ? "match" : "mismatch");
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
      {result === "mismatch" && (
        <p className="check-result bad" role="alert">
          This does not match the head published on chain. Do not trust this view.
        </p>
      )}
    </footer>
  );
}
