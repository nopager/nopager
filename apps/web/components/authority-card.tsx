import Link from "next/link";
import { ApprovalActions } from "@/components/approve-button";
import type { IncidentDetail } from "@/lib/api";
import {
  approvalState,
  humanize,
  planNumber,
  verificationSummary,
} from "@/lib/presentation";

export function AuthorityCard({
  incident,
  compact = false,
}: {
  incident: IncidentDetail;
  compact?: boolean;
}) {
  const operation = incident.currentOperation;
  if (!operation) return null;
  const timeout = planNumber(
    operation.plan,
    "timeoutSeconds",
    "timeout_seconds",
  );
  const waiting = incident.status === "WAITING_APPROVAL";

  return (
    <section
      className={`authority-card${compact ? " authority-card-compact" : ""}`}
    >
      <div className="authority-card-head">
        <div className="authority-capability">
          <p className="eyebrow">Authority boundary</p>
          <h2>One action. One target. One attempt.</h2>
        </div>
        <span
          className={waiting ? "authority-state waiting" : "authority-state"}
        >
          {approvalState(operation)}
        </span>
      </div>

      <div className="authority-action">
        <span>Proposed action</span>
        <strong>{humanize(operation.actionKind)}</strong>
        <code>{operation.actionKind}</code>
      </div>

      <dl className="authority-fields">
        <div className="authority-target">
          <dt>Exact target</dt>
          <dd>
            <code>{operation.targetId ?? "No target recorded"}</code>
          </dd>
        </div>
        <div className="authority-capability">
          <dt>Capability</dt>
          <dd>Restart this enrolled container only</dd>
        </div>
        <div className="authority-policy">
          <dt>Policy decision</dt>
          <dd>{humanize(operation.policyDecision)}</dd>
        </div>
        <div className="authority-attempts">
          <dt>Attempt budget</dt>
          <dd>1 execution attempt</dd>
        </div>
        {timeout && timeout > 0 ? (
          <div className="authority-timeout">
            <dt>Verification timeout</dt>
            <dd>{timeout} seconds</dd>
          </div>
        ) : null}
        <div className="authority-scope">
          <dt>Scope / blast radius</dt>
          <dd>One enrolled container; no shell or arbitrary Docker request</dd>
        </div>
        <div className="authority-approval">
          <dt>Approval state</dt>
          <dd>{approvalState(operation)}</dd>
        </div>
        <div className="authority-verification">
          <dt>Verification</dt>
          <dd>{verificationSummary(operation.plan)}</dd>
        </div>
        <div className="authority-failure">
          <dt>Failure path</dt>
          <dd>Stop and escalate; never blindly retry an ambiguous restart</dd>
        </div>
      </dl>

      {waiting ? (
        <ApprovalActions
          incidentId={incident.id}
          actionKind={operation.actionKind}
        />
      ) : (
        <p className="authority-note">
          This record describes authority granted to the persisted action, not
          general access granted to the AI.
        </p>
      )}

      {!compact ? (
        <Link
          className="receipt-link"
          href={`/incidents/${incident.id}/receipt`}
        >
          Open authority receipt <span aria-hidden="true">→</span>
        </Link>
      ) : null}
    </section>
  );
}
