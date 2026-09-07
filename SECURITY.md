# Security Policy

## Supported versions

Anole has not reached a supported public release. Security fixes currently target the main development branch.

## Reporting a vulnerability

Until a private repository advisory channel is configured, do not publish exploit details. Contact the project maintainers privately and include:

- Anole version or commit.
- Operating system and architecture.
- Engine name, version, and build configuration.
- Minimal reproduction steps.
- Whether a crafted file is required.
- Expected impact.

Do not attach sensitive user files. Prefer a synthetic reproducer.

## Security boundaries

The main application treats conversion engines and input files as potentially unsafe. The current development builds do not yet claim a complete OS sandbox. Release claims must match the controls proven by docs/security/THREAT_MODEL.md and the release checklist.

### Document passwords (encrypted PDF conversion)

Cleartext passwords are execution-only: they live in process memory and in a
single-use, plan-keyed secret store; serialized Plans, reports, logs, and the
SQLite job store carry only a `[redacted]` marker. Two residual exposures are
accepted and disclosed rather than claimed away: (1) Poppler engines accept
passwords only as command-line arguments, so the cleartext is visible in the
engine process's argv for the duration of the conversion (on Windows this is
readable by other same-user processes, the same exposure an environment
variable would have); (2) passwords are not wiped with `zeroize` and may
remain in freed memory until reused. Crash dumps are not produced by default;
if you enable them, assume captured memory can contain the password. A
durably queued encrypted-PDF plan never carries its password across process
restarts — it fails closed with a "password unavailable" error instead.

