import { cookies } from "next/headers";
import type { NextRequest } from "next/server";

import { readBoundedBody } from "@/lib/bounded-body";

const API_URL = process.env.NOPAGER_API_URL ?? "http://localhost:8080";
const MAX_BODY_BYTES = 1024 * 1024;

function sameOriginMutation(request: NextRequest) {
  if (request.method === "GET" || request.method === "HEAD") return true;
  const origin = request.headers.get("origin");
  const host = request.headers.get("host");
  if (!origin || !host) return false;
  try {
    return new URL(origin).host === host;
  } catch {
    return false;
  }
}

async function proxy(
  request: NextRequest,
  context: { params: Promise<{ path: string[] }> },
) {
  if (!sameOriginMutation(request)) {
    return Response.json({ error: "cross_origin_mutation_blocked" }, { status: 403 });
  }

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

  let body: Uint8Array | undefined;
  if (request.method !== "GET" && request.method !== "HEAD") {
    const boundedBody = await readBoundedBody(request, MAX_BODY_BYTES);
    if (!boundedBody) {
      return Response.json({ error: "payload_too_large" }, { status: 413 });
    }
    body = boundedBody;
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
