import { describe, expect, it } from "vitest";
import { maskSecret, projectIncidentState } from "./model";

describe("UI state projection", () => {
  it("collapses technical repair states", () => {
    expect(projectIncidentState("PREVIEW_VERIFYING")).toBe("REPAIRING");
    expect(projectIncidentState("ROLLING_BACK")).toBe("REPAIRING");
  });

  it("surfaces escalation as human needed", () => {
    expect(projectIncidentState("ESCALATED")).toBe("HUMAN_NEEDED");
  });
});

it("never renders a complete secret", () => {
  expect(maskSecret("sk-secret-1234")).toBe("••••••••1234");
});
