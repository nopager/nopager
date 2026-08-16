export type InternalIncidentState =
  | "OPEN"
  | "COLLECTING_CONTEXT"
  | "DIAGNOSING"
  | "PLANNING"
  | "REPAIRING"
  | "TESTING"
  | "PREVIEW_DEPLOYING"
  | "VERIFYING_PREVIEW"
  | "WAITING_APPROVAL"
  | "PRODUCTION_DEPLOYING"
  | "VERIFYING_PRODUCTION"
  | "ROLLING_BACK"
  | "ROLLED_BACK"
  | "RESOLVED"
  | "FAILED"
  | "ESCALATED"
  | "CANCELLED"
  | "IGNORED"
  | "DUPLICATE"
  | "PAUSED";

export type UiIncidentState =
  | "OPEN"
  | "DIAGNOSING"
  | "REPAIRING"
  | "WAITING_APPROVAL"
  | "RESOLVED"
  | "HUMAN_NEEDED"
  | "PAUSED";

export function projectIncidentState(
  state: InternalIncidentState,
): UiIncidentState {
  if (state === "OPEN" || state === "COLLECTING_CONTEXT") return "OPEN";
  if (state === "DIAGNOSING" || state === "PLANNING") return "DIAGNOSING";
  if (state === "WAITING_APPROVAL") return "WAITING_APPROVAL";
  if (state === "PAUSED") return "PAUSED";
  if (
    state === "RESOLVED" ||
    state === "ROLLED_BACK" ||
    state === "CANCELLED" ||
    state === "IGNORED" ||
    state === "DUPLICATE"
  )
    return "RESOLVED";
  if (state === "FAILED" || state === "ESCALATED") return "HUMAN_NEEDED";
  return "REPAIRING";
}

export function maskSecret(lastFour: string): string {
  return `••••••••${lastFour.slice(-4)}`;
}
