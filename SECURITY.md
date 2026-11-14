# Security policy

## Supported versions

AniMathIO 2.0 is a pre-release. Until 2.0 ships, only the tip of the `v2` branch is supported.
After the 2.0 swap the latest 2.x release is the only supported version. AniMathIO 1.x
(Electron) receives critical security fixes only, from the read-only `legacy` branch lineage,
until 2.0 is stable.

| Version | Supported |
| --- | --- |
| 2.x (latest release, or `v2` before 2.0) | Yes |
| 1.x | Critical fixes only |
| Older | No |

## Reporting a vulnerability

**Do not open a public issue for a vulnerability.** Report it privately through GitHub:

1. Open the [Security tab](https://github.com/AniMathIO/AniMathIO/security/advisories/new)
   of the repository and choose **Report a vulnerability** (a private security advisory).
2. Describe the problem, the affected version or commit (`animathio --version`), the steps
   to reproduce it and the impact. A minimal project file or plugin that triggers it helps.

If the form is unavailable, open a public issue that only asks for a private contact channel
and contains no details.

You can expect an acknowledgement within a few days. This is a small, volunteer-run project, so
fixes are best effort; we will keep you informed, agree on a disclosure date with you once a fix
is ready, and credit you in the advisory unless you prefer otherwise.

## Scope

Areas that matter most for 2.0 (see [`docs/plugins.md`](docs/plugins.md) and
[`docs/mcp.md`](docs/mcp.md)):

- **Plugin sandbox escapes**: wasmtime components must not exceed their fuel, memory, epoch and
  output limits or reach anything outside the host imports.
- **Untrusted project, `.amio`, media, importer (`import-v1`, `import-manim`) and subtitle
  input** that crashes, hangs, exhausts memory or writes outside its target.
- **Local control protocol and MCP sidecar**: the socket and token file must stay private to
  the current user; any way around the token is in scope.
- **Plugin component cache** integrity (the keyed MAC and private cache directory).
- **Installers and release artifacts**: tampering, missing provenance or unexpected network
  access. Installers are currently unsigned; that is a known gap, not a vulnerability.

Reports that need an already-compromised machine or a plugin you chose to install with
elevated trust are out of scope. Dependency advisories are tracked automatically
(`cargo deny`, Renovate and Dependabot security alerts), but tell us if one is exploitable.
