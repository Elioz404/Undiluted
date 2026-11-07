import { useState, type ReactNode } from "react";

export function Card({ title, children, aside }: { title: string; children: ReactNode; aside?: ReactNode }) {
  return (
    <section className="card">
      <header className="card-header">
        <h2>{title}</h2>
        {aside}
      </header>
      {children}
    </section>
  );
}

export function Notice({ tone = "info", children }: { tone?: "info" | "good" | "bad"; children: ReactNode }) {
  return <p className={`notice notice-${tone}`}>{children}</p>;
}

export function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <label className="field">
      <span>{label}</span>
      {children}
    </label>
  );
}

/**
 * Runs one slow action at a time and shows its progress. Every call goes
 * through a real enclave run on chain, so each takes several seconds.
 */
export function useAction() {
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function run<T>(label: string, work: () => Promise<T>): Promise<T | null> {
    setBusy(label);
    setError(null);
    try {
      return await work();
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
      return null;
    } finally {
      setBusy(null);
    }
  }

  const status = (
    <>
      {busy && (
        <p className="notice notice-info">
          <span className="spinner" aria-hidden /> {busy}… each step runs in the enclave on chain and takes a few seconds.
        </p>
      )}
      {error && <Notice tone="bad">{error}</Notice>}
    </>
  );

  return { busy: busy !== null, run, status };
}
