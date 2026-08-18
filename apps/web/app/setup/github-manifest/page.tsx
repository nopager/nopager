"use client";

import { useEffect, useState } from "react";

export default function GitHubManifestCallbackPage() {
  const [status, setStatus] = useState("Returning to NoPager…");

  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    const code = params.get("code");
    const state = params.get("state");
    if (!code || !state || !window.opener) {
      setStatus("NoPager could not complete the GitHub App registration. Close this window and try again.");
      return;
    }
    window.opener.postMessage(
      { type: "nopager-github-manifest", code, state },
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
