import { useAction } from "../components/ui.tsx";
import { companyKey, holderKey } from "../lib/keys.ts";
import { enclaveIdentity, publicKeyOf, Session } from "../lib/protocol.ts";
import { relay } from "../lib/relay.ts";
import { navigate } from "../router.ts";

const FOUNDER = "founder";
const FOUNDER_SHARES = "8000000";
const SOURCE = "https://github.com/Elioz404/Undiluted";

function newCompanyId(): string {
  const suffix = crypto.getRandomValues(new Uint8Array(4));
  return `co-${Array.from(suffix, (byte) => byte.toString(16).padStart(2, "0")).join("")}`;
}

export function Landing() {
  const { busy, run, status } = useAction();

  async function start() {
    const company = newCompanyId();
    const created = await run("Setting up your company", async () => {
      const session = new Session(company);
      const secret = await companyKey(company);
      const asCompany = { actor: { role: "company" as const }, secret };
      const config = await relay.config();
      const enclave = await enclaveIdentity();

      const response = await session.send(asCompany.actor, { kind: "create_company" }, secret);
      if (response.kind === "rejected") throw new Error(response.reason);
      await relay.registerCompany(company, enclave.signing_key);

      const treasury = await session.commit(
        asCompany,
        { kind: "treasury_designated", account: config.treasury, token: config.token },
        [asCompany],
      );
      if (treasury.kind === "rejected") throw new Error(treasury.reason);

      const founderSecret = await holderKey(company, FOUNDER);
      const asFounder = { actor: { role: "holder" as const, holder: FOUNDER }, secret: founderSecret };
      for (const event of [
        { kind: "holder_registered" as const, holder: FOUNDER, key: await publicKeyOf(founderSecret) },
        { kind: "common_issued" as const, instrument: "founder-stock", holder: FOUNDER, shares: FOUNDER_SHARES },
      ]) {
        const committed = await session.commit(asCompany, event, [asCompany, asFounder]);
        if (committed.kind === "rejected") throw new Error(committed.reason);
      }
      return company;
    });
    if (created) navigate(`/company/${created}`);
  }

  return (
    <main className="page">
      <section className="hero">
        <div>
          <h1>A cap table nobody can quietly dilute</h1>
          <p className="lead">Each investor sees only their own stake, and can prove nothing changed behind their back.</p>
          <div className="actions">
            <button type="button" className="btn btn-ink btn-hero" onClick={start} disabled={busy}>
              Start a demo company
            </button>
            {status}
          </div>
        </div>
        <ExampleStatement />
      </section>

      <dl className="guarantees">
        <div>
          <dt>Private by default</dt>
          <dd>The table lives in a TEE. Your browser and the enclave talk end to end; the relay only carries ciphertext.</dd>
        </div>
        <div>
          <dt>Paid means paid</dt>
          <dd>A SAFE is recorded only when its exact payment reached the treasury on chain, and only once.</dd>
        </div>
        <div>
          <dt>Rights enforce themselves</dt>
          <dd>An MFN holder adopts better terms alone. At a sale, payouts follow the waterfall and cannot be redirected.</dd>
        </div>
      </dl>

      <footer className="footer">
        <span>Runs on NEAR testnet with test tokens of no value. Keys stay in your browser.</span>
        <a href={SOURCE} target="_blank" rel="noreferrer">
          Source on GitHub
        </a>
      </footer>
    </main>
  );
}

/** What an investor's view looks like, with illustrative figures. */
function ExampleStatement() {
  return (
    <figure>
      <div className="sheet">
        <div className="sheet-head">
          <h2>ana</h2>
          <span className="muted small">1 line</span>
        </div>
        <table className="table">
          <thead>
            <tr>
              <th>Instrument</th>
              <th className="num">Shares</th>
              <th className="num">Ownership</th>
            </tr>
          </thead>
          <tbody>
            <tr>
              <td>
                <span className="id">safe-ana</span>
              </td>
              <td className="num fig">640,000</td>
              <td className="num fig">6.67%</td>
            </tr>
          </tbody>
          <tfoot>
            <tr>
              <td>Company total</td>
              <td className="num fig">9,600,000</td>
              <td className="num small">Adds up</td>
            </tr>
          </tfoot>
        </table>
        <div className="sheet-foot check-chain">
          <span className="head">
            History of 7 entries, head <span className="id">9f3c1a07e2…b81d44</span>
          </span>
          <span className="stamp">Matches NEAR</span>
        </div>
      </div>
      <figcaption>An investor’s view, with example figures. Who else invested, and on what terms, never reaches it.</figcaption>
    </figure>
  );
}
