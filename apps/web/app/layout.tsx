import type { Metadata } from "next";
import Link from "next/link";
import "./globals.css";

export const metadata: Metadata = {
  title: "NoPager",
  description: "Your app breaks. You don't get paged.",
};

const nav = [
  ["Overview", "/", "⌂"],
  ["Incidents", "/incidents", "◉"],
  ["Integrations", "/integrations", "⌁"],
  ["AI Provider", "/ai-provider", "✦"],
  ["Safety & Policy", "/safety", "⌾"],
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
                  <span aria-hidden="true">{icon}</span>
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
