/**
 * Test payments through a 1Click-compatible API: quote, deposit to the quoted
 * address, wait for it to settle. In this demo the relay pays from its own test
 * balance on an investor's behalf; the enclave still checks the payment itself.
 */

import type { Tokens } from "./chain.ts";

const QUOTE_VALIDITY_MS = 60 * 60 * 1000;
const SLIPPAGE_BASIS_POINTS = 100;

export interface PaymentSettings {
  api: string;
  originAsset: string;
  destinationAsset: string;
  originToken: string;
  treasury: string;
  payer: string;
  pollIntervalMs: number;
  maxPolls: number;
}

export interface Settlement {
  reference: string;
  receivedMicros: string;
}

interface StatusAnswer {
  status?: string;
  swapDetails?: {
    amountOut?: string;
    destinationChainTxHashes?: { hash?: string }[];
  };
}

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

/** Reads a status answer: settled, still going, or failed for good. */
export function settlementFrom(answer: StatusAnswer): Settlement | null {
  if (answer.status === "REFUNDED" || answer.status === "FAILED") {
    throw new Error(`the payment ended as ${answer.status}`);
  }
  if (answer.status !== "SUCCESS") {
    return null;
  }
  const receivedMicros = answer.swapDetails?.amountOut;
  const reference = answer.swapDetails?.destinationChainTxHashes?.[0]?.hash;
  if (!receivedMicros || !reference) {
    throw new Error("the payment settled without saying where or how much");
  }
  return { reference, receivedMicros };
}

/** What a subscription memo looks like: `undiluted:<company>:<holder key>`. */
const SUBSCRIPTION_MEMO = /^undiluted:[a-z0-9][a-z0-9_-]{1,62}:[0-9a-f]{64}$/;

export function isSubscriptionMemo(memo: unknown): memo is string {
  return typeof memo === "string" && SUBSCRIPTION_MEMO.test(memo);
}

/**
 * Pays the treasury directly in its own token, with a memo that names the
 * investor's key. A subscription needs that memo, and a cross-chain route
 * cannot attach one, so the payment stays on NEAR.
 */
export class Subscriptions {
  constructor(
    private readonly tokens: Tokens,
    private readonly token: string,
    private readonly treasury: string,
  ) {}

  async pay(micros: bigint, memo: string): Promise<Settlement> {
    const reference = await this.tokens.transfer(this.token, this.treasury, micros, memo);
    return { reference, receivedMicros: micros.toString() };
  }
}

export class Payments {
  constructor(
    private readonly tokens: Tokens,
    private readonly settings: PaymentSettings,
  ) {}

  async pay(micros: bigint): Promise<Settlement> {
    const quote = await this.post("/v0/quote", {
      dry: false,
      swapType: "EXACT_INPUT",
      slippageTolerance: SLIPPAGE_BASIS_POINTS,
      originAsset: this.settings.originAsset,
      depositType: "ORIGIN_CHAIN",
      destinationAsset: this.settings.destinationAsset,
      amount: micros.toString(),
      refundTo: this.settings.payer,
      refundType: "ORIGIN_CHAIN",
      recipient: this.settings.treasury,
      recipientType: "DESTINATION_CHAIN",
      deadline: new Date(Date.now() + QUOTE_VALIDITY_MS).toISOString(),
    });
    const address = (quote as { quote?: { depositAddress?: string } }).quote?.depositAddress;
    if (!address) {
      throw new Error("the quote has no deposit address");
    }

    await this.tokens.register(this.settings.originToken, address);
    const transaction = await this.tokens.transfer(this.settings.originToken, address, micros);
    await this.post("/v0/deposit/submit", { txHash: transaction, depositAddress: address });
    return this.waitForSettlement(address);
  }

  private async waitForSettlement(address: string): Promise<Settlement> {
    for (let poll = 0; poll < this.settings.maxPolls; poll += 1) {
      const answer = (await this.get(`/v0/status?depositAddress=${encodeURIComponent(address)}`)) as StatusAnswer;
      const settlement = settlementFrom(answer);
      if (settlement) {
        return settlement;
      }
      await sleep(this.settings.pollIntervalMs);
    }
    throw new Error(`the payment to ${address} did not settle in time`);
  }

  private async post(path: string, body: unknown): Promise<unknown> {
    const response = await fetch(`${this.settings.api}${path}`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    });
    return this.json(response);
  }

  private async get(path: string): Promise<unknown> {
    return this.json(await fetch(`${this.settings.api}${path}`));
  }

  private async json(response: Response): Promise<unknown> {
    const body = await response.json();
    if (!response.ok) {
      throw new Error(`the payment API answered ${response.status}: ${JSON.stringify(body)}`);
    }
    return body;
  }
}
