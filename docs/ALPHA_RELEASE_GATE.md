# Alpha release gate

Do not merge this branch into a design-partner release until all of the following are true:

- CI passes on the branch/PR.
- Policy tests prove failed Preview verification cannot be promoted.
- Runtime 500 dogfood passes end to end.
- Health-check regression dogfood passes end to end.
- Vercel build-failure dogfood passes end to end.
- Safe Mode stops at production approval.
- Production verification failure triggers rollback.
- Rollback verification failure escalates and stops mutations.
- A fresh technical user can complete setup without developer intervention beyond documented credential prerequisites.
- A real 60–90 second demo has been recorded from an actual successful run.
