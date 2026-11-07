import { useId, useState, type ReactNode } from "react";

/** The top of a view: what this page is for, one line of context, and its own actions. */
export function PageHeader({ title, children, actions }: { title: string; children?: ReactNode; actions?: ReactNode }) {
  return (
    <header className="page-header">
      <div>
        <h1>{title}</h1>
        {children && <p>{children}</p>}
      </div>
      {actions}
    </header>
  );
}

/** Something waiting on a signature. It sits above the work because it blocks it. */
export function Pending({ title, children, actions }: { title: string; children: ReactNode; actions: ReactNode }) {
  return (
    <section className="pending" aria-live="polite">
      <h2>{title}</h2>
      {children}
      <div className="actions">{actions}</div>
    </section>
  );
}

/** The document a view is about. */
export function Sheet({ title, aside, children }: { title: string; aside?: ReactNode; children: ReactNode }) {
  const id = useId();
  return (
    <section className="sheet" aria-labelledby={id}>
      <header className="sheet-head">
        <h2 id={id}>{title}</h2>
        {aside}
      </header>
      {children}
    </section>
  );
}

export interface Tab {
  id: string;
  label: string;
  content: ReactNode;
}

/**
 * The things to do on a view, one at a time. Until someone picks a tab, the
 * first one shows, so a tab that appears once data loads can lead.
 */
export function ActionPanel({ title, tabs }: { title: string; tabs: Tab[] }) {
  const [current, setCurrent] = useState<string | null>(null);
  const shown = tabs.find((tab) => tab.id === current) ?? tabs[0];
  const id = useId();
  return (
    <aside className="panel" aria-labelledby={id}>
      <h2 id={id}>{title}</h2>
      {tabs.length > 1 && (
        <div className="groove" role="tablist">
          {tabs.map((tab) => (
            <button
              key={tab.id}
              type="button"
              role="tab"
              id={`${id}-${tab.id}`}
              aria-selected={tab.id === shown?.id}
              aria-controls={`${id}-panel`}
              onClick={() => setCurrent(tab.id)}
            >
              {tab.label}
            </button>
          ))}
        </div>
      )}
      <div className="panel-body" role="tabpanel" id={`${id}-panel`} aria-labelledby={`${id}-${shown?.id}`}>
        {shown?.content}
      </div>
    </aside>
  );
}

export function Notice({ tone = "info", children }: { tone?: "info" | "good" | "bad"; children: ReactNode }) {
  return (
    <p className={`notice notice-${tone}`} role={tone === "bad" ? "alert" : "status"}>
      {children}
    </p>
  );
}

export function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <label className="field">
      <span>{label}</span>
      {children}
      {hint && <small>{hint}</small>}
    </label>
  );
}

/** Rows in the shape of what is loading. */
export function Skeleton() {
  return (
    <div className="skeleton" aria-hidden>
      <span />
      <span />
      <span />
    </div>
  );
}

export function Empty({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="empty">
      <strong>{title}</strong>
      <p className="muted small">{children}</p>
    </div>
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
        <p className="progress" role="status">
          <span className="spinner" aria-hidden />
          <span>
            {busy}. Each step runs in the enclave and is recorded on NEAR, so it takes a few seconds.
          </span>
        </p>
      )}
      {error && (
        <Notice tone="bad">
          <strong>That did not go through.</strong> {error}
        </Notice>
      )}
    </>
  );

  return { busy: busy !== null, run, status };
}
