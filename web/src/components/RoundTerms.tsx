import { describeTerms, dollars } from "../lib/format.ts";
import type { Mandate } from "../lib/types.ts";

/** The open round's terms, as anyone may read them. */
export function RoundTerms({ mandate }: { mandate: Mandate }) {
  return (
    <dl className="terms">
      <div>
        <dt>Terms</dt>
        <dd>{describeTerms(mandate.terms)}</dd>
      </div>
      <div>
        <dt>Checks from</dt>
        <dd className="fig">{dollars(mandate.min_check_micros)}</dd>
      </div>
      <div>
        <dt>Round size</dt>
        <dd className="fig">{dollars(mandate.max_raise_micros)}</dd>
      </div>
    </dl>
  );
}
