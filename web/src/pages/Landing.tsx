import { useAction, Card, Notice } from "../components/ui.tsx";
import { companyKey, holderKey } from "../lib/keys.ts";
import { enclaveIdentity, publicKeyOf, Session } from "../lib/protocol.ts";
import { relay } from "../lib/relay.ts";
import { navigate } from "../router.ts";

const FOUNDER = "founder";
const FOUNDER_SHARES = "8000000";

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
        <h1>A cap table nobody can quietly dilute</h1>
        <p>
          Each investor sees only their own position, yet can check that the total adds up and that the history is the one
          everyone else sees. The table lives in a TEE; NEAR holds the proof.
        </p>
        <button type="button" onClick={start} disabled={busy}>
          Start a demo company
        </button>
        {status}
      </section>

      <div className="grid">
        <Card title="Private by default">
          <p>Requests and replies are encrypted end to end between your browser and the enclave. The relay in between only sees ciphertext.</p>
        </Card>
        <Card title="Payments are checked">
          <p>A SAFE is accepted only if the enclave finds the exact payment arriving at the treasury on chain, and no other SAFE uses it.</p>
        </Card>
        <Card title="Rights enforce themselves">
          <p>An MFN holder adopts better later terms on their own. At an exit the payouts go out with one approval and cannot be redirected.</p>
        </Card>
      </div>

      <Notice>
        This demo runs on NEAR testnet with test tokens that have no value. Keys are created in this browser and never leave it.
      </Notice>
    </main>
  );
}
