import { useEffect, useMemo, useState } from "react";

import { ActionPanel, Empty, Field, Notice, PageHeader, Pending, Sheet, Skeleton, useAction, type Tab } from "../components/ui.tsx";
import { RoundTerms } from "../components/RoundTerms.tsx";
import { Verify } from "../components/Verify.tsx";
import { describe, dollars, percent, toMicros } from "../lib/format.ts";
import { holderKey, recordPayment, requestToJoin } from "../lib/keys.ts";
import { memoFor, publicKeyOf, Session } from "../lib/protocol.ts";
import { relay } from "../lib/relay.ts";
import type { Command, Mandate, Proposal, Statement } from "../lib/types.ts";

type Event = Parameters<Session["commit"]>[1];

export function Holder({ company, holder }: { company: string; holder: string }) {
  const session = useMemo(() => new Session(company), [company]);
  const actor = useMemo(() => ({ role: "holder" as const, holder }), [holder]);
  const { busy, run, status } = useAction();
  const [statement, setStatement] = useState<Statement | null>(null);
  const [proposal, setProposal] = useState<Proposal | null>(null);
  const [round, setRound] = useState<Mandate | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  async function send(command: Command) {
    return session.send(actor, command, await holderKey(company, holder));
  }

  async function refresh() {
    await run("Reading your position", async () => {
      const pending = await send({ kind: "get_proposal" });
      setProposal(pending.kind === "proposal" ? pending : null);
      const current = await send({ kind: "get_statement" });
      setStatement(current.kind === "statement" ? current : null);
      const open = await send({ kind: "get_round" });
      setRound(open.kind === "round" ? open.mandate : null);
      setLoaded(true);
    });
  }

  useEffect(() => {
    void (async () => {
      requestToJoin(company, { holder, key: await publicKeyOf(await holderKey(company, holder)) });
      await refresh();
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [company, holder]);

  /** Refreshes only after a success, so a failure stays on screen. */
  async function sign() {
    if (!proposal) return;
    setNotice(null);
    const signed = await run("Signing", async () => {
      const response = await send({ kind: "sign", digest: proposal.digest });
      if (response.kind === "rejected") throw new Error(response.reason);
      setNotice(response.kind === "committed" ? "Signed, recorded and anchored on NEAR." : "Signed. Waiting for the others.");
      setProposal(null);
      return true;
    });
    if (signed) await refresh();
  }

  async function act(event: Event, label: string) {
    setNotice(null);
    const recorded = await run(label, async () => {
      const secret = await holderKey(company, holder);
      const response = await session.commit({ actor, secret }, event, [{ actor, secret }]);
      if (response.kind === "rejected") throw new Error(response.reason);
      setNotice("Recorded and anchored on NEAR.");
      return true;
    });
    if (recorded) await refresh();
  }

  /** Pays with a memo naming this key, then proposes and signs the SAFE. No one else acts. */
  async function subscribe(micros: string) {
    setNotice(null);
    const recorded = await run("Paying and subscribing", async () => {
      const secret = await holderKey(company, holder);
      const settlement = await relay.paySubscription(micros, await memoFor(company, secret));
      const event: Event = {
        kind: "safe_subscribed",
        instrument: `round-${holder}-${settlement.reference.slice(0, 6).toLowerCase()}`,
        holder,
        key: await publicKeyOf(secret),
        purchase_micros: settlement.receivedMicros,
        settlement: settlement.reference,
      };
      const response = await session.commit({ actor, secret }, event, [{ actor, secret }]);
      if (response.kind === "rejected") throw new Error(response.reason);
      setNotice(`You are in: a ${dollars(settlement.receivedMicros)} SAFE, recorded and anchored on NEAR. Nobody had to approve it.`);
      return true;
    });
    if (recorded) await refresh();
  }

  async function pay(micros: string) {
    setNotice(null);
    await run("Paying into the company's treasury", async () => {
      const settlement = await relay.pay(micros);
      recordPayment(company, { holder, ...settlement });
      setNotice(`The treasury received ${dollars(settlement.receivedMicros)}. The company can now issue your SAFE against it.`);
    });
  }

  const joined = statement !== null;
  const tabs: Tab[] = [];
  if (round) {
    tabs.push({
      id: "round",
      label: "Round",
      content: <JoinRound round={round} disabled={busy || proposal !== null} onSubscribe={subscribe} />,
    });
  }
  tabs.push({ id: "invest", label: "Invest", content: <Invest disabled={busy || !joined} joined={loaded ? joined : null} onPay={pay} /> });
  if (joined) {
    tabs.push({
      id: "payout",
      label: "Payout",
      content: <PayoutAccount holder={holder} disabled={busy || proposal !== null} act={act} />,
    });
  }

  return (
    <main className="page">
      <PageHeader
        title="Your position"
        actions={
          <button type="button" className="btn" onClick={refresh} disabled={busy}>
            Refresh
          </button>
        }
      >
        You are {holder}, investing in {company}. You see your own lines and nothing about anyone else.
      </PageHeader>

      {status}
      {notice && <Notice tone="good">{notice}</Notice>}

      {proposal && (
        <Pending
          title="The company asks you to sign"
          actions={
            <button type="button" className="btn btn-ink" onClick={sign} disabled={busy}>
              Sign
            </button>
          }
        >
          <p>{describe(proposal.event)}</p>
          <p className="muted small">Your signature is checked against the key this browser holds. Nothing is recorded without it.</p>
        </Pending>
      )}

      <div className="workspace">
        <Sheet title={holder} aside={statement && <span className="muted small">{statement.lines.length} {statement.lines.length === 1 ? "line" : "lines"}</span>}>
          {!loaded ? (
            <Skeleton />
          ) : !statement ? (
            <Empty title="Not on the cap table yet">
              {round
                ? "This company has a round open. Join it from Next steps: you pay, you sign, and the enclave records your SAFE without anyone approving it."
                : "Your key was made in this browser and the company has your request. Once they add you and you sign, your position shows here."}
            </Empty>
          ) : (
            <Position statement={statement} disabled={busy || proposal !== null} act={act} />
          )}
          {statement && <Verify company={company} head={statement.head} entries={statement.entries} />}
        </Sheet>

        <ActionPanel title="Next steps" tabs={tabs} />
      </div>
    </main>
  );
}

function Position({ statement, disabled, act }: { statement: Statement; disabled: boolean; act: (event: Event, label: string) => Promise<void> }) {
  const exit = statement.exit;
  return (
    <>
      {statement.lines.length === 0 ? (
        <Empty title="Nothing issued to you yet">Pay into the treasury and the company can issue your SAFE against that payment.</Empty>
      ) : (
        <table className="table">
          <thead>
            <tr>
              <th>Instrument</th>
              <th className="num">Shares</th>
              <th className="num">Ownership</th>
            </tr>
          </thead>
          <tbody>
            {statement.lines.map((line) => (
              <tr key={line.instrument}>
                <td>
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
              <td>Company total</td>
              <td className="num fig">{Number(statement.capitalization).toLocaleString("en-US")}</td>
              <td className="num small">{statement.reconciles ? "Adds up" : "Does not add up"}</td>
            </tr>
          </tfoot>
        </table>
      )}

      {statement.mfn_offers.map((offer) => (
        <div key={offer.source} className="sheet-section">
          <h3>Better terms are available</h3>
          <p className="small">
            A later SAFE has {offer.valuation_cap_micros ? `a ${dollars(offer.valuation_cap_micros)} cap` : "better terms"}. Your most favored nation
            clause lets you adopt them without the company’s approval. You do not learn who holds it or what they paid.
          </p>
          <div>
            <button
              type="button"
              className="btn btn-ink"
              disabled={disabled}
              onClick={() => act({ kind: "mfn_elected", target: offer.target, source: offer.source }, "Adopting the better terms")}
            >
              Adopt the better terms
            </button>
          </div>
        </div>
      ))}

      {exit && (
        <div className="sheet-section">
          <h3>The company was sold for {dollars(exit.proceeds_micros)}</h3>
          <ul className="list">
            {exit.payouts.map((payout) => (
              <li key={payout.instrument}>
                <span className="who">
                  <strong className="fig">{dollars(payout.amount_micros)}</strong>
                  <span>{payout.took_preference ? "Your money back" : "As converted"}</span>
                </span>
                <span className="id">{payout.instrument}</span>
              </li>
            ))}
          </ul>
          <p className="muted small">All payouts together {exit.total_matches_proceeds ? "add up to the sale price exactly" : "do not add up to the sale price"}.</p>
        </div>
      )}
    </>
  );
}

/** `joined` is null while the position is still loading. */
function JoinRound({ round, disabled, onSubscribe }: { round: Mandate; disabled: boolean; onSubscribe: (micros: string) => Promise<void> }) {
  const [amount, setAmount] = useState(() => dollars(round.min_check_micros).slice(1));
  const [problem, setProblem] = useState<string | null>(null);

  function subscribe() {
    try {
      const micros = toMicros(amount);
      setProblem(null);
      void onSubscribe(micros);
    } catch (error) {
      setProblem(error instanceof Error ? error.message : String(error));
    }
  }

  return (
    <>
      <RoundTerms mandate={round} />
      <Field label="Your check ($)" hint="Test tokens on NEAR testnet. The relay pays them on your behalf.">
        <input value={amount} inputMode="decimal" onChange={(event) => setAmount(event.target.value)} />
      </Field>
      {problem && <Notice tone="bad">{problem}</Notice>}
      <button type="button" className="btn btn-ink" onClick={subscribe} disabled={disabled}>
        Pay and subscribe
      </button>
      <p className="hint">
        The payment carries a memo naming your key, so nobody else can claim it. The enclave checks it arrived and that the round has room.
      </p>
    </>
  );
}

function Invest({ disabled, joined, onPay }: { disabled: boolean; joined: boolean | null; onPay: (micros: string) => Promise<void> }) {
  const [amount, setAmount] = useState("2");
  const [problem, setProblem] = useState<string | null>(null);

  function pay() {
    try {
      const micros = toMicros(amount);
      setProblem(null);
      void onPay(micros);
    } catch (error) {
      setProblem(error instanceof Error ? error.message : String(error));
    }
  }

  return (
    <>
      <Field label="Amount ($)" hint="Test tokens on NEAR testnet. The relay pays them on your behalf.">
        <input value={amount} inputMode="decimal" onChange={(event) => setAmount(event.target.value)} />
      </Field>
      {problem && <Notice tone="bad">{problem}</Notice>}
      <button type="button" className="btn btn-ink" onClick={pay} disabled={disabled}>
        Pay into the treasury
      </button>
      {joined !== null && (
        <p className="hint">
          {joined
            ? "The company issues your SAFE against this exact payment. The enclave checks it arrived on chain."
            : "You can pay once the company has added you to the cap table."}
        </p>
      )}
    </>
  );
}

function PayoutAccount({ holder, disabled, act }: { holder: string; disabled: boolean; act: (event: Event, label: string) => Promise<void> }) {
  const [destination, setDestination] = useState("");
  return (
    <>
      <Field label="NEAR account" hint="Only you can set this. The company cannot redirect your payout.">
        <input value={destination} onChange={(event) => setDestination(event.target.value)} placeholder="you.testnet" autoComplete="off" spellCheck={false} />
      </Field>
      <button
        type="button"
        className="btn btn-ink"
        disabled={disabled || !destination.trim()}
        onClick={() => act({ kind: "payout_destination_set", holder, destination: destination.trim() }, "Saving your payout account")}
      >
        Save payout account
      </button>
    </>
  );
}
