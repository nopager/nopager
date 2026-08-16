export type InternalIncidentState =
  | "OPEN"
  | "COLLECTING_CONTEXT"
  | "DIAGNOSING"
  | "PLANNING"
  | "REPAIRING"
  | "TESTING"
  | "PREVIEW_DEPLOYING"
  | "PREVIEW_VERIFYING"
  | "WAITING_APPROVAL"
  | "PRODUCTION_DEPLOYING"
  | "PRODUCTION_VERIFYING"
  | "WATCHING"
  | "ROLLING_BACK"
  | "RESOLVED"
  | "FAILED"
  | "ESCALATED"
  | "CANCELLED";

export type UiIncidentState =
  | "OPEN"
  | "DIAGNOSING"
  | "REPAIRING"
  | "WAITING_APPROVAL"
  | "RESOLVED"
  | "HUMAN_NEEDED";

export function projectIncidentState(
  state: InternalIncidentState,
): UiIncidentState {
  if (state === "OPEN" || state === "COLLECTING_CONTEXT") return "OPEN";
  if (state === "DIAGNOSING" || state === "PLANNING") return "DIAGNOSING";
  if (state === "WAITING_APPROVAL") return "WAITING_APPROVAL";
  if (state === "RESOLVED" || state === "CANCELLED") return "RESOLVED";
  if (state === "FAILED" || state === "ESCALATED") return "HUMAN_NEEDED";
  return "REPAIRING";
}

export function maskSecret(lastFour: string): string {
  return `••••••••${lastFour.slice(-4)}`;
}
