import { useEffect, useMemo, useState } from "react";

import { Card, Field, Notice, useAction } from "../components/ui.tsx";
import { Verify } from "../components/Verify.tsx";
import { describe, dollars, percent, toMicros } from "../lib/format.ts";
import { companyKey, joinRequests, payments, type JoinRequest } from "../lib/keys.ts";
import { enclaveIdentity, Session } from "../lib/protocol.ts";
import { relay } from "../lib/relay.ts";
import type { Line, Proposal, Response } from "../lib/types.ts";

type Table = Extract<Response, { kind: "cap_table" }>;

const asCompany = (secret: string) => ({ actor: { role: "company" as const }, secret });

export function Company({ company }: { company: string }) {
  const session = useMemo(() => new Session(company), [company]);
  const { busy, run, status } = useAction();
  const [table, setTable] = useState<Table | null>(null);
  const [proposal, setProposal] = useState<Proposal | null>(null);
  const [joins, setJoins] = useState<JoinRequest[]>(() => joinRequests(company));
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    const refresh = () => setJoins(joinRequests(company));
    window.addEventListener("storage", refresh);
    return () => window.removeEventListener("storage", refresh);
  }, [company]);

  async function send(command: Parameters<Session["send"]>[1]) {
    const secret = await companyKey(company);
    const response = await session.send({ role: "company" }, command, secret);
    if (response.kind === "rejected") throw new Error(response.reason);
    return response;
  }

  async function refresh() {
    await run("Reading the cap table", async () => {
      const response = await send({ kind: "get_cap_table" });
      if (response.kind === "cap_table") setTable(response);
      const pending = await session.send({ role: "company" }, { kind: "get_proposal" }, await companyKey(company));
      setProposal(pending.kind === "proposal" ? pending : null);
    });
  }

  useEffect(() => {
    void refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [company]);

  /** Proposes as the company and signs at once; holders then sign in their own view. */
  async function propose(event: Parameters<Session["commit"]>[1], label: string) {
    await run(label, async () => {
      const secret = await companyKey(company);
      const response = await session.commit(asCompany(secret), event, [asCompany(secret)]);
      if (response.kind === "rejected") throw new Error(response.reason);
      setProposal(response.kind === "proposal" ? response : null);
      setNotice(
        response.kind === "proposal" && response.holder_signatures_missing.length > 0
          ? `Waiting for ${response.holder_signatures_missing.join(", ")} to sign in their own view.`
          : "Recorded.",
      );
    });
  }

  const holders = (table?.lines ?? []).map((line) => line.holder);
  const registered = new Set(holders);

  return (
    <main className="page">
      <header className="page-header">
        <div>
          <p className="eyebrow">Company console</p>
          <h1>{company}</h1>
        </div>
        <button type="button" className="secondary" onClick={refresh} disabled={busy}>
          Refresh
        </button>
      </header>
      {status}
      {notice && <Notice tone="good">{notice}</Notice>}

      <Invite company={company} />

      {proposal && (
        <Card title="Waiting for signatures">
          <p>{describe(proposal.event)}</p>
          <p className="muted">
            Missing: {proposal.holder_signatures_missing.join(", ") || "none"}
            {proposal.company_signature_missing ? ", the company" : ""}
          </p>
          <button type="button" className="secondary" disabled={busy} onClick={() => run("Cancelling", () => send({ kind: "cancel_proposal" }).then(() => setProposal(null)))}>
            Cancel
          </button>
        </Card>
      )}

      <Card title="Cap table">
        {table ? <CapTable lines={table.lines} capitalization={table.capitalization} /> : <p className="muted">Loading…</p>}
        {table && <Verify company={company} head={table.head} entries={table.entries} />}
      </Card>

      <div className="grid">
        <Card title="Add an investor">
          {joins.filter((join) => !registered.has(join.holder)).length === 0 && (
            <p className="muted">Open an investor link above. Their join request appears here.</p>
          )}
          {joins
            .filter((join) => !registered.has(join.holder))
            .map((join) => (
              <div key={join.holder} className="row">
                <span>{join.holder}</span>
                <button type="button" disabled={busy || proposal !== null} onClick={() => propose({ kind: "holder_registered", holder: join.holder, key: join.key }, `Inviting ${join.holder}`)}>
                  Add
                </button>
              </div>
            ))}
        </Card>

        <IssueSafe company={company} disabled={busy || proposal !== null} onIssue={(event) => propose(event, `Issuing a SAFE to ${event.holder}`)} />

        <ApproveExit company={company} disabled={busy || proposal !== null} run={run} send={send} propose={propose} />
      </div>
    </main>
  );
}

function Invite({ company }: { company: string }) {
  const links = ["ana", "ben"].map((holder) => ({
    holder,
    url: `${window.location.origin}${window.location.pathname}#/holder/${company}/${holder}`,
  }));
  return (
    <Card title="Investor links">
      <p className="muted">Open each in a new tab. Every investor gets their own key in their own view.</p>
      {links.map((link) => (
        <p key={link.holder}>
          <a href={link.url} target="_blank" rel="noreferrer">
            Open as {link.holder}
          </a>
        </p>
      ))}
    </Card>
  );
}

function CapTable({ lines, capitalization }: { lines: Line[]; capitalization: string }) {
  return (
    <table>
      <thead>
        <tr>
          <th>Holder</th>
          <th>Instrument</th>
          <th>Shares</th>
          <th>Ownership</th>
        </tr>
      </thead>
      <tbody>
        {lines.map((line) => (
          <tr key={line.instrument}>
            <td>{line.holder}</td>
            <td className="mono">{line.instrument}</td>
            <td>{line.shares ? Number(line.shares).toLocaleString() : "-"}</td>
            <td>{percent(line.ownership_basis_points)}</td>
          </tr>
        ))}
      </tbody>
      <tfoot>
        <tr>
          <td colSpan={2}>Fully diluted</td>
          <td>{Number(capitalization).toLocaleString()}</td>
          <td />
        </tr>
      </tfoot>
    </table>
  );
}

function IssueSafe({
  company,
  disabled,
  onIssue,
}: {
  company: string;
  disabled: boolean;
  onIssue: (event: Extract<Parameters<Session["commit"]>[1], { kind: "safe_issued" }>) => void;
}) {
  const [cap, setCap] = useState("30");
  const [mfn, setMfn] = useState(false);
  const received = payments(company);
  const [chosen, setChosen] = useState(0);
  const payment = received[chosen];

  return (
    <Card title="Issue a SAFE">
      {received.length === 0 ? (
        <p className="muted">An investor pays first in their view. Their payment shows up here.</p>
      ) : (
        <>
          <Field label="Payment">
            <select value={chosen} onChange={(event) => setChosen(Number(event.target.value))}>
              {received.map((item, index) => (
                <option key={item.reference} value={index}>
                  {item.holder}: {dollars(item.receivedMicros)}
                </option>
              ))}
            </select>
          </Field>
          <Field label="Post-money cap ($, empty for none)">
            <input value={cap} onChange={(event) => setCap(event.target.value)} />
          </Field>
          <label className="check">
            <input type="checkbox" checked={mfn} onChange={(event) => setMfn(event.target.checked)} /> MFN clause
          </label>
          <button
            type="button"
            disabled={disabled || !payment}
            onClick={() =>
              payment &&
              onIssue({
                kind: "safe_issued",
                instrument: `safe-${payment.holder}-${payment.reference.slice(0, 6).toLowerCase()}`,
                holder: payment.holder,
                purchase_micros: payment.receivedMicros,
                valuation_cap_micros: cap.trim() ? toMicros(cap) : null,
                discount_basis_points: null,
                mfn,
                settlement: payment.reference,
              })
            }
          >
            Issue
          </button>
          <p className="muted">The enclave refuses it unless this exact payment reached the treasury on chain.</p>
        </>
      )}
    </Card>
  );
}

function ApproveExit({
  company,
  disabled,
  run,
  send,
  propose,
}: {
  company: string;
  disabled: boolean;
  run: ReturnType<typeof useAction>["run"];
  send: (command: Parameters<Session["send"]>[1]) => Promise<Response>;
  propose: (event: Parameters<Session["commit"]>[1], label: string) => Promise<void>;
}) {
  const [proceeds, setProceeds] = useState("10");
  const [paid, setPaid] = useState<string[]>([]);

  async function payOut() {
    await run("Paying out", async () => {
      const plan = await send({ kind: "get_payout_plan" });
      if (plan.kind !== "payout_plan") throw new Error("no approved exit yet");
      const enclave = await enclaveIdentity();
      await relay.openPayouts(company, enclave.signing_key, plan.proceeds_micros);
      const lines: string[] = [];
      for (const instruction of plan.instructions) {
        await relay.payOut(company, instruction);
        lines.push(`${dollars(instruction.amount_micros)} to ${instruction.recipient}`);
        setPaid([...lines]);
      }
    });
  }

  return (
    <Card title="Sell the company">
      <Field label="Sale price ($)">
        <input value={proceeds} onChange={(event) => setProceeds(event.target.value)} />
      </Field>
      <button type="button" disabled={disabled} onClick={() => propose({ kind: "exit_approved", proceeds_micros: toMicros(proceeds) }, "Approving the sale")}>
        Approve once
      </button>
      <button type="button" className="secondary" disabled={disabled} onClick={payOut}>
        Pay everyone out
      </button>
      <p className="muted">Everyone paid needs a payout account first. The contract pays only what the enclave signed.</p>
      {paid.map((line) => (
        <p key={line}>Paid {line}</p>
      ))}
    </Card>
  );
}
