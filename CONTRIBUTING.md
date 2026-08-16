# Contributing

Use a focused branch (`feat/...` or `fix/...`) and Conventional Commits. Before opening a PR, run:

```bash
pnpm format:check
pnpm lint
pnpm typecheck
pnpm test
pnpm build
```

Add tests for every safety-sensitive primitive and never commit secrets. Keep changes within the v0.1 GitHub + Vercel scope.
