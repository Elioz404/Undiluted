import { useState } from "react";

import { shortHex } from "../lib/format.ts";
import { relay } from "../lib/relay.ts";
import { Notice } from "./ui.tsx";

/**
 * Compares the head the enclave showed this person with the one published on
 * chain. If they match, the history behind it is the same one everyone sees.
 */
export function Verify({ company, head, entries }: { company: string; head: string; entries: number }) {
  const [result, setResult] = useState<"match" | "mismatch" | "missing" | null>(null);
  const [checking, setChecking] = useState(false);

  async function check() {
    setChecking(true);
    try {
      const published = await relay.headAt(company, entries - 1);
      setResult(published.head === null ? "missing" : published.head === head ? "match" : "mismatch");
    } finally {
      setChecking(false);
    }
  }

  return (
    <div className="verify">
      <p className="mono">
        Head {shortHex(head)} after {entries} entries
      </p>
      <button type="button" className="secondary" onClick={check} disabled={checking || entries === 0}>
        {checking ? "Checking…" : "Check against the chain"}
      </button>
      {result === "match" && (
        <Notice tone="good">It matches the head published on NEAR. Nobody can be shown a different history.</Notice>
      )}
      {result === "mismatch" && <Notice tone="bad">It does not match the published head. Do not trust this view.</Notice>}
      {result === "missing" && <Notice tone="info">That head is not published yet. Try again in a moment.</Notice>}
    </div>
  );
}
