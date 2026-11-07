/**
 * The relay's HTTP interface. Every route either forwards ciphertext or does
 * something the contracts only accept with the enclave's signature, so the
 * relay holds no power over anyone's position. It does pay for everything from
 * its own test funds, so inputs are checked, each caller is rate limited, and
 * all spending shares one daily budget.
 */

import { Router, type NextFunction, type Request, type Response } from "express";

import type { AnchorOutput, Anchors, PayoutInstruction, Payouts } from "./chain.ts";
import type { Config } from "./config.ts";
import { isHex } from "./encoding.ts";
import type { Enclave } from "./enclave.ts";
import type { Budget, RateLimiter } from "./limits.ts";
import { Refused } from "./near.ts";
import { isSubscriptionMemo, type Payments, type Subscriptions } from "./payments.ts";

export interface Limits {
  /** Every call that reaches the chain or the enclave. */
  calls: RateLimiter;
  /** Calls that spend the relay's test tokens. */
  spending: RateLimiter;
  budget: Budget;
}

export interface Services {
  config: Config;
  enclave: Enclave;
  anchors: Anchors;
  payments: Payments;
  subscriptions: Subscriptions;
  payouts: Payouts;
  limits: Limits;
}

class BadRequest extends Error {}

class TooMany extends Error {}

const COMPANY_ID = /^[a-z0-9][a-z0-9_-]{1,62}$/;
const ACCOUNT_ID = /^(([a-z\d]+[-_])*[a-z\d]+\.)*([a-z\d]+[-_])*[a-z\d]+$/;
const BASE64 = /^[A-Za-z0-9+/]+={0,2}$/;
const ED25519_SIGNATURE_BASE64_LENGTH = 88;
const MAX_ENVELOPE_LENGTH = 8192;

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

function instruction(value: unknown, cap: bigint): PayoutInstruction {
  const candidate = value as Partial<PayoutInstruction> | undefined;
  if (
    !candidate ||
    !Number.isSafeInteger(candidate.index) ||
    (candidate.index as number) < 0 ||
    typeof candidate.recipient !== "string" ||
    candidate.recipient.length < 2 ||
    candidate.recipient.length > 64 ||
    !ACCOUNT_ID.test(candidate.recipient) ||
    typeof candidate.signature !== "string" ||
    candidate.signature.length !== ED25519_SIGNATURE_BASE64_LENGTH ||
    !BASE64.test(candidate.signature)
  ) {
    throw new BadRequest("not a payout instruction");
  }
  micros(candidate.amount_micros, cap);
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

function limitedBy(limiter: RateLimiter) {
  return (request: Request, _response: Response, next: NextFunction) => {
    next(limiter.allow(request.ip ?? "unknown") ? undefined : new TooMany("too many requests; try again in a minute"));
  };
}

/** Spends from the daily budget, and gives it back if the spending fails. */
async function spending<T>(budget: Budget, amount: bigint, work: () => Promise<T>): Promise<T> {
  if (!budget.reserve(amount)) {
    throw new TooMany("this demo relay has spent today's test budget; try again tomorrow");
  }
  try {
    return await work();
  } catch (error) {
    budget.release(amount);
    throw error;
  }
}

export function apiRoutes(services: Services): Router {
  const { config, limits } = services;
  const router = Router();
  const spends = [limitedBy(limits.calls), limitedBy(limits.spending)];

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

  router.get("/attestation", handle(() => services.enclave.identityAttestation()));

  router.post(
    "/enclave",
    limitedBy(limits.calls),
    handle(async (request) => {
      const envelope = request.body?.envelope;
      if (typeof envelope !== "string" || envelope.length > MAX_ENVELOPE_LENGTH || !BASE64.test(envelope)) {
        throw new BadRequest("envelope must be base64 text");
      }
      return services.enclave.request(envelope);
    }),
  );

  router.post(
    "/companies",
    limitedBy(limits.calls),
    handle(async (request) => {
      const id = company(request.body?.company);
      await services.anchors.register(id, await services.enclave.signingKey());
      return { registered: id };
    }),
  );

  router.post(
    "/anchors",
    limitedBy(limits.calls),
    handle(async (request) => {
      await services.anchors.publish(anchor(request.body));
      return { published: true };
    }),
  );

  router.post(
    "/payments",
    ...spends,
    handle(async (request) => {
      const amount = micros(request.body?.micros, config.maxPaymentMicros);
      return spending(limits.budget, amount, () => services.payments.pay(amount));
    }),
  );

  router.post(
    "/payments/subscription",
    ...spends,
    handle(async (request) => {
      const memo = request.body?.memo;
      if (!isSubscriptionMemo(memo)) {
        throw new BadRequest("memo must be undiluted:<company>:<64 hex characters>");
      }
      const amount = micros(request.body?.micros, config.maxPaymentMicros);
      return spending(limits.budget, amount, () => services.subscriptions.pay(amount, memo));
    }),
  );

  router.post(
    "/payouts/open",
    ...spends,
    handle(async (request) => {
      const id = company(request.body?.company);
      const total = micros(request.body?.totalMicros, config.maxExitMicros);
      const enclaveKey = await services.enclave.signingKey();
      await spending(limits.budget, total, async () => {
        await services.payouts.open(id, enclaveKey, total);
        await services.payouts.fund(id, total);
      });
      return { opened: id };
    }),
  );

  router.post(
    "/payouts/pay",
    limitedBy(limits.calls),
    handle(async (request) => {
      await services.payouts.pay(company(request.body?.company), instruction(request.body?.instruction, config.maxExitMicros));
      return { paid: true };
    }),
  );

  router.use((error: unknown, _request: Request, response: Response, _next: NextFunction) => {
    if (error instanceof BadRequest) {
      response.status(400).json({ error: error.message });
    } else if (error instanceof TooMany) {
      response.status(429).json({ error: error.message });
    } else if (error instanceof Refused) {
      response.status(409).json({ error: error.message, refused: true });
    } else {
      console.error(error);
      response.status(502).json({ error: "the chain or the enclave did not answer; try again" });
    }
  });

  return router;
}
