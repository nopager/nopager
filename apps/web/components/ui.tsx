import Link from "next/link";
import type { ReactNode } from "react";
import type { UiIncidentState } from "@/lib/model";

export function PageHeader({
  eyebrow,
  title,
  description,
  action,
}: {
  eyebrow?: string;
  title: string;
  description: string;
  action?: ReactNode;
}) {
  return (
    <header className="page-header">
      <div>
        {eyebrow && <p className="eyebrow">{eyebrow}</p>}
        <h1>{title}</h1>
        <p>{description}</p>
      </div>
      {action}
    </header>
  );
}

export function Card({
  children,
  className = "",
}: {
  children: ReactNode;
  className?: string;
}) {
  return <section className={`card ${className}`}>{children}</section>;
}

export function StatusBadge({ state }: { state: UiIncidentState }) {
  const label = state.replaceAll("_", " ");
  return (
    <span className={`status-badge status-${state.toLowerCase()}`}>
      {label}
    </span>
  );
}

export function SectionTitle({
  title,
  detail,
}: {
  title: string;
  detail?: string;
}) {
  return (
    <div className="section-title">
      <h2>{title}</h2>
      {detail && <span>{detail}</span>}
    </div>
  );
}

export function IncidentLink({
  id,
  children,
}: {
  id: string;
  children: ReactNode;
}) {
  return (
    <Link className="text-link" href={`/incidents/${id}`}>
      {children} <span aria-hidden="true">→</span>
    </Link>
  );
}
