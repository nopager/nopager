import type { NextRequest } from "next/server";

const API_URL = process.env.NOPAGER_API_URL ?? "http://localhost:8080";
const MAX_BODY_BYTES = 1024 * 1024;

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

  const declaredLength = request.headers.get("content-length");
  if (
    declaredLength &&
    Number.isFinite(Number(declaredLength)) &&
    Number(declaredLength) > MAX_BODY_BYTES
  ) {
    return Response.json({ error: "payload_too_large" }, { status: 413 });
  }

  const body = await request.arrayBuffer();
  if (body.byteLength > MAX_BODY_BYTES) {
    return Response.json({ error: "payload_too_large" }, { status: 413 });
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
      body,
      cache: "no-store",
    });
    const responseHeaders = new Headers({
      "content-type":
        upstream.headers.get("content-type") ?? "application/json",
    });
    return new Response(upstream.body, {
      status: upstream.status,
      headers: responseHeaders,
    });
  } catch {
    return Response.json({ error: "api_unavailable" }, { status: 503 });
  }
}
