"use client";

import { useRouter } from "next/navigation";
import { useState } from "react";

export function ApprovalActions({
  incidentId,
  actionKind,
}: {
  incidentId: string;
  actionKind: string;
}) {
  const router = useRouter();
  const [intent, setIntent] = useState<"approve" | "deny" | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const restart = actionKind === "restart_container";
  const approveLabel = restart
    ? "Approve restart"
    : "Approve production deploy";

  async function submit(decision: "approve" | "reject") {
    setBusy(true);
    setError("");
    try {
      const response = await fetch(
        `/api/nopager/incidents/${encodeURIComponent(incidentId)}/${decision}`,
        { method: "POST" },
      );
      if (response.ok) {
        router.refresh();
        return;
      }
      const body = (await response.json().catch(() => ({}))) as {
        error?: string;
      };
      setError(body.error?.replaceAll("_", " ") ?? `${decision} failed`);
    } catch {
      setError(
        "The request could not reach NoPager. No decision was recorded.",
      );
    } finally {
      setBusy(false);
    }
  }

  if (intent) {
    const approving = intent === "approve";
    return (
      <div
        className="approval-confirm"
        role="alertdialog"
        aria-label={
          approving ? "Approve bounded action" : "Deny bounded action"
        }
        aria-live="polite"
      >
        <strong>
          {approving ? "Grant this bounded authority?" : "Deny this action?"}
        </strong>
        <p>
          {approving
            ? "This permits one execution attempt against the exact target above. Verification must still pass."
            : "NoPager will record the denial and will not execute this proposed action."}
        </p>
        <div className="approval-confirm-actions">
          <button
            type="button"
            className={approving ? "primary-button" : "secondary-button"}
            disabled={busy}
            onClick={() => submit(approving ? "approve" : "reject")}
          >
            {busy ? "Recording…" : approving ? approveLabel : "Deny action"}
          </button>
          <button
            type="button"
            className="quiet-button"
            disabled={busy}
            onClick={() => setIntent(null)}
          >
            Cancel
          </button>
        </div>
        {error ? (
          <p className="form-error" role="alert">
            {error}
          </p>
        ) : null}
      </div>
    );
  }

  return (
    <div className="approval-actions">
      <button
        type="button"
        className="primary-button"
        onClick={() => setIntent("approve")}
      >
        {approveLabel}
      </button>
      <button
        type="button"
        className="secondary-button"
        onClick={() => setIntent("deny")}
      >
        Deny
      </button>
      <button
        type="button"
        className="quiet-button"
        disabled
        title="Manual escalation is not exposed by this Alpha control plane"
      >
        Escalate
      </button>
      <small>
        Escalation is automatic on unsafe or unverifiable outcomes; manual
        escalation is not available from this Alpha console.
      </small>
    </div>
  );
}
