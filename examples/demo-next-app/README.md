# NoPager demo Next.js app

This is a deliberately breakable dogfood target for exercising NoPager against a real GitHub + Vercel application.

## Create a disposable standalone repository

For the real-provider Alpha gate, do not connect `nopager/nopager` itself or another customer/product repository. From a NoPager checkout, use an authenticated GitHub CLI session to create a dedicated disposable repository:

```bash
gh auth status
sh scripts/create-dogfood-repo.sh YOUR_OWNER/nopager-dogfood
```

The bootstrap command refuses to overwrite an existing repository, defaults to a private repository, copies only this demo app, adds standalone ignore rules, creates one `main` commit, and pushes it. To make the disposable repository public explicitly:

```bash
NOPAGER_DOGFOOD_VISIBILITY=public \
  sh scripts/create-dogfood-repo.sh YOUR_OWNER/nopager-dogfood
```

Import that repository into a disposable Vercel project, set the Vercel Production Branch to `main`, and keep real user traffic away from it. Follow GitHub issue #55 for the evidence that must be captured before the Design Partner Alpha gate is considered proven.

## Run locally

Run the healthy baseline from the NoPager monorepo:

```bash
pnpm --filter @nopager/demo-next-app dev
curl http://localhost:3100/api/health
```

In a standalone dogfood repository, install dependencies and run the app with the package manager of your choice before connecting it to Vercel.

## Inject supported faults

Inject one fault at a time through Vercel project environment variables, then redeploy the commit:

| `DEMO_SCENARIO`     | Reproduction                            | Expected signal                         |
| ------------------- | --------------------------------------- | --------------------------------------- |
| unset / `healthy`   | `/api/health`                           | HTTP 200                                |
| `build-failure`     | `pnpm build`                            | deterministic build failure             |
| `health-failure`    | `/api/health`                           | HTTP 503                                |
| `recent-regression` | `/api/health` immediately after deploy | HTTP 503 correlated with the new commit |

The always-on `/api/runtime-error` route provides a deterministic runtime HTTP 500 fixture. For a source-level missing-import repair exercise, create a branch and replace the first line of `app/page.tsx` with an import from a nonexistent local module, commit it, and push the branch. Revert the injected commit or unset `DEMO_SCENARIO` to restore the known-good baseline.

Use this app only as a test target; never point real user traffic at it.
