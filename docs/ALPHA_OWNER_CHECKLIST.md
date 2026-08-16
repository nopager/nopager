# First design partner owner checklist

Before connecting an external production app:

- Use Safe Mode.
- Use a disposable/staging-equivalent app for the first run when possible.
- Confirm a latest known-good Vercel deployment exists.
- Confirm the health URL is public HTTPS and represents business health, not only process liveness.
- Confirm GitHub/Vercel credentials are scoped to the intended repository/project.
- Confirm the Kill Switch works from the dashboard.
- Keep the owner present for the first incident approval.
- Do not enable Autopilot for the first external run.
