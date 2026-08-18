# Code privacy and model boundary

NoPager's privacy goal is **minimum necessary incident context**, not "upload the repository and trust the model provider."

The v0.1 Alpha is self-hosted and BYOK. The trusted worker downloads the protected repository into a local incident workspace so it can apply and validate repairs. The complete repository is **not** serialized as a model prompt. Diagnosis currently sends bounded incident evidence such as recent GitHub diff context, deployment/health metadata, and an available stack trace. Repair uses the verified diff evidence preserved from diagnosis.

## Before a model request leaves the host

Every structured model input passes through a deterministic redaction boundary in `nopager-providers`.

The boundary currently:

- replaces values under secret-bearing JSON keys such as passwords, API keys, private keys, authorization fields, cookies, connection strings, DSNs, and access/refresh tokens;
- removes PEM private-key blocks from free-form evidence;
- redacts common secret assignments and authorization headers in logs/diffs;
- redacts common provider/token prefixes such as GitHub, OpenAI-style, Slack, Google API, GitLab, and AWS access-key forms;
- removes username/password userinfo from URLs before they are sent to a model;
- redacts verified GitHub diff evidence **before** that evidence is persisted into the diagnosis used by the repair stage.

Redaction placeholders deliberately preserve surrounding evidence so the model can still reason about the failure without receiving the credential value.

## What still leaves the host

Using an external model provider means some incident evidence leaves the NoPager host. In the Alpha that can include bounded code diffs, file paths, commit messages, stack traces, deployment metadata, health-check evidence, and the model-facing diagnosis/repair conversation after redaction.

Therefore the correct security statement is:

> NoPager does not send the whole repository to the model. It sends bounded incident evidence and applies a local secret-redaction boundary first.

This is materially different from claiming that no source code ever leaves the machine.

## BYOK and provider policy

The Alpha uses the operator's OpenAI, Anthropic, or Gemini API key. Provider retention/training terms are controlled by the selected provider/account and are **not** a cryptographic property of NoPager. Operators with strict requirements should configure the strongest data-retention controls available from their provider and review the provider agreement applicable to their account.

NoPager must not advertise "zero retention" unless the configured provider/account actually guarantees it.

## Local trust boundary

The self-hosted machine, trusted worker, PostgreSQL database, Docker daemon, and `NOPAGER_MASTER_KEY` remain inside the operator trust boundary. Integration credentials are encrypted before PostgreSQL persistence. Repair containers do not receive provider/service credentials or the Docker socket.

The local worker necessarily sees repository plaintext while preparing and validating a repair. Protect the NoPager host as production-adjacent infrastructure.

## Confidential inference

Hardware-backed confidential inference (for example a remotely attested TEE/confidential GPU deployment) is a future high-assurance mode, not an Alpha feature. The same applies to fully local/air-gapped model execution.

Do not describe the current Alpha as TEE-backed, air-gapped, or cryptographically unable to expose model input to an external provider.

## Product principle

**The model does not need your repository. It needs the evidence.**

Future context selection should continue moving toward smaller, causally relevant evidence sets while preserving the ability to reproduce, test, preview, verify, and roll back repairs locally.
