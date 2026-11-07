/**
 * Keys stay in this browser. Nothing here is ever sent anywhere: requests carry
 * signatures made with these keys, never the keys themselves.
 */

import { generateKey } from "./protocol.ts";

const PREFIX = "undiluted";

function storage(): Storage | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

function read(name: string): string | null {
  try {
    return storage()?.getItem(`${PREFIX}:${name}`) ?? null;
  } catch {
    return null;
  }
}

function write(name: string, value: string): void {
  try {
    storage()?.setItem(`${PREFIX}:${name}`, value);
  } catch {
    // Without storage the key lives only as long as this page.
  }
}

async function keyFor(name: string): Promise<string> {
  const existing = read(name);
  if (existing) {
    return existing;
  }
  const created = await generateKey();
  write(name, created);
  return created;
}

export const companyKey = (company: string) => keyFor(`${company}:company`);
export const holderKey = (company: string, holder: string) => keyFor(`${company}:holder:${holder}`);
export const hasCompanyKey = (company: string) => read(`${company}:company`) !== null;

export interface JoinRequest {
  holder: string;
  key: string;
}

/** Join requests from holders opened in other tabs of this browser. */
export function joinRequests(company: string): JoinRequest[] {
  try {
    return JSON.parse(read(`${company}:joins`) ?? "[]") as JoinRequest[];
  } catch {
    return [];
  }
}

export function requestToJoin(company: string, request: JoinRequest): void {
  const others = joinRequests(company).filter((existing) => existing.holder !== request.holder);
  write(`${company}:joins`, JSON.stringify([...others, request]));
}

export interface Payment {
  holder: string;
  reference: string;
  receivedMicros: string;
}

/** Payments holders made, so the company can issue against them. */
export function payments(company: string): Payment[] {
  try {
    return JSON.parse(read(`${company}:payments`) ?? "[]") as Payment[];
  } catch {
    return [];
  }
}

export function recordPayment(company: string, payment: Payment): void {
  write(`${company}:payments`, JSON.stringify([...payments(company), payment]));
}
