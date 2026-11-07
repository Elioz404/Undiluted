/**
 * Walks the web interface's flow against a running relay, with the same wasm
 * protocol and the same code the pages use, printing every answer.
 *
 *   RELAY_URL=http://localhost:8080 npx tsx --env-file=.env.local scripts/e2e.ts
 *
 * The VITE_* settings in .env.local tell it which enclave to trust and where
 * to read heads from, as they do for the site.
 *
 * It spends test funds through the relay and takes a few minutes, because every
 * step is a real enclave run on chain.
 */

import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { initSync } from "../src/wasm/undiluted_web.js";
import { useNetwork } from "../src/lib/network.ts";
import { generateKey, memoFor, publicKeyOf, Session } from "../src/lib/protocol.ts";
import { relay, useRelayAt } from "../src/lib/relay.ts";
import type { Actor, Event, Response } from "../src/lib/types.ts";

const PAYMENT_MICROS = "1000000";
const EXIT_MICROS = "2000000";

function step(title: string) {
  console.log(`\n> ${title}`);
}

function expect(response: Response, kind: Response["kind"]): Response {
  console.log(`  ${JSON.stringify(response).slice(0, 220)}`);
  if (response.kind !== kind) {
    throw new Error(`expected ${kind}, got ${response.kind}`);
  }
  return response;
}

async function main() {
  initSync({ module: readFileSync(resolve(import.meta.dirname, "../src/wasm/undiluted_web_bg.wasm")) });
  useRelayAt(process.env.RELAY_URL ?? "http://localhost:8080");
  useNetwork({
    rpcUrl: process.env.VITE_NEAR_RPC_URL,
    anchorContract: process.env.VITE_ANCHOR_CONTRACT,
    enclaveSigningKey: process.env.VITE_ENCLAVE_SIGNING_KEY,
  });

  const company = `e2e-${Date.now().toString(36)}`;
  const session = new Session(company);
  const config = await relay.config();
  const asCompany = { actor: { role: "company" } as Actor, secret: await generateKey() };
  const ana = { actor: { role: "holder", holder: "ana" } as Actor, secret: await generateKey() };

  step(`Create ${company}`);
  expect(await session.send(asCompany.actor, { kind: "create_company" }, asCompany.secret), "company_created");
  await relay.registerCompany(company);

  step("Name the treasury");
  const treasury: Event = { kind: "treasury_designated", account: config.treasury, token: config.token };
  expect(await session.commit(asCompany, treasury, [asCompany]), "committed");

  step("Invite ana, who reviews and signs from her own key");
  const invitation: Event = { kind: "holder_registered", holder: "ana", key: await publicKeyOf(ana.secret) };
  expect(await session.commit(asCompany, invitation, [asCompany]), "proposal");
  expect(await session.send(ana.actor, { kind: "get_proposal" }, ana.secret), "proposal");
  expect(await session.send(ana.actor, { kind: "get_statement" }, ana.secret), "rejected");
  const pending = await session.send(asCompany.actor, { kind: "get_proposal" }, asCompany.secret);
  if (pending.kind !== "proposal") throw new Error("the invitation is not pending");
  expect(await session.send(ana.actor, { kind: "sign", digest: pending.digest }, ana.secret), "committed");
  const table = expect(await session.send(asCompany.actor, { kind: "get_cap_table" }, asCompany.secret), "cap_table");
  if (table.kind !== "cap_table" || !table.holders.includes("ana")) {
    throw new Error("the company does not see ana as registered");
  }

  step("Common stock for ana, so there is a company to own");
  const stock: Event = { kind: "common_issued", instrument: "stock-ana", holder: "ana", shares: "8000000" };
  expect(await session.commit(asCompany, stock, [asCompany, ana]), "committed");

  step("ana pays, and the company issues her SAFE against that payment");
  const payment = await relay.pay(PAYMENT_MICROS);
  console.log(`  ${JSON.stringify(payment)}`);
  const safe: Event = {
    kind: "safe_issued",
    instrument: "safe-ana",
    holder: "ana",
    purchase_micros: payment.receivedMicros,
    valuation_cap_micros: null,
    discount_basis_points: null,
    mfn: true,
    settlement: payment.reference,
  };
  const proposed = expect(await session.commit(asCompany, safe, [asCompany]), "proposal");
  if (proposed.kind !== "proposal") return;
  expect(await session.send(ana.actor, { kind: "sign", digest: proposed.digest }, ana.secret), "committed");

  step("ana's statement");
  expect(await session.send(ana.actor, { kind: "get_statement" }, ana.secret), "statement");

  step("The company opens a round once; from here it approves nothing");
  const mandate = {
    terms: { valuation_cap_micros: "30000000", discount_basis_points: null, mfn: false },
    min_check_micros: "1000000",
    max_raise_micros: "5000000",
  };
  expect(await session.commit(asCompany, { kind: "safe_round_opened", mandate }, [asCompany]), "committed");
  const ben = { actor: { role: "holder", holder: "ben" } as Actor, secret: await generateKey() };
  expect(await session.send(ben.actor, { kind: "get_round" }, ben.secret), "round");

  step("ben, never invited, pays with a memo naming his key");
  const paid = await relay.paySubscription(PAYMENT_MICROS, await memoFor(company, ben.secret));
  console.log(`  ${JSON.stringify(paid)}`);
  const subscription = (holder: string, key: string): Event => ({
    kind: "safe_subscribed",
    instrument: `round-${holder}`,
    holder,
    key,
    purchase_micros: paid.receivedMicros,
    settlement: paid.reference,
  });

  step("Someone watching the treasury tries to claim ben's payment");
  const eve = { actor: { role: "holder", holder: "eve" } as Actor, secret: await generateKey() };
  const stolen = subscription("eve", await publicKeyOf(eve.secret));
  expect(await session.send(eve.actor, { kind: "propose", event: stolen }, eve.secret), "rejected");

  step("ben subscribes and signs; the enclave records it on its own");
  const own = subscription("ben", await publicKeyOf(ben.secret));
  expect(await session.commit(ben, own, [ben]), "committed");
  const after = expect(await session.send(asCompany.actor, { kind: "get_cap_table" }, asCompany.secret), "cap_table");
  if (after.kind !== "cap_table" || !after.holders.includes("ben")) {
    throw new Error("ben is not on the cap table");
  }
  expect(await session.send(ben.actor, { kind: "get_statement" }, ben.secret), "statement");

  step("Each holder chooses where to be paid, and the company is sold");
  for (const [holder, destination] of [
    [ana, process.env.ANA_ACCOUNT ?? config.treasury],
    [ben, process.env.BEN_ACCOUNT ?? config.treasury],
  ] as const) {
    const name = holder.actor.role === "holder" ? holder.actor.holder : "";
    const chosen: Event = { kind: "payout_destination_set", holder: name, destination };
    expect(await session.commit(holder, chosen, [holder]), "committed");
  }
  expect(await session.commit(asCompany, { kind: "exit_approved", proceeds_micros: EXIT_MICROS }, [asCompany]), "committed");
  const plan = expect(await session.send(asCompany.actor, { kind: "get_payout_plan" }, asCompany.secret), "payout_plan");
  if (plan.kind !== "payout_plan") return;

  step("The relay funds its own distribution and relays the enclave's payouts");
  console.log(`  ${JSON.stringify(await relay.openPayouts(company, plan.proceeds_micros))}`);
  for (const instruction of plan.instructions) {
    console.log(`  ${instruction.amount_micros} to ${instruction.recipient}: ${JSON.stringify(await relay.payOut(company, instruction))}`);
  }

  step("A payment the enclave did not sign is refused before the relay spends anything");
  const [first] = plan.instructions;
  if (first) {
    const forged = { ...first, amount_micros: String(BigInt(first.amount_micros) + 1n) };
    const refusal = await relay.payOut(company, forged).then(
      () => "paid",
      (error: unknown) => (error instanceof Error ? error.message : String(error)),
    );
    console.log(`  ${refusal}`);
    if (refusal === "paid") throw new Error("a forged payout was relayed");
  }

  console.log("\nDone.");
}

main().catch((error) => {
  console.error(`\nFAILED: ${error instanceof Error ? error.message : String(error)}`);
  process.exit(1);
});
