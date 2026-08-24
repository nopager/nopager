"use client";

import { useRouter } from "next/navigation";
import { useState } from "react";

type ApprovalKind = "deployment" | "operation";

export function ApproveButton({
  incidentId,
  kind = "deployment",
}: {
  incidentId: string;
  kind?: ApprovalKind;
}) {
  const router = useRouter();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const operation = kind === "operation";

  async function approve() {
    if (
      !window.confirm(
        operation
          ? "Approve this bounded production operation? NoPager will execute only the persisted configured action, then independently verify production health."
          : "Promote this verified repair to production? NoPager will verify production health and roll back if verification fails.",
      )
    ) {
      return;
    }
    setBusy(true);
    setError("");
    try {
      const response = await fetch(
        `/api/nopager/incidents/${encodeURIComponent(incidentId)}/approve`,
        { method: "POST" },
      );
      if (response.ok) {
        router.refresh();
        return;
      }
      const body = (await response.json().catch(() => ({}))) as {
        error?: string;
      };
      setError(body.error?.replaceAll("_", " ") ?? "Approval failed");
    } catch {
      setError("Approval request could not reach NoPager.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <button className="primary-button wide" disabled={busy} onClick={approve}>
        {busy
          ? "Approving…"
          : operation
            ? "Approve operation"
            : "Approve production deploy"}
      </button>
      {error && <p className="form-error">{error}</p>}
    </>
  );
}

export function RejectButton({
  incidentId,
  kind = "deployment",
}: {
  incidentId: string;
  kind?: ApprovalKind;
}) {
  const router = useRouter();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const operation = kind === "operation";

  async function reject() {
    setBusy(true);
    setError("");
    try {
      const response = await fetch(
        `/api/nopager/incidents/${encodeURIComponent(incidentId)}/reject`,
        { method: "POST" },
      );
      if (response.ok) {
        router.refresh();
        return;
      }
      const body = (await response.json().catch(() => ({}))) as {
        error?: string;
      };
      setError(body.error?.replaceAll("_", " ") ?? "Rejection failed");
    } catch {
      setError("Rejection request could not reach NoPager.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <button
        className="secondary-button wide"
        disabled={busy}
        onClick={reject}
      >
        {busy ? "Rejecting…" : operation ? "Reject operation" : "Reject repair"}
      </button>
      {error && <p className="form-error">{error}</p>}
    </>
  );
}
