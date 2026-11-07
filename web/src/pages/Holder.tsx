import { useEffect, useMemo, useState } from "react";

import { Card, Field, Notice, useAction } from "../components/ui.tsx";
import { Verify } from "../components/Verify.tsx";
import { describe, dollars, percent, toMicros } from "../lib/format.ts";
import { holderKey, recordPayment, requestToJoin } from "../lib/keys.ts";
import { publicKeyOf, Session } from "../lib/protocol.ts";
import { relay } from "../lib/relay.ts";
import type { Command, Proposal, Statement } from "../lib/types.ts";

export function Holder({ company, holder }: { company: string; holder: string }) {
  const session = useMemo(() => new Session(company), [company]);
  const actor = useMemo(() => ({ role: "holder" as const, holder }), [holder]);
  const { busy, run, status } = useAction();
  const [statement, setStatement] = useState<Statement | null>(null);
  const [proposal, setProposal] = useState<Proposal | null>(null);
  const [joined, setJoined] = useState(false);
  const [amount, setAmount] = useState("2");
  const [destination, setDestination] = useState("");
  const [notice, setNotice] = useState<string | null>(null);

  async function send(command: Command) {
    return session.send(actor, command, await holderKey(company, holder));
  }

  async function refresh() {
    await run("Reading your position", async () => {
      const pending = await send({ kind: "get_proposal" });
      setProposal(pending.kind === "proposal" ? pending : null);
      const current = await send({ kind: "get_statement" });
      setJoined(current.kind === "statement");
      setStatement(current.kind === "statement" ? current : null);
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
    const signed = await run("Signing", async () => {
      const response = await send({ kind: "sign", digest: proposal.digest });
      if (response.kind === "rejected") throw new Error(response.reason);
      setNotice(response.kind === "committed" ? "Signed and recorded." : "Signed.");
      setProposal(null);
      return true;
    });
    if (signed) await refresh();
  }

  async function act(event: Parameters<Session["commit"]>[1], label: string) {
    const recorded = await run(label, async () => {
      const secret = await holderKey(company, holder);
      const response = await session.commit({ actor, secret }, event, [{ actor, secret }]);
      if (response.kind === "rejected") throw new Error(response.reason);
      setNotice("Recorded.");
      return true;
    });
    if (recorded) await refresh();
  }

  async function pay() {
    await run("Paying into the treasury", async () => {
      const settlement = await relay.pay(toMicros(amount));
      recordPayment(company, { holder, ...settlement });
      setNotice(`The treasury received ${dollars(settlement.receivedMicros)}. The company can now issue your SAFE against it.`);
    });
  }

  return (
    <main className="page">
      <header className="page-header">
        <div>
          <p className="eyebrow">Investor view · {company}</p>
          <h1>{holder}</h1>
        </div>
        <button type="button" className="secondary" onClick={refresh} disabled={busy}>
          Refresh
        </button>
      </header>
      {status}
      {notice && <Notice tone="good">{notice}</Notice>}

      {proposal && (
        <Card title="The company asks you to sign">
          <p>{describe(proposal.event)}</p>
          <button type="button" onClick={sign} disabled={busy}>
            Sign
          </button>
        </Card>
      )}

      {!joined && !proposal && (
        <Notice>Your key was created in this browser. Back in the company console, add {holder} as an investor, then refresh here.</Notice>
      )}

      {joined && statement && (
        <>
          <Card title="Your position">
            {statement.lines.length === 0 && <p className="muted">Nothing issued to you yet.</p>}
            {statement.lines.map((line) => (
              <p key={line.instrument}>
                <span className="mono">{line.instrument}</span>: {percent(line.ownership_basis_points)}
              </p>
            ))}
            <p className="muted">
              The total of {Number(statement.capitalization).toLocaleString()} shares {statement.reconciles ? "adds up" : "does not add up"}. You see no other holder.
            </p>
            <Verify company={company} head={statement.head} entries={statement.entries} />
          </Card>

          {statement.mfn_offers.map((offer) => (
            <Card key={offer.source} title="Better terms are available">
              <p>
                A later SAFE has {offer.valuation_cap_micros ? `a ${dollars(offer.valuation_cap_micros)} cap` : "better terms"}. Your MFN clause lets you adopt them. You do not learn who holds it or how much they paid.
              </p>
              <button type="button" disabled={busy} onClick={() => act({ kind: "mfn_elected", target: offer.target, source: offer.source }, "Adopting the better terms")}>
                Adopt them, no approval needed
              </button>
            </Card>
          ))}

          {statement.exit && (
            <Card title="The company was sold">
              {statement.exit.payouts.map((payout) => (
                <p key={payout.instrument}>
                  You receive {dollars(payout.amount_micros)} of {dollars(statement.exit!.proceeds_micros)}
                  {payout.took_preference ? ", your money back" : ", as converted"}.
                </p>
              ))}
              <p className="muted">All payouts together {statement.exit.total_matches_proceeds ? "add up exactly" : "do not add up"}.</p>
            </Card>
          )}
        </>
      )}

      <div className="grid">
        <Card title="Invest">
          <Field label="Amount ($, test tokens)">
            <input value={amount} onChange={(event) => setAmount(event.target.value)} />
          </Field>
          <button type="button" onClick={pay} disabled={busy}>
            Pay into the treasury
          </button>
          <p className="muted">On this testnet demo the relay pays with its own test tokens on your behalf.</p>
        </Card>

        {joined && (
          <Card title="Where to be paid at an exit">
            <Field label="NEAR account">
              <input value={destination} onChange={(event) => setDestination(event.target.value)} placeholder="you.testnet" />
            </Field>
            <button type="button" disabled={busy || !destination.trim()} onClick={() => act({ kind: "payout_destination_set", holder, destination: destination.trim() }, "Saving your payout account")}>
              Save
            </button>
            <p className="muted">Only you can set this. The company cannot redirect your payout.</p>
          </Card>
        )}
      </div>
    </main>
  );
}
