import { describe, expect, it } from "vitest";
import {
  maskSecret,
  projectIncidentState,
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

it("never renders a complete secret", () => {
  expect(maskSecret("sk-secret-1234")).toBe("••••••••1234");
});
