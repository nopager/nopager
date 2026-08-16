import { describe, expect, it } from "vitest";
import { healthStatus, scenario } from "./scenario";

describe("demo fault injection", () => {
  it("is healthy by default", () => {
    expect(healthStatus(undefined)).toEqual({
      active: "healthy",
      healthy: true,
      status: 200,
    });
  });

  it("turns health and recent regressions into a stable 503", () => {
    expect(healthStatus("health-failure").status).toBe(503);
    expect(healthStatus("recent-regression").status).toBe(503);
  });

  it("does not let unknown values activate a fault", () => {
    expect(scenario("unknown")).toBe("healthy");
  });
});
