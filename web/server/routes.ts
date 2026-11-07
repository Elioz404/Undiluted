/**
 * The relay's HTTP interface. Every route either forwards ciphertext or does
 * something the contracts only accept with the enclave's signature, so the
 * relay holds no power over anyone's position. Inputs are still checked, and
 * spending is capped, because it pays for everything from its own test funds.
 */

import { Router, type NextFunction, type Request, type Response } from "express";

import type { AnchorOutput, Anchors, PayoutInstruction, Payouts } from "./chain.ts";
import type { Config } from "./config.ts";
import { isHex } from "./encoding.ts";
import type { Enclave } from "./enclave.ts";
import { Refused } from "./near.ts";
import type { Payments } from "./payments.ts";

export interface Services {
  config: Config;
  enclave: Enclave;
  anchors: Anchors;
  payments: Payments;
  payouts: Payouts;
}

class BadRequest extends Error {}

const COMPANY_ID = /^[a-z0-9][a-z0-9_-]{1,62}$/;

function company(value: unknown): string {
  if (typeof value !== "string" || !COMPANY_ID.test(value)) {
    throw new BadRequest("company must be 2 to 63 lowercase letters, digits, - or _");
  }
  return value;
}

function micros(value: unknown, cap: bigint): bigint {
  if (typeof value !== "string" || !/^\d+$/.test(value)) {
    throw new BadRequest("amounts are whole numbers of micro units, as strings");
  }
  const amount = BigInt(value);
  if (amount === 0n || amount > cap) {
    throw new BadRequest(`amount must be between 1 and ${cap} micro units`);
  }
  return amount;
}

function anchor(value: unknown): AnchorOutput {
  const candidate = value as Partial<AnchorOutput> | undefined;
  if (
    !candidate ||
    !Number.isSafeInteger(candidate.sequence) ||
    !isHex(candidate.head, 32) ||
    !isHex(candidate.signature, 64)
  ) {
    throw new BadRequest("not an anchor");
  }
  return {
    company: company(candidate.company),
    sequence: candidate.sequence as number,
    head: candidate.head,
    signature: candidate.signature,
  };
}

function instruction(value: unknown): PayoutInstruction {
  const candidate = value as Partial<PayoutInstruction> | undefined;
  if (
    !candidate ||
    !Number.isSafeInteger(candidate.index) ||
    typeof candidate.recipient !== "string" ||
    typeof candidate.amount_micros !== "string" ||
    typeof candidate.signature !== "string"
  ) {
    throw new BadRequest("not a payout instruction");
  }
  return candidate as PayoutInstruction;
}

/** Wraps a handler so errors become JSON answers instead of crashing the server. */
function handle(work: (request: Request) => Promise<unknown>) {
  return async (request: Request, response: Response, next: NextFunction) => {
    try {
      response.json(await work(request));
    } catch (error) {
      next(error);
    }
  };
}

export function apiRoutes(services: Services): Router {
  const { config } = services;
  const router = Router();

  router.get(
    "/config",
    handle(async () => ({
      network: config.network,
      treasury: config.treasury,
      token: config.destinationAsset.replace(/^nep141:/, ""),
      anchorContract: config.anchorContract,
      distributionContract: config.distributionContract,
      maxPaymentMicros: config.maxPaymentMicros.toString(),
      maxExitMicros: config.maxExitMicros.toString(),
    })),
  );

  router.get("/identity", handle(() => services.enclave.getIdentity()));

  router.post(
    "/enclave",
    handle(async (request) => {
      const envelope = request.body?.envelope;
      if (typeof envelope !== "string" || envelope.length > 8192) {
        throw new BadRequest("envelope must be base64 text");
      }
      return services.enclave.request(envelope);
    }),
  );

  router.post(
    "/companies",
    handle(async (request) => {
      const id = company(request.body?.company);
      if (!isHex(request.body?.enclaveKey, 32)) {
        throw new BadRequest("enclaveKey must be 64 hex characters");
      }
      await services.anchors.register(id, request.body.enclaveKey);
      return { registered: id };
    }),
  );

  router.post(
    "/anchors",
    handle(async (request) => {
      await services.anchors.publish(anchor(request.body));
      return { published: true };
    }),
  );

  router.get(
    "/anchors/:company/:sequence",
    handle(async (request) => {
      const sequence = Number(request.params.sequence);
      if (!Number.isSafeInteger(sequence) || sequence < 0) {
        throw new BadRequest("sequence must be a whole number");
      }
      return { head: await services.anchors.headAt(company(request.params.company), sequence) };
    }),
  );

  router.post(
    "/payments",
    handle(async (request) => services.payments.pay(micros(request.body?.micros, config.maxPaymentMicros))),
  );

  router.post(
    "/payouts/open",
    handle(async (request) => {
      if (!isHex(request.body?.enclaveKey, 32)) {
        throw new BadRequest("enclaveKey must be 64 hex characters");
      }
      const id = company(request.body?.company);
      const total = micros(request.body?.totalMicros, config.maxExitMicros);
      await services.payouts.open(id, request.body.enclaveKey, total);
      await services.payouts.fund(id, total);
      return { opened: id };
    }),
  );

  router.post(
    "/payouts/pay",
    handle(async (request) => {
      await services.payouts.pay(company(request.body?.company), instruction(request.body?.instruction));
      return { paid: true };
    }),
  );

  router.use((error: unknown, _request: Request, response: Response, _next: NextFunction) => {
    if (error instanceof BadRequest) {
      response.status(400).json({ error: error.message });
    } else if (error instanceof Refused) {
      response.status(409).json({ error: error.message, refused: true });
    } else {
      console.error(error);
      response.status(502).json({ error: error instanceof Error ? error.message : String(error) });
    }
  });

  return router;
}
