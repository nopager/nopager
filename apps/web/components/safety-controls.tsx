"use client";

import { useState } from "react";
import { Card, SectionTitle } from "@/components/ui";

export function SafetyControls({
  initialMode,
  initialPaused,
}: {
  initialMode: string;
  initialPaused: boolean;
}) {
  const [mode, setMode] = useState(initialMode);
  const [paused, setPaused] = useState(initialPaused);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  async function mutate(path: string, body?: object) {
    setBusy(true);
    setError("");
    const response = await fetch(`/api/nopager/${path}`, {
      method: "POST",
      headers: body ? { "content-type": "application/json" } : undefined,
      body: body ? JSON.stringify(body) : undefined,
    });
    if (!response.ok) {
      const value = (await response.json().catch(() => ({}))) as {
        error?: string;
      };
      setError(value.error?.replaceAll("_", " ") ?? "Change failed");
      setBusy(false);
      return false;
    }
    setBusy(false);
    return true;
  }
  async function changeMode(next: string) {
    if (await mutate("safety/mode", { mode: next }))
      setMode(next === "autopilot" ? "autopilot_experimental" : next);
  }
  async function changePaused(next: boolean) {
    if (await mutate(next ? "protection/pause" : "protection/resume"))
      setPaused(next);
  }
  return (
    <>
      {paused && (
        <div className="notice red">
          <span>!</span>
          <div>
            <strong>Mutating actions are paused</strong>
            <p>
              Read-only monitoring continues. Repairs and deployments wait until
              protection resumes.
            </p>
          </div>
          <button
            className="primary-button"
            disabled={busy}
            onClick={() => changePaused(false)}
          >
            Resume protection
          </button>
        </div>
      )}
      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}
      <div className="safety-grid">
        <div>
          <Card>
            <SectionTitle title="Operating mode" />
            <label
              className={`mode-option ${mode === "safe" ? "selected" : ""}`}
            >
              <input
                type="radio"
                name="mode"
                checked={mode === "safe"}
                disabled={busy}
                onChange={() => changeMode("safe")}
              />
              <span>
                <strong>Safe Mode</strong>
                <small>Recommended</small>
                <p>
                  Diagnose, repair, test, and verify previews automatically. Ask
                  before production.
                </p>
              </span>
            </label>
            <label
              className={`mode-option ${mode !== "safe" ? "selected" : ""}`}
            >
              <input
                type="radio"
                name="mode"
                checked={mode !== "safe"}
                disabled={busy}
                onChange={() => changeMode("autopilot")}
              />
              <span>
                <strong>Autopilot</strong>
                <small className="experimental">Experimental</small>
                <p>
                  Allow only low-risk, verified, reversible production actions
                  without approval.
                </p>
              </span>
            </label>
          </Card>
          <Card>
            <SectionTitle
              title="Production guardrails"
              detail="Required in every mode"
            />
            <div className="guardrail-list">
              {[
                "Preview build, tests, and health verification",
                "Known-good rollback path",
                "High-risk actions always blocked",
                "Maximum 3 repair attempts",
              ].map((item) => (
                <div key={item}>
                  <span className="lock-icon">✓</span>
                  <span>
                    <strong>{item}</strong>
                  </span>
                </div>
              ))}
            </div>
          </Card>
        </div>
        <aside>
          <Card className="kill-card">
            <div className="kill-icon">!</div>
            <h2>Kill Switch</h2>
            <p>
              Pause every mutating action while read-only monitoring continues.
            </p>
            <button
              className="danger-button wide"
              disabled={paused || busy}
              onClick={() => changePaused(true)}
            >
              {paused ? "Protection is paused" : "Pause all actions"}
            </button>
            <small>This action is recorded in the audit log.</small>
          </Card>
        </aside>
      </div>
    </>
  );
}
