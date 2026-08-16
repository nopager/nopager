export type DemoScenario = "healthy" | "health-failure" | "recent-regression";

export function scenario(value = process.env.DEMO_SCENARIO): DemoScenario {
  if (value === "health-failure" || value === "recent-regression") {
    return value;
  }
  return "healthy";
}

export function healthStatus(value?: string) {
  const active = scenario(value);
  return {
    active,
    healthy: active === "healthy",
    status: active === "healthy" ? 200 : 503,
  } as const;
}
