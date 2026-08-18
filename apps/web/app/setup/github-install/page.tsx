"use client";

import { useEffect, useState } from "react";

export default function GitHubInstallCallbackPage() {
  const [status, setStatus] = useState("Returning to NoPager…");

  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    const installationId = params.get("installation_id");
    if (!installationId || !/^\d+$/.test(installationId) || !window.opener) {
      setStatus(
        "NoPager could not read the GitHub installation. Close this window and try again.",
      );
      return;
    }
    window.opener.postMessage(
      { type: "nopager-github-install", installationId },
      window.location.origin,
    );
    window.close();
  }, []);

  return (
    <div className="setup-page">
      <div className="setup-card">
        <p className="eyebrow">GITHUB</p>
        <h1>{status}</h1>
      </div>
    </div>
  );
}
