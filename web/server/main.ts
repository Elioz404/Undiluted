import { existsSync } from "node:fs";
import { resolve } from "node:path";

import express from "express";
import { JsonRpcProvider } from "near-api-js";

import { Anchors, Payouts, Tokens } from "./chain.ts";
import { loadConfig, tokenContract } from "./config.ts";
import { Enclave } from "./enclave.ts";
import { CliSubmitter, KeySubmitter, Near, type Submitter } from "./near.ts";
import { Payments, Subscriptions } from "./payments.ts";
import { apiRoutes } from "./routes.ts";

const SETTLING = { checks: 20, intervalMs: 3000 };
const PAYMENT_POLLING = { pollIntervalMs: 5000, maxPolls: 40 };

const config = loadConfig(process.env);
const provider = new JsonRpcProvider({ url: config.rpcUrl });
const submitter: Submitter =
  config.signer.kind === "key"
    ? new KeySubmitter(provider, config.relayAccount, config.signer.privateKey)
    : new CliSubmitter(config.signer.path, config.relayAccount, config.network);
const near = new Near(provider, config.relayAccount, submitter, SETTLING);
const tokens = new Tokens(near);

const app = express();
app.use(express.json({ limit: "32kb" }));
app.use(
  "/api",
  apiRoutes({
    config,
    enclave: new Enclave(near, {
      contract: config.outlayerContract,
      project: config.enclaveProject,
      secretsAccount: config.secretsAccount,
      secretsProfile: config.secretsProfile,
    }),
    anchors: new Anchors(near, config.anchorContract),
    payments: new Payments(tokens, {
      api: config.paymentApi,
      originAsset: config.originAsset,
      destinationAsset: config.destinationAsset,
      originToken: tokenContract(config.originAsset),
      treasury: config.treasury,
      payer: config.relayAccount,
      ...PAYMENT_POLLING,
    }),
    subscriptions: new Subscriptions(tokens, tokenContract(config.destinationAsset), config.treasury),
    payouts: new Payouts(near, tokens, config.distributionContract, tokenContract(config.destinationAsset)),
  }),
);

const site = resolve(import.meta.dirname, "../dist");
if (existsSync(site)) {
  app.use(express.static(site));
  app.get(/^(?!\/api\/).*/, (_request, response) => response.sendFile(resolve(site, "index.html")));
}

app.listen(config.port, () => {
  console.log(`relay listening on ${config.port} as ${config.relayAccount} on ${config.network}`);
});
