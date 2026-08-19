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

export type SourceRecoveryAction = {
  kind:
    | "review_source_revert"
    | "verify_existing_source_revert"
    | "create_or_verify_source_revert"
    | "revert_merged_repair";
  pullRequestUrl: string | null;
  pullRequestNumber: number | null;
};

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

export function sourceRecoveryAction(
  events: ReadonlyArray<{ metadata: unknown }>,
): SourceRecoveryAction | null {
  for (let index = events.length - 1; index >= 0; index -= 1) {
    const metadata = recordValue(events[index]?.metadata);
    if (!metadata) continue;
    const kind = sourceRecoveryKind(metadata.actionRequired);
    if (!kind) continue;

    const revert =
      kind === "review_source_revert"
        ? recordValue(metadata.sourceRevert)
        : kind === "verify_existing_source_revert"
          ? recordValue(metadata.sourceRevertCandidate)
          : null;
    const pullRequestNumber = positiveInteger(revert?.pullRequestNumber);

    return {
      kind,
      pullRequestUrl: safeGithubPullRequestUrl(revert?.pullRequestUrl),
      pullRequestNumber,
    };
  }
  return null;
}

export function maskSecret(lastFour: string): string {
  return `••••••••${lastFour.slice(-4)}`;
}

function sourceRecoveryKind(
  value: unknown,
): SourceRecoveryAction["kind"] | null {
  switch (value) {
    case "review_source_revert":
    case "verify_existing_source_revert":
    case "create_or_verify_source_revert":
    case "revert_merged_repair":
      return value;
    default:
      return null;
  }
}

function recordValue(value: unknown): Record<string, unknown> | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  return value as Record<string, unknown>;
}

function positiveInteger(value: unknown): number | null {
  return typeof value === "number" && Number.isSafeInteger(value) && value > 0
    ? value
    : null;
}

function safeGithubPullRequestUrl(value: unknown): string | null {
  if (typeof value !== "string") return null;
  try {
    const url = new URL(value);
    if (
      url.protocol !== "https:" ||
      url.hostname !== "github.com" ||
      url.port !== "" ||
      url.username !== "" ||
      url.password !== "" ||
      url.search !== "" ||
      url.hash !== "" ||
      !/^\/[^/]+\/[^/]+\/pull\/[1-9]\d*\/?$/.test(url.pathname)
    ) {
      return null;
    }
    return value;
  } catch {
    return null;
  }
}
