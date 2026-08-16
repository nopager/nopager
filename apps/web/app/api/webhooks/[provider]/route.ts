import type { NextRequest } from "next/server";

const API_URL = process.env.NOPAGER_API_URL ?? "http://localhost:8080";

const forwardedHeaders: Record<string, readonly string[]> = {
  github: [
    "content-type",
    "x-github-delivery",
    "x-github-event",
    "x-hub-signature-256",
  ],
  vercel: ["content-type", "x-vercel-signature"],
};

export const dynamic = "force-dynamic";
export const runtime = "nodejs";

export async function POST(
  request: NextRequest,
  context: { params: Promise<{ provider: string }> },
) {
  const { provider } = await context.params;
  const allowed = forwardedHeaders[provider];
  if (!allowed) {
    return Response.json(
      { error: "unsupported_webhook_provider" },
      { status: 404 },
    );
  }

  const headers = new Headers();
  for (const name of allowed) {
    const value = request.headers.get(name);
    if (value) headers.set(name, value);
  }

  const target = new URL(`/api/v1/integrations/${provider}/webhook`, API_URL);
  try {
    const upstream = await fetch(target, {
      method: "POST",
      headers,
      body: await request.arrayBuffer(),
      cache: "no-store",
    });
    const responseHeaders = new Headers({
      "content-type": upstream.headers.get("content-type") ?? "application/json",
    });
    return new Response(upstream.body, {
      status: upstream.status,
      headers: responseHeaders,
    });
  } catch {
    return Response.json({ error: "api_unavailable" }, { status: 503 });
  }
}
