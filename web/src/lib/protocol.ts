/**
 * Talks to the enclave. Signing, encryption and the check that a reply really
 * comes from the enclave all happen in the wasm build of the Rust client.
 */

import init, { newParty, openReply, partyPublicKey, prepareRequest } from "../wasm/undiluted_web.js";
import { relay, RelayError } from "./relay.ts";
import type { Actor, Anchor, Command, EnclaveOutput, Event, Response } from "./types.ts";

const ATTEMPTS = 3;

let loading: Promise<unknown> | null = null;
export const ready = () => (loading ??= init());

export type Identity = Extract<EnclaveOutput, { status: "identity" }>;

let identity: Promise<Identity> | null = null;
export function enclaveIdentity(): Promise<Identity> {
  identity ??= relay.identity().then((output) => {
    if (output.status !== "identity") {
      throw new Error("the enclave did not return its identity");
    }
    return output;
  });
  return identity;
}

export async function generateKey(): Promise<string> {
  await ready();
  return newParty();
}

export async function publicKeyOf(secret: string): Promise<string> {
  await ready();
  return partyPublicKey(secret);
}

/** Heads are published in order, so one that failed waits here for the next try. */
const pendingAnchors: Anchor[] = [];

async function publishAnchors(): Promise<void> {
  while (pendingAnchors.length > 0) {
    const anchor = pendingAnchors[0]!;
    try {
      await relay.publishAnchor(anchor);
    } catch (error) {
      // A head that is already there was published by an earlier attempt.
      const published = await relay.headAt(anchor.company, anchor.sequence).catch(() => null);
      if (published?.head !== anchor.head) {
        throw error;
      }
    }
    pendingAnchors.shift();
  }
}

export class Session {
  constructor(readonly company: string) {}

  /**
   * Sends one command. The same sealed request is resent on a failure on the
   * way, which the enclave answers with its stored reply instead of acting twice.
   */
  async send(actor: Actor, command: Command, secret: string): Promise<Response> {
    await ready();
    const enclave = await enclaveIdentity();
    const identityJson = JSON.stringify(enclave);
    const prepared = prepareRequest(identityJson, this.company, JSON.stringify(actor), JSON.stringify(command), secret);
    const { envelope } = JSON.parse(prepared) as { envelope: string };

    let output: EnclaveOutput | null = null;
    let lastError: unknown = null;
    for (let attempt = 0; attempt < ATTEMPTS && output === null; attempt += 1) {
      try {
        output = await relay.enclave(envelope);
      } catch (error) {
        if (error instanceof RelayError && error.refused) {
          throw error;
        }
        lastError = error;
      }
    }
    if (output === null) {
      throw lastError ?? new Error("the enclave could not be reached");
    }
    if (output.status === "error") {
      throw new Error(output.message);
    }
    if (output.status !== "reply") {
      throw new Error("the enclave answered something unexpected");
    }

    if (output.anchor) {
      pendingAnchors.push(output.anchor);
      await publishAnchors();
    }
    return JSON.parse(openReply(identityJson, prepared, output.envelope)) as Response;
  }

  /** Proposes `event` as `proposer`, then has each signer sign it. */
  async commit(
    proposer: { actor: Actor; secret: string },
    event: Event,
    signers: { actor: Actor; secret: string }[],
  ): Promise<Response> {
    const proposal = await this.send(proposer.actor, { kind: "propose", event }, proposer.secret);
    if (proposal.kind !== "proposal") {
      return proposal;
    }
    let last: Response = proposal;
    for (const signer of signers) {
      last = await this.send(signer.actor, { kind: "sign", digest: proposal.digest }, signer.secret);
      if (last.kind === "rejected") {
        return last;
      }
    }
    return last;
  }
}
