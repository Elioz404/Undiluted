import { useEffect, useMemo, useState } from "react";

import { ActionPanel, Empty, Field, Notice, PageHeader, Pending, Sheet, Skeleton, useAction } from "../components/ui.tsx";
import { RoundTerms } from "../components/RoundTerms.tsx";
import { Verify } from "../components/Verify.tsx";
import { describe, dollars, percent, toMicros } from "../lib/format.ts";
import { companyKey, joinRequests, payments, type JoinRequest } from "../lib/keys.ts";
import { Session } from "../lib/protocol.ts";
import { relay } from "../lib/relay.ts";
import type { Line, Mandate, Proposal, Response } from "../lib/types.ts";

type Table = Extract<Response, { kind: "cap_table" }>;
type Propose = (event: Parameters<Session["commit"]>[1], label: string) => Promise<void>;

/** The investors this demo hands out links for. */
const DEMO_INVESTORS = ["ana", "ben"];

const asCompany = (secret: string) => ({ actor: { role: "company" as const }, secret });

export function Company({ company }: { company: string }) {
  const session = useMemo(() => new Session(company), [company]);
  const { busy, run, status } = useAction();
  const [table, setTable] = useState<Table | null>(null);
  const [proposal, setProposal] = useState<Proposal | null>(null);
  const [round, setRound] = useState<Mandate | null>(null);
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
    setJoins(joinRequests(company));
    await run("Reading the cap table", async () => {
      const response = await send({ kind: "get_cap_table" });
      if (response.kind === "cap_table") setTable(response);
      const current = await send({ kind: "get_round" });
      if (current.kind === "round") setRound(current.mandate);
      const pending = await session.send({ role: "company" }, { kind: "get_proposal" }, await companyKey(company));
      setProposal(pending.kind === "proposal" ? pending : null);
    });
  }

  useEffect(() => {
    void refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [company]);

  /** Proposes as the company and signs at once; holders then sign in their own view. */
  const propose: Propose = async (event, label) => {
    setNotice(null);
    const recorded = await run(label, async () => {
      const secret = await companyKey(company);
      const response = await session.commit(asCompany(secret), event, [asCompany(secret)]);
      if (response.kind === "rejected") throw new Error(response.reason);
      setProposal(response.kind === "proposal" ? response : null);
      return response.kind === "committed";
    });
    if (recorded) {
      setNotice("Recorded and anchored on NEAR.");
      await refresh();
    }
  };

  async function cancel() {
    const cancelled = await run("Withdrawing the proposal", () => send({ kind: "cancel_proposal" }));
    if (cancelled) setProposal(null);
  }

  const blocked = busy || proposal !== null;
  const registered = new Set(table?.holders ?? []);

  return (
    <main className="page">
      <PageHeader
        title="Cap table"
        actions={
          <button type="button" className="btn" onClick={refresh} disabled={busy}>
            Refresh
          </button>
        }
      >
        You see every holder. Each investor sees only their own lines.
      </PageHeader>

      {status}
      {notice && <Notice tone="good">{notice}</Notice>}

      {proposal && (
        <Pending
          title="Waiting for signatures"
          actions={
            <button type="button" className="btn" disabled={busy} onClick={cancel}>
              Withdraw
            </button>
          }
        >
          <p>{describe(proposal.event)}</p>
          <p className="muted small">
            {proposal.holder_signatures_missing.length > 0
              ? `${proposal.holder_signatures_missing.join(" and ")} must sign in their own view. Nothing else can be recorded until then.`
              : "The company still has to sign."}
          </p>
        </Pending>
      )}

      <div className="workspace">
        <Sheet title={company} aside={table && <span className="muted small">{table.holders.length} registered</span>}>
          {table === null ? (
            <Skeleton />
          ) : table.lines.length === 0 ? (
            <Empty title="Nothing issued yet">Invite an investor and issue a SAFE against their payment.</Empty>
          ) : (
            <CapTable lines={table.lines} capitalization={table.capitalization} />
          )}
          {table && <Verify company={company} head={table.head} entries={table.entries} />}
        </Sheet>

        <ActionPanel
          title="Next steps"
          tabs={[
            {
              id: "investors",
              label: "Investors",
              content: <Investors company={company} joins={joins} registered={registered} disabled={blocked} propose={propose} />,
            },
            {
              id: "round",
              label: "Round",
              content: <Round round={round} disabled={blocked} propose={propose} />,
            },
            {
              id: "safe",
              label: "SAFE",
              content: <IssueSafe company={company} registered={registered} disabled={blocked} propose={propose} />,
            },
            {
              id: "exit",
              label: "Sell",
              content: <SellCompany company={company} disabled={blocked} run={run} send={send} propose={propose} />,
            },
          ]}
        />
      </div>
    </main>
  );
}

function CapTable({ lines, capitalization }: { lines: Line[]; capitalization: string }) {
  return (
    <table className="table">
      <thead>
        <tr>
          <th>Holder</th>
          <th className="hide-narrow">Instrument</th>
          <th className="num">Shares</th>
          <th className="num">Ownership</th>
        </tr>
      </thead>
      <tbody>
        {lines.map((line) => (
          <tr key={line.instrument}>
            <td>{line.holder}</td>
            <td className="hide-narrow">
              <span className="id">{line.instrument}</span>
            </td>
            <td className="num fig">{line.shares ? Number(line.shares).toLocaleString("en-US") : "–"}</td>
            <td className="num">
              {line.ownership_basis_points === null ? (
                <span className="pending-round">At the next round</span>
              ) : (
                <span className="fig">{percent(line.ownership_basis_points)}</span>
              )}
            </td>
          </tr>
        ))}
      </tbody>
      <tfoot>
        <tr>
          <td>Fully diluted</td>
          <td className="hide-narrow" />
          <td className="num fig">{Number(capitalization).toLocaleString("en-US")}</td>
          <td className="num fig">100.00%</td>
        </tr>
      </tfoot>
    </table>
  );
}

function Investors({
  company,
  joins,
  registered,
  disabled,
  propose,
}: {
  company: string;
  joins: JoinRequest[];
  registered: Set<string>;
  disabled: boolean;
  propose: Propose;
}) {
  const base = `${window.location.origin}${window.location.pathname}`;
  return (
    <>
      <p className="hint">Open an investor’s view in a new tab. It makes their key there, then their request to join shows here.</p>
      <ul className="list">
        {DEMO_INVESTORS.map((holder) => {
          const join = joins.find((request) => request.holder === holder);
          const state = registered.has(holder) ? "On the cap table" : join ? "Asked to join" : "Not opened yet";
          return (
            <li key={holder}>
              <span className="who">
                <strong>{holder}</strong>
                <span>{state}</span>
              </span>
              {join && !registered.has(holder) ? (
                <button
                  type="button"
                  className="btn btn-ink"
                  disabled={disabled}
                  onClick={() => propose({ kind: "holder_registered", holder, key: join.key }, `Inviting ${holder}`)}
                >
                  Add
                </button>
              ) : (
                <a className="btn" href={`${base}#/holder/${company}/${holder}`} target="_blank" rel="noreferrer">
                  Open view
                </a>
              )}
            </li>
          );
        })}
      </ul>
    </>
  );
}

/**
 * The company sets a round's limits once. From then on investors join on their
 * own and the enclave records each SAFE that stays within them.
 */
function Round({ round, disabled, propose }: { round: Mandate | null; disabled: boolean; propose: Propose }) {
  const [cap, setCap] = useState("30");
  const [mfn, setMfn] = useState(false);
  const [minimum, setMinimum] = useState("1");
  const [size, setSize] = useState("5");
  const [problem, setProblem] = useState<string | null>(null);

  if (round) {
    return (
      <>
        <RoundTerms mandate={round} />
        <p className="hint">
          Investors join from their own view: they pay, sign, and the enclave records each SAFE that stays within these limits. You approve nothing.
        </p>
        <button type="button" className="btn" disabled={disabled} onClick={() => propose({ kind: "safe_round_ended" }, "Ending the round")}>
          End the round
        </button>
      </>
    );
  }

  function open() {
    try {
      const mandate: Mandate = {
        terms: { valuation_cap_micros: cap.trim() ? toMicros(cap) : null, discount_basis_points: null, mfn },
        min_check_micros: toMicros(minimum),
        max_raise_micros: toMicros(size),
      };
      setProblem(null);
      void propose({ kind: "safe_round_opened", mandate }, "Opening the round");
    } catch (error) {
      setProblem(error instanceof Error ? error.message : String(error));
    }
  }

  return (
    <>
      <p className="hint">Set the limits once. After that, the round runs without you.</p>
      <Field label="Post-money valuation cap ($)" hint="Leave empty for an uncapped SAFE.">
        <input value={cap} inputMode="decimal" onChange={(event) => setCap(event.target.value)} />
      </Field>
      <label className="check">
        <input type="checkbox" checked={mfn} onChange={(event) => setMfn(event.target.checked)} />
        Most favored nation clause
      </label>
      <Field label="Smallest check ($)">
        <input value={minimum} inputMode="decimal" onChange={(event) => setMinimum(event.target.value)} />
      </Field>
      <Field label="Round size ($)" hint="Subscriptions stop once this much has come in.">
        <input value={size} inputMode="decimal" onChange={(event) => setSize(event.target.value)} />
      </Field>
      {problem && <Notice tone="bad">{problem}</Notice>}
      <button type="button" className="btn btn-ink" disabled={disabled} onClick={open}>
        Open the round
      </button>
    </>
  );
}

function IssueSafe({
  company,
  registered,
  disabled,
  propose,
}: {
  company: string;
  registered: Set<string>;
  disabled: boolean;
  propose: Propose;
}) {
  const [cap, setCap] = useState("30");
  const [mfn, setMfn] = useState(false);
  const received = payments(company).filter((payment) => registered.has(payment.holder));
  const [chosen, setChosen] = useState(0);
  const payment = received[chosen];
  const [problem, setProblem] = useState<string | null>(null);

  if (received.length === 0) {
    return (
      <Empty title="No payment to issue against">
        A SAFE is issued against money that arrived. Once an investor on the cap table pays in their view, the payment shows here.
      </Empty>
    );
  }

  function issue() {
    if (!payment) return;
    let valuationCap: string | null;
    try {
      valuationCap = cap.trim() ? toMicros(cap) : null;
    } catch (error) {
      setProblem(error instanceof Error ? error.message : String(error));
      return;
    }
    setProblem(null);
    void propose(
      {
        kind: "safe_issued",
        instrument: `safe-${payment.holder}-${payment.reference.slice(0, 6).toLowerCase()}`,
        holder: payment.holder,
        purchase_micros: payment.receivedMicros,
        valuation_cap_micros: valuationCap,
        discount_basis_points: null,
        mfn,
        settlement: payment.reference,
      },
      `Issuing a SAFE to ${payment.holder}`,
    );
  }

  return (
    <>
      <Field label="Payment received">
        <select value={chosen} onChange={(event) => setChosen(Number(event.target.value))}>
          {received.map((item, index) => (
            <option key={item.reference} value={index}>
              {item.holder}, {dollars(item.receivedMicros)}
            </option>
          ))}
        </select>
      </Field>
      <Field label="Post-money valuation cap ($)" hint="Leave empty for an uncapped SAFE.">
        <input value={cap} inputMode="decimal" onChange={(event) => setCap(event.target.value)} />
      </Field>
      <label className="check">
        <input type="checkbox" checked={mfn} onChange={(event) => setMfn(event.target.checked)} />
        Most favored nation clause
      </label>
      {problem && <Notice tone="bad">{problem}</Notice>}
      <button type="button" className="btn btn-ink" disabled={disabled || !payment} onClick={issue}>
        Issue SAFE
      </button>
      <p className="hint">The enclave refuses it unless this exact payment reached the treasury on chain and no other SAFE used it.</p>
    </>
  );
}

function SellCompany({
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
  propose: Propose;
}) {
  const [proceeds, setProceeds] = useState("10");
  const [paid, setPaid] = useState<string[]>([]);
  const [problem, setProblem] = useState<string | null>(null);

  function approve() {
    let micros: string;
    try {
      micros = toMicros(proceeds);
    } catch (error) {
      setProblem(error instanceof Error ? error.message : String(error));
      return;
    }
    setProblem(null);
    void propose({ kind: "exit_approved", proceeds_micros: micros }, "Approving the sale");
  }

  async function payOut() {
    await run("Paying everyone out", async () => {
      const plan = await send({ kind: "get_payout_plan" });
      if (plan.kind !== "payout_plan") throw new Error("approve a sale first");
      await relay.openPayouts(company, plan.proceeds_micros);
      const lines: string[] = [];
      for (const instruction of plan.instructions) {
        await relay.payOut(company, instruction);
        lines.push(`${dollars(instruction.amount_micros)} to ${instruction.recipient}`);
        setPaid([...lines]);
      }
    });
  }

  return (
    <>
      <Field label="Sale price ($)" hint="Split by the waterfall: money back or as converted, whichever is more.">
        <input value={proceeds} inputMode="decimal" onChange={(event) => setProceeds(event.target.value)} />
      </Field>
      {problem && <Notice tone="bad">{problem}</Notice>}
      <button type="button" className="btn btn-ink" disabled={disabled} onClick={approve}>
        Approve the sale
      </button>
      <div className="sheet-section">
        <h3>Then pay everyone</h3>
        <p className="hint">Each holder needs a payout account. The contract pays only what the enclave signed, once.</p>
        <button type="button" className="btn" disabled={disabled} onClick={payOut}>
          Pay everyone out
        </button>
        {paid.length > 0 && (
          <ul className="list">
            {paid.map((line) => (
              <li key={line}>
                <span>Paid {line}</span>
              </li>
            ))}
          </ul>
        )}
      </div>
    </>
  );
}
