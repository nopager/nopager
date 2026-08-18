import { cookies } from "next/headers";
import type { NextRequest } from "next/server";

import { readBoundedBody } from "@/lib/bounded-body";

const API_URL = process.env.NOPAGER_API_URL ?? "http://localhost:8080";
const GITHUB_API_URL = "https://api.github.com";
const MAX_BODY_BYTES = 4096;

function privateHeaders() {
  return new Headers({
    "content-type": "application/json",
    "cache-control": "private, no-store, max-age=0",
    pragma: "no-cache",
    expires: "0",
  });
}

function jsonError(error: string, status: number) {
  return Response.json({ error }, { status, headers: privateHeaders() });
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

async function localAdminAuthenticated() {
  const cookieStore = await cookies();
  const session = cookieStore.get("nopager_session");
  if (!session) return false;

  let response: Response;
  try {
    response = await fetch(new URL("/api/v1/setup/status", API_URL), {
      headers: { cookie: `nopager_session=${session.value}` },
      cache: "no-store",
    });
  } catch {
    return false;
  }
  if (!response.ok) return false;
  const status = (await response.json().catch(() => null)) as {
    authenticated?: boolean;
  } | null;
  return status?.authenticated === true;
}

export async function POST(request: NextRequest) {
  if (!sameOrigin(request)) {
    return jsonError("cross_origin_mutation_blocked", 403);
  }
  if (!(await localAdminAuthenticated())) {
    return jsonError("unauthorized", 401);
  }

  const body = await readBoundedBody(request, MAX_BODY_BYTES);
  if (!body) return jsonError("payload_too_large", 413);

  let code: string;
  try {
    const value = JSON.parse(new TextDecoder().decode(body)) as {
      code?: unknown;
    };
    if (typeof value.code !== "string") {
      return jsonError("invalid_github_manifest_code", 400);
    }
    code = value.code.trim();
  } catch {
    return jsonError("invalid_json", 400);
  }

  if (!/^[A-Za-z0-9_-]{8,256}$/.test(code)) {
    return jsonError("invalid_github_manifest_code", 400);
  }

  let response: Response;
  try {
    response = await fetch(
      `${GITHUB_API_URL}/app-manifests/${encodeURIComponent(code)}/conversions`,
      {
        method: "POST",
        headers: {
          accept: "application/vnd.github+json",
          "x-github-api-version": "2022-11-28",
          "user-agent": "NoPager-self-host",
        },
        cache: "no-store",
      },
    );
  } catch {
    return jsonError("github_manifest_exchange_unavailable", 502);
  }

  if (!response.ok) {
    return jsonError(
      response.status === 404 || response.status === 422
        ? "github_manifest_exchange_failed"
        : "github_manifest_exchange_unavailable",
      response.status === 404 || response.status === 422 ? 400 : 502,
    );
  }

  const app = (await response.json().catch(() => null)) as {
    id?: unknown;
    pem?: unknown;
    webhook_secret?: unknown;
    html_url?: unknown;
    slug?: unknown;
  } | null;

  if (
    !app ||
    typeof app.id !== "number" ||
    typeof app.pem !== "string" ||
    typeof app.webhook_secret !== "string" ||
    typeof app.html_url !== "string"
  ) {
    return jsonError("github_manifest_exchange_invalid_response", 502);
  }

  return Response.json(
    {
      appId: app.id,
      privateKey: app.pem,
      webhookSecret: app.webhook_secret,
      appUrl: app.html_url,
      slug: typeof app.slug === "string" ? app.slug : null,
    },
    { status: 201, headers: privateHeaders() },
  );
}

export const dynamic = "force-dynamic";
