import { describe, expect, it } from "vitest";
import {
  maskSecret,
  projectIncidentState,
  sourceRecoveryAction,
  type InternalIncidentState,
  type UiIncidentState,
} from "./model";

const stateCases: Array<[InternalIncidentState, UiIncidentState]> = [
  ["OPEN", "OPEN"],
  ["COLLECTING_CONTEXT", "OPEN"],
  ["DIAGNOSING", "DIAGNOSING"],
  ["PLANNING", "DIAGNOSING"],
  ["REPAIRING", "REPAIRING"],
  ["TESTING", "REPAIRING"],
  ["PREVIEW_DEPLOYING", "REPAIRING"],
  ["VERIFYING_PREVIEW", "REPAIRING"],
  ["WAITING_APPROVAL", "WAITING_APPROVAL"],
  ["PRODUCTION_DEPLOYING", "REPAIRING"],
  ["VERIFYING_PRODUCTION", "REPAIRING"],
  ["ROLLING_BACK", "REPAIRING"],
  ["ROLLED_BACK", "RESOLVED"],
  ["RESOLVED", "RESOLVED"],
  ["FAILED", "HUMAN_NEEDED"],
  ["ESCALATED", "HUMAN_NEEDED"],
  ["CANCELLED", "RESOLVED"],
  ["IGNORED", "RESOLVED"],
  ["DUPLICATE", "RESOLVED"],
  ["PAUSED", "PAUSED"],
];

describe("UI state projection", () => {
  it.each(stateCases)("projects %s to %s", (internal, expected) => {
    expect(projectIncidentState(internal)).toBe(expected);
  });
});

describe("source recovery projection", () => {
  it("uses the newest source recovery event and preserves review identity", () => {
    const action = sourceRecoveryAction([
      {
        metadata: {
          actionRequired: "review_source_revert",
          sourceRevert: {
            pullRequestUrl: "https://github.com/example/app/pull/41",
            pullRequestNumber: 41,
          },
        },
      },
      { metadata: { unrelated: true } },
      {
        metadata: {
          actionRequired: "verify_existing_source_revert",
          sourceRevertCandidate: {
            pullRequestUrl: "https://github.com/example/app/pull/42",
            pullRequestNumber: 42,
          },
        },
      },
    ]);

    expect(action).toEqual({
      kind: "verify_existing_source_revert",
      pullRequestUrl: "https://github.com/example/app/pull/42",
      pullRequestNumber: 42,
    });
  });

  it.each([
    "javascript:alert(1)",
    "https://github.com.evil.test/example/app/pull/42",
    "https://user:password@github.com/example/app/pull/42",
    "https://github.com/example/app/issues/42",
    "https://github.com/example/app/pull/42?diff=split",
  ])("does not render an untrusted PR URL: %s", (pullRequestUrl) => {
    const action = sourceRecoveryAction([
      {
        metadata: {
          actionRequired: "review_source_revert",
          sourceRevert: { pullRequestUrl, pullRequestNumber: 42 },
        },
      },
    ]);

    expect(action).toEqual({
      kind: "review_source_revert",
      pullRequestUrl: null,
      pullRequestNumber: 42,
    });
  });

  it("keeps manual source recovery actionable without inventing a PR", () => {
    expect(
      sourceRecoveryAction([
        { metadata: { actionRequired: "revert_merged_repair" } },
      ]),
    ).toEqual({
      kind: "revert_merged_repair",
      pullRequestUrl: null,
      pullRequestNumber: null,
    });
  });
});

it("never renders a complete secret", () => {
  expect(maskSecret("sk-secret-1234")).toBe("••••••••1234");
});
