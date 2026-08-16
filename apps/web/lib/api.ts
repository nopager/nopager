import type { InternalIncidentState } from "@/lib/model";
import { cookies } from "next/headers";

const API_URL = process.env.NOPAGER_API_URL ?? "http://localhost:8080";

export type IncidentSummary = {
  id: string;
  projectId: string;
  title: string;
  status: InternalIncidentState;
  severity: string;
  openedAt: string;
  resolvedAt: string | null;
  triggerType?: string;
  rootCauseSummary?: string | null;
  autonomousResolution?: boolean;
  timeToRecoverySeconds?: number;
  actionRequired?: boolean;
};

export type Overview = {
  configured: boolean;
  systemStatus: string;
  actionRequired: boolean;
  protectionMode?: string;
  protectionPaused?: boolean;
  project?: { id: string; name: string };
  latestIncident?: IncidentSummary | null;
  healthCheckCount?: number;
  lastCheckedAt?: string | null;
  incidentsThisMonth?: number;
  autonomousThisMonth?: number;
  latestDeployment?: {
    id: string;
    environment: string;
    commitSha: string;
    url: string;
    status: string;
    knownGood: boolean;
    createdAt: string;
  } | null;
};

export type IncidentDetail = IncidentSummary & {
  projectName: string;
  triggerType: string;
  rootCauseSummary: string | null;
  autonomousResolution: boolean;
  safetyMode: string;
  protectionPaused: boolean;
  events: Array<{
    id: string;
    type: string;
    actor: string;
    message: string;
    metadata: unknown;
    createdAt: string;
  }>;
  currentAttempt: null | {
    id: string;
    attemptNumber: number;
    baseCommitSha: string;
    diagnosis: Record<string, unknown> | null;
    proposal: Record<string, unknown> | null;
    patchDiff: string | null;
    riskLevel: string | null;
    sandboxStatus: string;
    testStatus: string;
    validation: unknown[];
    repairBranch: string | null;
    pullRequestUrl: string | null;
    previewUrl: string | null;
    status: string;
  };
};

export type AppSettings = {
  project: {
    id: string;
    name: string;
    repoOwner: string;
    repoName: string;
    productionUrl: string;
    safetyMode: string;
    protectionPaused: boolean;
  };
  integrations: Array<{
    type: string;
    externalAccountId: string | null;
    externalProjectId: string | null;
    metadata: Record<string, unknown>;
    status: string;
  }>;
  healthChecks: Array<{
    id: string;
    url: string;
    status: string;
    lastCheckedAt: string | null;
  }>;
};

export async function api<T>(path: string): Promise<T | null> {
  try {
    const cookieStore = await cookies();
    const session = cookieStore.get("nopager_session");
    const response = await fetch(new URL(`/api/v1/${path}`, API_URL), {
      cache: "no-store",
      headers: session
        ? { cookie: `nopager_session=${session.value}` }
        : undefined,
    });
    if (!response.ok) return null;
    return (await response.json()) as T;
  } catch {
    return null;
  }
}
