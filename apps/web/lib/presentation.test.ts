import { describe, expect, it } from "vitest";
import {
  apiDate,
  planEvidence,
  safeTechnicalEvidence,
  verificationSummary,
} from "./presentation";

it("parses ISO and Rust time tuple timestamps", () => {
  expect(apiDate("2026-08-26T10:13:25Z")?.toISOString()).toBe(
    "2026-08-26T10:13:25.000Z",
  );
  expect(
    apiDate([2026, 238, 10, 13, 25, 614_000_000, 0, 0, 0])?.toISOString(),
  ).toBe("2026-08-26T10:13:25.614Z");
  expect(apiDate("not-a-date")).toBeNull();
});

describe("incident presentation", () => {
  it("extracts only bounded evidence and verification labels", () => {
    const plan = {
      evidence: [
        { source: "external_http", finding: "Three checks returned 503" },
        { source: "missing finding" },
      ],
      verificationSignals: [
        { kind: "external_http", sourceId: "primary_https_health" },
        { kind: "container_status", sourceId: "docker:abc" },
      ],
      requiredConsecutiveSuccesses: 2,
    };
    expect(planEvidence(plan)).toEqual([
      { source: "external_http", finding: "Three checks returned 503" },
    ]);
    expect(verificationSummary(plan)).toBe(
      "External HTTP + Container state · 2 consecutive successes",
    );
  });

  it("redacts secrets, private addresses, and URL query values", () => {
    expect(
      safeTechnicalEvidence({
        apiKey: "not-for-display",
        message:
          "probe 10.0.0.8 and https://user:pass@192.168.1.4/health?token=value",
      }),
    ).toEqual({
      apiKey: "[redacted]",
      message:
        "probe [private-ip] and https://redacted@private-ip/health?[redacted]",
    });
  });
});
