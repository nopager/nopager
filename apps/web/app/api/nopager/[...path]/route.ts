import { cookies } from "next/headers";
import type { NextRequest } from "next/server";

const API_URL = process.env.NOPAGER_API_URL ?? "http://localhost:8080";
const MAX_BODY_BYTES = 1024 * 1024;

async function proxy(
  request: NextRequest,
  context: { params: Promise<{ path: string[] }> },
) {
  const { path } = await context.params;
  const target = new URL(
    `/api/v1/${path.map(encodeURIComponent).join("/")}`,
    API_URL,
  );
  const headers = new Headers();
  headers.set("accept", "application/json");
  const contentType = request.headers.get("content-type");
  if (contentType) headers.set("content-type", contentType);
  const cookieStore = await cookies();
  const session = cookieStore.get("nopager_session");
  if (session) headers.set("cookie", `nopager_session=${session.value}`);

  let body: ArrayBuffer | undefined;
  if (request.method !== "GET") {
    const declaredLength = request.headers.get("content-length");
    if (
      declaredLength &&
      Number.isFinite(Number(declaredLength)) &&
      Number(declaredLength) > MAX_BODY_BYTES
    ) {
      return Response.json({ error: "payload_too_large" }, { status: 413 });
    }
    body = await request.arrayBuffer();
    if (body.byteLength > MAX_BODY_BYTES) {
      return Response.json({ error: "payload_too_large" }, { status: 413 });
    }
  }

  let upstream: Response;
  try {
    upstream = await fetch(target, {
      method: request.method,
      headers,
      body,
      cache: "no-store",
    });
  } catch {
    return Response.json({ error: "api_unavailable" }, { status: 503 });
  }
  const responseHeaders = new Headers({
    "content-type": upstream.headers.get("content-type") ?? "application/json",
  });
  const setCookie = upstream.headers.get("set-cookie");
  if (setCookie) responseHeaders.set("set-cookie", setCookie);
  return new Response(upstream.body, {
    status: upstream.status,
    headers: responseHeaders,
  });
}

export const dynamic = "force-dynamic";
export const GET = proxy;
export const POST = proxy;
