# ADR 0001: Rust-first implementation

Status: Accepted  
Date: 2026-08-15

## Context

The PRD originally proposes a TypeScript modular monolith. The project directive is to use Rust wherever practical while preserving the product and frontend design baselines.

## Decision

- Keep Next.js and React for the web console because the PRD fixes that frontend architecture and its six core views.
- Implement the API server, background worker, CLI, domain state machine, policy engine, cryptography, monitoring, provider abstraction, webhook verification, database queue, and sandbox command policy in Rust.
- Keep one public monorepo with a Cargo workspace alongside the pnpm/Turborepo web workspace.
- Preserve provider and deployment connector traits so Rust does not couple the product to one model or hosting platform.

## Consequences

- Safety-sensitive code benefits from explicit types, exhaustive state handling, bounded concurrency, and no garbage-collector pauses.
- Contributors need both Rust 1.92 and Node.js 22.
- Docker images use separate Rust and Node build stages.
- The frontend consumes stable read models rather than internal Rust states, preserving the PRD's high-signal UI projection.
