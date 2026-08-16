import { PageHeader } from "@/components/ui";
import { SafetyControls } from "@/components/safety-controls";
import { api, type AppSettings } from "@/lib/api";

export default async function SafetyPage() {
  const settings = await api<AppSettings>("settings");
  const mode = settings?.project.safetyMode ?? "safe";
  const paused = settings?.project.protectionPaused ?? false;
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
