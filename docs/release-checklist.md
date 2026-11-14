# 2.0 release checklist

What is left between the `v2` branch and a published AniMathIO 2.0, grouped by who has to
act. The plan is in [issue #156](https://github.com/AniMathIO/AniMathIO/issues/156)
(milestone M5, "Repository and branch plan"); this page is the operational list.

State was checked on **2026-11-14** against the repository settings (`gh api`) and the
tree. "Automatable" means a script, `gh` command or agent can do it; "Manual" needs an
account owner, a payment or a judgement call.

**Owners:** *Maintainer* is the repository owner (account, billing and settings
access); *Agent* is a coding agent working through a PR; *Bot* is Renovate or release-plz
once enabled.

## 1. Rehearse before touching anything

| Step | Owner | Automatable | State |
| --- | --- | --- | --- |
| Run `scripts/swap-rehearsal.sh --v2 HEAD` on the final `v2` commit. It performs the swap in a throwaway clone (never pushes to a real remote) and verifies the result. | Agent / Maintainer | Yes | Passes with 0 failures; its WARN lines are the wording changes in section 4 |
| Run `scripts/check-docs.sh --require-binary` (CI does this) so the docs match the code at the swap commit | Agent | Yes (CI) | Wired into `ci.yml` |
| First release PR dry run: `git cliff` over the v2 range gives a clean `2.0.0` changelog (38 entries, grouped Security/Added/Fixed, no chore/ci noise), and `release-plz update` proposes `2.0.0 -> 2.1.0` for a `feat`. `scripts/swap-rehearsal.sh` runs both when `git-cliff` and `release-plz` are on `PATH` (`cargo install --locked git-cliff release-plz`). | Agent | Yes | Done (2026-11-14). **Finding:** with `publish = false` the release PR workflow has been a silent no-op (`{"prs":[]}`) because release-plz needs git-only mode and a baseline tag, see below |

## 2. Repository settings (Maintainer only)

| Step | Owner | Automatable | State (2026-11-14) |
| --- | --- | --- | --- |
| Install the **Renovate** GitHub app on the repo. Renovate reads its config from the default branch, so it only starts working at the swap, when `v2` becomes `main`. | Maintainer | No (app install needs an owner) | Not installed |
| Enable **Allow auto-merge** (Settings, General, Pull Requests). `renovate.json` uses `platformAutomerge`. | Maintainer | Yes: `gh api -X PATCH repos/AniMathIO/AniMathIO -F allow_auto_merge=true` (needs admin) | `allow_auto_merge: false` |
| Add a **ruleset for `main`** (after the swap) that requires the CI checks to pass. **Without required checks, auto-merge merges immediately and ignores CI.** Today neither `v2` nor `main` has branch protection or a ruleset. | Maintainer | Yes via `gh api repos/AniMathIO/AniMathIO/rulesets` | None exist |
| Required checks: `Rust CI / check (ubuntu-latest)` and `Rust CI / check (windows-latest)`. | Maintainer | Yes (same ruleset) | Job names are stable since #178 |
| Required macOS check once **#197** lands. `package.yml` runs the macOS dmg job with `continue-on-error: true` until the smoke test passes; remove that line in the same PR that makes the check required. Note: `package.yml` has path filters, so it cannot be a required check unless it also runs on every PR (or a macOS job is added to `ci.yml`). | Agent (workflow change), Maintainer (ruleset) | Yes | #197 open |
| **Release PRs need a token that triggers workflows.** PRs opened with the default `GITHUB_TOKEN` do not start other workflows, so `ci.yml` and `package.yml` would never run on release-plz PRs. Create a fine-grained PAT (or a GitHub App) with contents and pull-request write access, store it as the repo secret `RELEASE_PLZ_TOKEN` and use it in `release-plz.yml` instead of `GITHUB_TOKEN`. | Maintainer (secret), Agent (workflow) | Partly: the secret is manual | No repo secrets exist yet |
| "Allow GitHub Actions to create and approve pull requests" | Maintainer | Yes (`gh api repos/AniMathIO/AniMathIO/actions/permissions/workflow`) | Already enabled |
| Protect **`legacy`** as read-only (block pushes, force pushes and deletion; ruleset or `lock_branch`) right after it is created | Maintainer | Yes (ruleset API) | Branch does not exist yet (the rehearsal creates it in the throwaway clone) |
| Delete `staging` and every `release-please--branches--*` branch | Maintainer | Yes: `git push origin --delete ...` (rehearsed) | Pending the swap |
| Scheduled workflows (`security-audit.yml`, the weekly rot check) and Renovate only run from the default branch | n/a | n/a | They start working at the swap; nothing to do |

## 3. Release engineering decisions

| Decision or step | Owner | Automatable | State |
| --- | --- | --- | --- |
| **Enable releases in release-plz:** on the single `[[package]]` block flip `git_tag_enable` and `git_release_enable` to `true` and uncomment `git_only = true` (packages with `publish = false` only take part in a release in git-only mode; **without a `v2.*` baseline tag `git_only` makes release-plz error, so enable it together with the tag**), and uncomment the `release` job in `release-plz.yml`. Rehearsed by `scripts/swap-rehearsal.sh`. Keep the workspace-level switches `false` so internal crates get no tags. Point the `[[package]]` block at `animathio` if the desktop app should own the tag instead of `animathio-cli`. | Agent | Yes (rehearsed) | Do in the swap PR |
| **crates.io:** every crate has `publish = false` and release-plz is configured the same way. Decide whether the plugin SDK (`animathio-plugin-api`, promised in #156 as "published to crates.io") ships at 2.0. Recommended: not at 2.0, publish after the API freeze so the name and semver are real. If yes: reserve the name, add `CARGO_REGISTRY_TOKEN`, set `publish = true` for that package only and add a manifest `description`/`readme`. | Maintainer (decision, token) | Yes after the decision | Undecided |
| **Plugin API freeze at `@1.0.0`:** WIT, SDK example and first-party manifests target 1.0.0. The host adapts frozen 0.2.0/0.2.1/0.2.2 components; `docs/plugins.md` defines semver and guarantees legacy support through AniMathIO 2.1 inclusive. | Agent | Yes: compatibility/conformance tests | Implemented |
| **Windows code signing** (SmartScreen warning on the NSIS installer). Open question 5 in #156: worth paying for? Options: an OV/EV certificate, Azure Trusted Signing, or ship unsigned with a note. | Maintainer | Signing step is scriptable once a certificate exists | Unsigned |
| **macOS signing and notarization:** the `.dmg` is unsigned and not notarized, so Gatekeeper blocks it by default. Needs an Apple Developer ID ($99/year), `cargo-packager` signing identity and `notarytool` secrets. Until then ship it as "best effort, right-click Open". | Maintainer | Yes after enrolment | Unsigned, and macOS packaging still red (#197) |
| **Linux:** AppImage and deb are unsigned; provenance is attested (`actions/attest-build-provenance`). A `SHA256SUMS` file is promised in #156 but not produced yet. | Agent | Yes | `SHA256SUMS` missing |
| Flatpak, AUR `PKGBUILD` bump and Flathub submission (#156 lists them; the v1 `flatpak/` manifest disappears from `main` at the swap) | Maintainer (Flathub account), Agent (manifest) | Partly | Not started; decide whether they block 2.0 |
| In-app "update available" notice (notify only, from GitHub Releases) | Agent | Yes | Not implemented |

## 4. Content and links

| Step | Owner | Automatable | State |
| --- | --- | --- | --- |
| **Final v1 release `1.9.0`** from `main` (via release-please) with an in-app "AniMathIO 2 is here" notice and import instructions; then create `legacy` from its tag | Maintainer + Agent (v1 code change on `main`/`staging`) | Release itself is automated; the v1 change is a normal PR | Not started |
| After the swap, any v1 patch release must be published with **"Set as latest release" off** | Maintainer | No | Policy only |
| **Repoint AniMathIODocs links** from `blob/main/CHANGELOG.md` to tag permalinks (for example `blob/animathio-v1.7.0/CHANGELOG.md`). Four posts link there today: `blog/2025-03-09-launching-v1.4.1.md`, `blog/2025-03-17-launching-v1.5.0.md`, `blog/2026-01-27-launching-v1.6.3.md`, `blog/2026-09-07-launching-v1.7.0.md`. | Agent | Yes (`sed` + PR to AniMathIODocs) | Not done; must land before the swap, because `main/CHANGELOG.md` is replaced by the 2.0 changelog |
| AniMathIODocs **v2 section** (generated from `docs/*.md`) | Agent | Mostly | Not started |
| AniMathIOWebsite download links: `src/pages/Home.tsx` hardcodes `animathio-v1.8.3` asset names. Replace with 2.0 installer names (`animathio_<version>_<arch>.AppImage`, `animathio_<version>_amd64.deb`, `animathio_<version>_x64-setup.exe`, `AniMathIO_<version>_<arch>.dmg`) or a release-triggered workflow that rewrites them. | Agent | Yes | Not done |
| **Rewrite the pre-swap wording** in the swap PR: `README.md` ("This is the `v2` branch", "Pull requests target `v2`"), `AGENTS.md` branch rules and the Renovate/release-plz notes, `renovate.json` (`baseBranchPatterns`), workflow branch filters. The rehearsal flags these as WARN lines and performs the mechanical part (`v2` to `main` in workflows and Renovate). | Agent | Mostly | Do in the swap PR |
| **Community files:** `v1` had `SECURITY.md` and `CONTRIBUTING.md` on `main`; the `-s ours` merge drops them because the `v2` tree wins. Add 2.0 versions (security reporting address, contribution flow with Conventional Commits) and issue/PR templates before the swap. | Agent | Yes | Missing on `v2` |
| 2.0 release notes: written from the generated `CHANGELOG.md` plus a "what's new since v1" summary | Agent + Maintainer | Partly | Pending the first release PR |

## 5. Quality gates still open (#156 exit criteria)

| Gate | Owner | Automatable | State |
| --- | --- | --- | --- |
| **M3 exit:** the v1 demo video recreated entirely in 2.0 | Maintainer | No | Not verified |
| **M4 exit:** every real v1 project in a fixture corpus imports and renders | Maintainer (supplies files), Agent (tests) | Tests yes, corpus no | Only synthetic fixtures |
| Windows hands-on pass of the editor (CI covers build and tests only) | Maintainer | No | Not done |
| Real-hardware listen test for the audio shuttle (#196 deferred) | Maintainer | No | Not done |
| Fuzzing (`cargo-fuzz`) of the project parser, v1 importer and media probe (listed under "Quality bar" in #156) | Agent | Yes | Not set up |

## 6. Order of operations at the swap

1. Everything in sections 2 to 5 that is marked "before the swap" is done. Docs links are repointed.
2. Final v1 release `1.9.0`; create and protect `legacy` from its tag.
3. On `v2`: `git merge -s ours main`, then one commit that retargets automation to `main`, enables release-plz tagging and rewrites the README/AGENTS wording.
4. Open the PR `v2` to `main` and merge it with a **merge commit** (not squash).
5. Delete `staging` and the `release-please--branches--*` branches. Install Renovate and add the `main` ruleset with required checks.
6. **Bootstrap `v2.0.0` by hand.** release-plz never creates the first tag (`release_always = false` only releases merged release PRs, and a release PR needs an earlier tag to diff against). Generate the changelog from the v2-only range, commit it, tag and publish:
   `git cliff --config cliff.toml --tag v2.0.0 <final-v1-tag>..HEAD -o CHANGELOG.md`, commit, `git tag -a v2.0.0`, push the tag, `gh release create v2.0.0 --notes-file <2.0.0 section>`. The package workflow should be run on that commit (`workflow_dispatch`) to produce the installers to attach.
7. From then on release-plz opens normal release PRs (`2.1.0`, ...); the package workflow smoke-tests the installers on each; merging one tags and publishes. Update the website download links and publish the AniMathIODocs launch post.

`scripts/swap-rehearsal.sh` exercises steps 2 to 6 end to end on a scratch clone, including the changelog bootstrap and a release-plz dry run.
