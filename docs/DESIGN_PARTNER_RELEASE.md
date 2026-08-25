# Design Partner production-operations release

## Validation status

The repository test suite and Linux/Docker privilege-boundary smoke test pass for this source milestone. A complete acceptance record from a tester-provided real VPS, real BYOK provider, and public HTTPS recovery target does **not** exist yet. This build is therefore not externally validated and must not be represented as having passed Design Partner real-host acceptance. NoPager does not provide or require a vendor-operated VPS for this self-hosted OSS Alpha; each tester supplies the disposable host and BYOK credentials, then follows the [Design Partner Acceptance Test](DESIGN_PARTNER_ACCEPTANCE.md).

## Tested version

The release identifier is `0.2.0-design-partner.1`, stored in [`DESIGN_PARTNER_VERSION`](../DESIGN_PARTNER_VERSION). A design partner must use an annotated source commit/tag containing that exact file and record `git rev-parse HEAD` in the acceptance record. Do not describe an uncommitted working tree as a tested release. The acceptance script prints and records both values; the exact commit becomes the release identifier alongside the version.

## Required platform

- Dedicated disposable or staging Linux VPS; Ubuntu 24.04 LTS is the reference platform.
- x86_64 or arm64, systemd, Linux Unix sockets and `SO_PEERCRED`.
- Docker Engine 26 or newer with the standard local Unix socket at `/var/run/docker.sock`.
- Docker Compose v2, Git, curl, Python 3, and Rust/Cargo 1.92.0 for building the helper.
- A public HTTPS health URL checking meaningful application behavior.
- One target container on the same daemon, outside the NoPager Compose project.
- One BYOK OpenAI, Anthropic, or Gemini key and an exact structured-output-capable model ID.

Do not begin on a shared multi-tenant host, a business-critical database host, the only production node, or a container that cannot safely tolerate one restart. Put the disposable VPS in a separate cloud account/project or security group where possible; restrict SSH, do not mount host secrets into the target, keep backups elsewhere, and start in Safe Mode.

## Install

```sh
git clone <pinned-design-partner-repository-url> nopager
cd nopager
git checkout <tested-commit-or-tag>
test "$(cat DESIGN_PARTNER_VERSION)" = "0.2.0-design-partner.1"
rustup override set 1.92.0
sh scripts/operations-quickstart.sh
```

The quickstart builds and installs `nopager-runtime-helper`, enrolls the target's immutable ID, creates the dedicated IPC group/token, starts the hardened systemd service, then recreates the worker without Docker authority. Inspect the generated `/etc/nopager/runtime-helper.json`, systemd unit, Compose rendering, helper journal directory, and [security architecture](RUNTIME_HELPER_SECURITY.md) before approval.

Run the full [Design Partner Acceptance Test](DESIGN_PARTNER_ACCEPTANCE.md) on the disposable environment. The release is accepted only when the record contains the exact version/commit, platform versions, happy path, Kill Switch proof, boundary smoke proof, and all automated failure/ambiguity tests.

## Uninstall and revoke

First enable the Kill Switch and disable restart capability. Then:

```sh
curl -X POST -H "authorization: Bearer $NOPAGER_ADMIN_TOKEN" \
  http://127.0.0.1:8080/api/v1/protection/pause
sudo sh scripts/uninstall-runtime-helper.sh
docker compose down
```

Remove `NOPAGER_RUNTIME_HELPER_TOKEN`, `NOPAGER_DOCKER_TARGET`, and the provider integration/key; rotate the provider key and `NOPAGER_MASTER_KEY` after suspected compromise. The uninstaller deliberately preserves `/var/lib/nopager-runtime/requests` as audit evidence. After review, a host administrator may archive and remove it. Remove the helper user from the Docker group or delete the disposable VPS to fully revoke host authority.

## Maintenance expectation

Pin the exact commit, monitor security advisories, patch Linux/Docker/NoPager, and rerun acceptance after any change. Stop the helper when the test is not active. This Alpha has no broad server-operations promise and no paid availability SLA.
