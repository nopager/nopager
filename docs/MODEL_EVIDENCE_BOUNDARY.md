# Model evidence boundary and BYOK accounting

## What can leave the machine

For the production-operations `restart_container` path, each incident-planning request contains only:

- the bounded incident title/summary;
- the deterministic trigger and health-check context;
- the enrolled target ID and helper-reported container state;
- bounded deployment context already attached to the incident, if any;
- the exact typed actions currently available (`observe_only`, `escalate`, and only when gated, `restart_container` for the enrolled ID);
- the exact available verification signal identifiers and descriptions;
- NoPager's fixed system instruction and strict JSON response schema.

The configured provider receives this through its normal API endpoint: OpenAI Responses API, Anthropic Messages API, or Gemini `generateContent`. Provider/model identity is selected during local setup. NoPager does not proxy these requests through a NoPager-operated model account.

The separate deployment-recovery subsystem may additionally send bounded commit messages, file paths, verified relevant text diffs, stack traces, deployment metadata, health evidence, diagnosis context, and validation failures. It does not serialize the whole repository.

## Redaction and bounding

Immediately before an operations request, NoPager recursively bounds JSON to depth 8, 64 object keys, 32 array elements, and 12,000 characters per string. It redacts values whose key indicates passwords, secrets, API/private keys, authorization, cookies, tokens, database URLs, connection strings, or DSNs. It also redacts PEM private-key blocks, credential-like assignments, URL user/password components, and recognized GitHub/GitLab/Slack/OpenAI/Google/AWS token prefixes.

Redaction is best-effort deterministic filtering, not a proof that arbitrary sensitive prose cannot pass. IP addresses, container IDs/names, URLs without embedded credentials, service names, timestamps, ordinary log text, and arbitrary numbers are not blanket-redacted because they can be operational evidence. Design partners must point health checks at endpoints whose response/context is safe to share with the chosen provider.

## Never intentionally sent

The operations model request does not include the helper IPC credential, Docker socket, Docker API credentials, arbitrary Docker inspection JSON, host files, `.env`, `NOPAGER_MASTER_KEY`, provider API key, database connection string/content, browser session cookie, administrator password, arbitrary environment variables, complete logs, arbitrary account inventories, or whole repository.

The provider key itself is necessarily transmitted as HTTP authentication to the chosen provider. TLS, provider retention, abuse monitoring, residency, and training terms depend on the operator's provider/account agreement; NoPager cannot cryptographically override them.

## BYOK behavior

The provider key is submitted to the local setup API, encrypted using `NOPAGER_MASTER_KEY`, and stored in local PostgreSQL. The operations quickstart removes duplicate provider-key values from worker `.env` after setup. The worker decrypts the key only when making a provider request. Removing the integration/provider key revokes future AI reasoning but does not delete evidence already retained by the external provider under its terms.

## Request-level usage accounting

Every operations-planning call creates one `model.operations_plan` audit event, including failures. It records provider ID, request count `1`, input byte count before final provider-boundary redaction, an approximate input-token estimate (`ceil(bytes/4)`), outcome, and on success the validated output byte count/approximate token estimate and decision. These estimates are useful for incident-level request/cost correlation but are not provider billing truth: tokenization, fixed prompts, response schemas, redaction, retries below the HTTP layer, caching, and provider accounting can differ. Use the provider's usage dashboard/invoice for authoritative billing.

NoPager currently makes one operations-planning API request per incident planning job. A separate fresh incident or resumed workflow can create another auditable request; mutation ambiguity never causes an automatic model-driven restart replay.
