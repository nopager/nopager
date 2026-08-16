"use client";

import { useRouter } from "next/navigation";
import { useState } from "react";

export function ApproveButton({ incidentId }: { incidentId: string }) {
  const router = useRouter();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  async function approve() {
    setBusy(true);
    setError("");
    const response = await fetch(
      `/api/nopager/incidents/${encodeURIComponent(incidentId)}/approve`,
      { method: "POST" },
    );
    if (response.ok) router.refresh();
    else {
      const body = (await response.json().catch(() => ({}))) as {
        error?: string;
      };
      setError(body.error?.replaceAll("_", " ") ?? "Approval failed");
    }
    setBusy(false);
  }
  return (
    <>
      <button className="primary-button wide" disabled={busy} onClick={approve}>
        {busy ? "Approving…" : "Approve production deploy"}
      </button>
      {error && <p className="form-error">{error}</p>}
    </>
  );
}

export function RejectButton({ incidentId }: { incidentId: string }) {
  const router = useRouter();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  async function reject() {
    setBusy(true);
    setError("");
    const response = await fetch(
      `/api/nopager/incidents/${encodeURIComponent(incidentId)}/reject`,
      { method: "POST" },
    );
    if (response.ok) router.refresh();
    else {
      const body = (await response.json().catch(() => ({}))) as {
        error?: string;
      };
      setError(body.error?.replaceAll("_", " ") ?? "Rejection failed");
    }
    setBusy(false);
  }
  return (
    <>
      <button
        className="secondary-button wide"
        disabled={busy}
        onClick={reject}
      >
        {busy ? "Rejecting…" : "Reject repair"}
      </button>
      {error && <p className="form-error">{error}</p>}
    </>
  );
}
