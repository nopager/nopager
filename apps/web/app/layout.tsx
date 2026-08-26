import type { Metadata } from "next";
import Link from "next/link";
import "./globals.css";

export const metadata: Metadata = {
  title: "NoPager",
  description: "Your app breaks. You don't get paged.",
};

const nav = [
  ["Overview", "/", "overview"],
  ["Incidents", "/incidents", "incidents"],
  ["Integrations", "/integrations", "integrations"],
  ["AI Provider", "/ai-provider", "provider"],
  ["Safety & Policy", "/safety", "safety"],
] as const;

export default function RootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return (
    <html lang="en">
      <body>
        <div className="app-shell">
          <aside className="sidebar">
            <Link href="/" className="brand" aria-label="NoPager overview">
              <span className="brand-mark">N</span>
              <span>NoPager</span>
            </Link>
            <nav aria-label="Primary navigation">
              {nav.map(([label, href, icon]) => (
                <Link key={href} href={href} className="nav-link">
                  <NavIcon name={icon} />
                  {label}
                </Link>
              ))}
            </nav>
            <div className="sidebar-bottom">
              <div className="protection-chip">
                <span className="pulse" /> Protection console
              </div>
              <div className="workspace-row">
                <span className="avatar">NP</span>
                <span>
                  Local workspace<small>Administrator</small>
                </span>
              </div>
              <div className="legal-links">
                <a href="https://github.com/nopager/nopager">Source</a>
                <a href="https://github.com/nopager/nopager/blob/main/LICENSE">
                  AGPL-3.0
                </a>
              </div>
            </div>
          </aside>
          <main>{children}</main>
        </div>
      </body>
    </html>
  );
}

function NavIcon({ name }: { name: (typeof nav)[number][2] }) {
  const path = {
    overview: "M4 5.5h16v13H4z M8 9.5h2v5H8z M14 7.5h2v7h-2z",
    incidents:
      "M12 3.5a8.5 8.5 0 1 0 0 17 8.5 8.5 0 0 0 0-17z M12 8v4.5 M12 16h.01",
    integrations:
      "M8.5 8.5 6 6a2.1 2.1 0 0 0-3 3l3 3a2.1 2.1 0 0 0 3 0l1-1 M15.5 15.5 18 18a2.1 2.1 0 0 0 3-3l-3-3a2.1 2.1 0 0 0-3 0l-1 1 M8.5 15.5l7-7",
    provider: "M12 3l1.6 5.4L19 10l-5.4 1.6L12 17l-1.6-5.4L5 10l5.4-1.6z",
    safety:
      "M12 3l7 3v5c0 4.6-2.8 7.8-7 10-4.2-2.2-7-5.4-7-10V6z M9.5 12l1.7 1.7 3.6-4",
  }[name];
  return (
    <svg
      aria-hidden="true"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.6"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d={path} />
    </svg>
  );
}
