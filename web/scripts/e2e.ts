/**
 * Walks the web interface's flow against a running relay, with the same wasm
 * protocol and the same code the pages use, printing every answer.
 *
 *   RELAY_URL=http://localhost:8080 npx tsx scripts/e2e.ts
 *
 * It spends test funds through the relay and takes a few minutes, because every
 * step is a real enclave run on chain.
 */

import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { initSync } from "../src/wasm/undiluted_web.js";
import { enclaveIdentity, generateKey, publicKeyOf, Session } from "../src/lib/protocol.ts";
import { relay, useRelayAt } from "../src/lib/relay.ts";
import type { Actor, Event, Response } from "../src/lib/types.ts";

const PAYMENT_MICROS = "1000000";

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

  const company = `e2e-${Date.now().toString(36)}`;
  const session = new Session(company);
  const config = await relay.config();
  const enclave = await enclaveIdentity();
  const asCompany = { actor: { role: "company" } as Actor, secret: await generateKey() };
  const ana = { actor: { role: "holder", holder: "ana" } as Actor, secret: await generateKey() };

  step(`Create ${company}`);
  expect(await session.send(asCompany.actor, { kind: "create_company" }, asCompany.secret), "company_created");
  await relay.registerCompany(company, enclave.signing_key);

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

  console.log("\nDone.");
}

main().catch((error) => {
  console.error(`\nFAILED: ${error instanceof Error ? error.message : String(error)}`);
  process.exit(1);
});
