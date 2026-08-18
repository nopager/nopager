import type { NextRequest } from "next/server";

import { readBoundedBody } from "@/lib/bounded-body";

const API_URL = process.env.NOPAGER_API_URL ?? "http://localhost:8080";
const GITHUB_API_VERSION = "2022-11-28";
const MAX_BODY_BYTES = 4096;

function privateNoStoreHeaders() {
  return new Headers({
    "content-type": "application/json",
    "cache-control": "private, no-store, max-age=0",
    pragma: "no-cache",
    expires: "0",
  });
}

function jsonError(error: string, status: number) {
  return Response.json({ error }, { status, headers: privateNoStoreHeaders() });
}

function sameOrigin(request: NextRequest) {
  const origin = request.headers.get("origin");
  const host = request.headers.get("host");
  if (!origin || !host) return false;
  try {
    return new URL(origin).host === host;
  } catch {
    return false;
  }
}

async function authenticated(request: NextRequest) {
  const headers = new Headers({ accept: "application/json" });
  const cookie = request.headers.get("cookie");
  if (cookie) headers.set("cookie", cookie);
  try {
    const response = await fetch(new URL("/api/v1/setup/status", API_URL), {
      headers,
      cache: "no-store",
    });
    if (!response.ok) return false;
    const status = (await response.json()) as { authenticated?: boolean };
    return status.authenticated === true;
  } catch {
    return false;
  }
}

export async function POST(request: NextRequest) {
  if (!sameOrigin(request)) return jsonError("cross_origin_mutation_blocked", 403);
  if (!(await authenticated(request))) return jsonError("unauthorized", 401);

  const bounded = await readBoundedBody(request, MAX_BODY_BYTES);
  if (!bounded) return jsonError("payload_too_large", 413);

  let code: string;
  try {
    const body = JSON.parse(new TextDecoder().decode(bounded)) as { code?: unknown };
    if (typeof body.code !== "string" || body.code.length < 10 || body.code.length > 256) {
      return jsonError("invalid_manifest_code", 400);
    }
    code = body.code;
  } catch {
    return jsonError("invalid_json", 400);
  }

  let response: Response;
  try {
    response = await fetch(
      `https://api.github.com/app-manifests/${encodeURIComponent(code)}/conversions`,
      {
        method: "POST",
        headers: {
          accept: "application/vnd.github+json",
          "x-github-api-version": GITHUB_API_VERSION,
          "user-agent": "NoPager-setup",
        },
        cache: "no-store",
        signal: AbortSignal.timeout(15_000),
      },
    );
  } catch {
    return jsonError("github_manifest_exchange_failed", 502);
  }

  if (!response.ok) return jsonError("github_manifest_exchange_failed", 502);

  const app = (await response.json()) as {
    id?: unknown;
    slug?: unknown;
    pem?: unknown;
    webhook_secret?: unknown;
  };
  if (
    typeof app.id !== "number" ||
    typeof app.slug !== "string" ||
    typeof app.pem !== "string" ||
    typeof app.webhook_secret !== "string" ||
    app.pem.length === 0 ||
    app.webhook_secret.length < 16
  ) {
    return jsonError("github_manifest_response_invalid", 502);
  }

  return Response.json(
    {
      appId: app.id,
      slug: app.slug,
      privateKey: app.pem,
      webhookSecret: app.webhook_secret,
    },
    { headers: privateNoStoreHeaders() },
  );
}
