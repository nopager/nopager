import { NextResponse } from "next/server";
import { healthStatus } from "@/lib/scenario";

export function GET() {
  const result = healthStatus();
  return NextResponse.json(
    {
      status: result.healthy ? "ok" : "unavailable",
      scenario: result.active,
      service: "nopager-demo-next-app",
    },
    { status: result.status },
  );
}
