import Link from "next/link";
import { PageHeader } from "@/components/ui";
import { SafetyControls } from "@/components/safety-controls";
import { api, type AppSettings } from "@/lib/api";

export default async function SafetyPage() {
  const settings = await api<AppSettings>("settings");
  if (!settings) {
    return (
      <div className="page">
        <PageHeader
          eyebrow="Guardrails"
          title="Safety controls unavailable"
          description="Sign in or complete setup before changing production protection policy."
          action={
            <Link className="primary-button link-button" href="/setup">
              Open setup
            </Link>
          }
        />
      </div>
    );
  }

  const mode = settings.project.safetyMode;
  const paused = settings.project.protectionPaused;
  return (
    <div className="page">
      <PageHeader
        eyebrow="Guardrails"
        title="Safety & Policy"
        description="Every production action remains verifiable, reversible, and auditable."
        action={
          <span className={paused ? "paused-pill" : "safe-pill"}>
            {paused
              ? "Protection paused"
              : mode === "safe"
                ? "Safe Mode active"
                : "Autopilot Experimental"}
          </span>
        }
      />
      <SafetyControls initialMode={mode} initialPaused={paused} />
    </div>
  );
}
