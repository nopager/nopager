import type { IncidentDetail } from "@/lib/api";

type EvidenceItem = { source: string; finding: string };
type VerificationSignal = { kind: string; sourceId: string };

const SENSITIVE_KEY =
  /(?:authorization|cookie|password|secret|token|api[_-]?key|private[_-]?key|credential)/i;
const PRIVATE_IPV4 =
  /\b(?:10(?:\.\d{1,3}){3}|127(?:\.\d{1,3}){3}|169\.254(?:\.\d{1,3}){2}|192\.168(?:\.\d{1,3}){2}|172\.(?:1[6-9]|2\d|3[01])(?:\.\d{1,3}){2})\b/g;
const PRIVATE_IPV6 = /\b(?:fc|fd|fe80):[0-9a-f:]+|::1\b/gi;

export function humanize(value: string) {
  return value
    .replaceAll("_", " ")
    .replaceAll("-", " ")
    .replace(/\b\w/g, (letter) => letter.toUpperCase());
}

export function apiDate(value: unknown): Date | null {
  if (typeof value === "string" || typeof value === "number") {
    const date = new Date(value);
    return Number.isNaN(date.getTime()) ? null : date;
  }
  if (!Array.isArray(value) || value.length < 6) return null;
  const parts = value.slice(0, 9).map(Number);
  if (parts.some((part) => !Number.isFinite(part))) return null;
  const [
    year,
    ordinal,
    hour,
    minute,
    second,
    nanosecond,
    offsetHour = 0,
    offsetMinute = 0,
    offsetSecond = 0,
  ] = parts;
  if (
    !Number.isInteger(year) ||
    !Number.isInteger(ordinal) ||
    ordinal < 1 ||
    ordinal > 366
  )
    return null;
  const offset =
    Math.sign(offsetHour || offsetMinute || offsetSecond) *
    (Math.abs(offsetHour) * 3600 +
      Math.abs(offsetMinute) * 60 +
      Math.abs(offsetSecond));
  const timestamp =
    Date.UTC(year, 0, ordinal, hour, minute, second, nanosecond / 1_000_000) -
    offset * 1000;
  const date = new Date(timestamp);
  return Number.isNaN(date.getTime()) ? null : date;
}

export function planString(
  plan: Record<string, unknown> | null | undefined,
  ...keys: string[]
) {
  for (const key of keys) {
    const value = plan?.[key];
    if (typeof value === "string" && value.trim()) return value.trim();
    if (typeof value === "number" && Number.isFinite(value))
      return String(value);
  }
  return null;
}

export function planNumber(
  plan: Record<string, unknown> | null | undefined,
  ...keys: string[]
) {
  for (const key of keys) {
    const value = plan?.[key];
    if (typeof value === "number" && Number.isFinite(value)) return value;
  }
  return null;
}

export function planEvidence(
  plan: Record<string, unknown> | null | undefined,
): EvidenceItem[] {
  const value = plan?.evidence;
  if (!Array.isArray(value)) return [];
  return value.flatMap((item) => {
    const record = asRecord(item);
    const source = record && planString(record, "source");
    const finding = record && planString(record, "finding");
    return source && finding ? [{ source, finding }] : [];
  });
}

export function verificationSummary(
  plan: Record<string, unknown> | null | undefined,
) {
  const signals = verificationSignals(plan);
  const labels = signals.map((signal) => {
    if (signal.kind === "external_http") return "External HTTP";
    if (signal.kind === "container_status") return "Container state";
    return humanize(signal.kind);
  });
  const unique = [...new Set(labels)];
  const successes = planNumber(
    plan,
    "requiredConsecutiveSuccesses",
    "required_consecutive_successes",
  );
  if (unique.length === 0) return "Not recorded";
  return `${unique.join(" + ")}${successes ? ` · ${successes} consecutive successes` : ""}`;
}

export function approvalState(
  operation: NonNullable<IncidentDetail["currentOperation"]>,
) {
  if (operation.status === "APPROVAL_REQUIRED") return "Awaiting administrator";
  if (operation.policyDecision === "approved") return "Approved";
  if (
    operation.policyDecision === "rejected" ||
    operation.status === "REJECTED"
  )
    return "Denied";
  if (operation.policyDecision === "require_approval")
    return "Approval required";
  return humanize(operation.status);
}

export function safeTechnicalEvidence(value: unknown): unknown {
  return sanitize(value, 0);
}

function verificationSignals(
  plan: Record<string, unknown> | null | undefined,
): VerificationSignal[] {
  const value = plan?.verificationSignals ?? plan?.verification_signals;
  if (!Array.isArray(value)) return [];
  return value.flatMap((item) => {
    const record = asRecord(item);
    const kind = record && planString(record, "kind");
    const sourceId = record && planString(record, "sourceId", "source_id");
    return kind && sourceId ? [{ kind, sourceId }] : [];
  });
}

function sanitize(value: unknown, depth: number): unknown {
  if (depth > 8) return "[bounded]";
  if (typeof value === "string") return sanitizeString(value);
  if (Array.isArray(value))
    return value.slice(0, 100).map((item) => sanitize(item, depth + 1));
  const record = asRecord(value);
  if (!record) return value;
  return Object.fromEntries(
    Object.entries(record)
      .slice(0, 100)
      .map(([key, item]) => [
        key,
        SENSITIVE_KEY.test(key) ? "[redacted]" : sanitize(item, depth + 1),
      ]),
  );
}

function sanitizeString(value: string) {
  const withoutUnsafeUrls = value.replace(
    /https?:\/\/[^\s"'<>]+/gi,
    (candidate) => sanitizeUrl(candidate),
  );
  return withoutUnsafeUrls
    .replace(PRIVATE_IPV4, "[private-ip]")
    .replace(PRIVATE_IPV6, "[private-ip]");
}

function sanitizeUrl(value: string) {
  try {
    const url = new URL(value);
    const hostname = url.hostname.replaceAll("[", "").replaceAll("]", "");
    const privateHost =
      PRIVATE_IPV4.test(hostname) || PRIVATE_IPV6.test(hostname);
    PRIVATE_IPV4.lastIndex = 0;
    PRIVATE_IPV6.lastIndex = 0;
    const authority = `${url.username || url.password ? "redacted@" : ""}${privateHost ? "private-ip" : url.hostname}${url.port ? `:${url.port}` : ""}`;
    return `${url.protocol}//${authority}${url.pathname}${url.search ? "?[redacted]" : ""}`;
  } catch {
    return "[redacted-url]";
  }
}

function asRecord(value: unknown): Record<string, unknown> | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  return value as Record<string, unknown>;
}
