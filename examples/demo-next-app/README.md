# NoPager demo Next.js app

This is a deliberately breakable dogfood target for exercising NoPager against a real GitHub + Vercel application.

Run the healthy baseline:

```bash
pnpm --filter @nopager/demo-next-app dev
curl http://localhost:3100/api/health
```

Inject one fault at a time through Vercel project environment variables, then redeploy the commit:

| `DEMO_SCENARIO`     | Reproduction                           | Expected signal                         |
| ------------------- | -------------------------------------- | --------------------------------------- |
| unset / `healthy`   | `/api/health`                          | HTTP 200                                |
| `build-failure`     | `pnpm build`                           | deterministic build failure             |
| `health-failure`    | `/api/health`                          | HTTP 503                                |
| `recent-regression` | `/api/health` immediately after deploy | HTTP 503 correlated with the new commit |

The always-on `/api/runtime-error` route provides a deterministic runtime HTTP 500 fixture. For a source-level missing-import repair exercise, create a branch and replace the first line of `app/page.tsx` with an import from a nonexistent local module, commit it, and push the branch. Revert the injected commit or unset `DEMO_SCENARIO` to restore the known-good baseline.

Use this app only as a test target; never point real user traffic at it.
