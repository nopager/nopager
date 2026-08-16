import { NextResponse } from "next/server";

export function GET() {
  return NextResponse.json(
    {
      error: "checkout_service_unavailable",
      fixture: "runtime-500",
    },
    { status: 500 },
  );
}
