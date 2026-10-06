# Releasing AniMathIO

The release itself is automated by release-please; the parts that go wrong are
the ones around it. This is the checklist, written from a release where most of
these steps bit us.

Three repositories are involved:

| Repo | Role |
| --- | --- |
| `AniMathIO/AniMathIO` | the app; where the release is cut |
| `AniMathIO/AniMathIOWebsite` | animathio.com — download links point at release assets |
| `AniMathIO/AniMathIODocs` | docs.animathio.com — guides and release blog posts |

## 1. Get `staging` green

Work lands on `staging` first. Before considering a release:

```bash
npm run test:unit      # unit
npm run test:browser   # real Chromium
npx tsc -p renderer/tsconfig.json --noEmit   # NOT the bare root tsconfig, it is known-noisy
npm run build          # AppImage + deb + snap
```

**Always run `npm run build` before a release, even though CI doesn't.** The
push-triggered workflow runs tests only — nothing exercises packaging until a
tag already exists. Two separate packaging breakages (a missing `homepage`
field, and a renamed Linux executable) reached a published release precisely
because of this gap.

## 2. Deal with open dependabot PRs first

Merge or resolve them *before* cutting the release, so the release ships the
security fixes and the lockfile doesn't churn mid-release.

**Read the `package.json` diff, not the PR summary table.** Dependabot bundles
unrelated version bumps into security-group PRs, and the body's table does not
always list them:

```bash
gh pr diff <N> | awk '/^diff --git a\/package.json/,/^diff --git a\/package-lock.json/'
```

Nextron in particular keeps arriving this way. It is deliberately held at 9.x
(see `DEPENDENCIES.md`) and it is what builds the release binaries, so a
surprise major there breaks packaging. To take the safe parts of such a PR:
check out its branch, revert the deferred package in `package.json`, re-run
`npm install`, verify, and push back to the same branch.

Also re-check `allowScripts` in `package.json` after any bump — the entries are
version-pinned, so a bumped package silently loses its install-script approval.
`npm install` warns about this; don't ignore it.

## 3. Merge `staging` → `main`

Open a PR and **merge it with a merge commit — never squash.**

A squash collapses everything into one commit whose subject is the PR title.
Unless that title happens to be a conventional-commit string, release-please
sees nothing releasable, opens no release PR, and the entire build-and-upload
chain (gated on `release_created`) never runs.

## 4. Merge the release PR — this is a second merge

Release-please opens `chore(main): release animathio X.Y.Z`, which bumps
`package.json`, `package-lock.json`, `.release-please-manifest.json` and
`CHANGELOG.md`. **The tag and the binaries only appear when that PR merges.**
Don't go looking for a tag after step 3.

If dependabot merged something in between, sanity-check that the release PR
won't revert it. Git merges by diff, not snapshot, so an older branch is
usually fine — verify rather than assume:

```bash
git checkout -B trial origin/main
git merge --no-commit --no-ff origin/release-please--branches--main--components--animathio
# inspect package-lock.json, then:
git merge --abort
```

## 5. Verify the artifacts actually uploaded

A green workflow is not proof. Check the release itself:

```bash
gh release view animathio-vX.Y.Z --json assets -q '.assets[].name'
```

Expect: `.AppImage`, `.deb`, `.snap`, `.flatpak`, `.exe` (+ `.blockmap`),
`linux-unpacked.tar.gz`, `win-unpacked.zip`.

**A missing `.flatpak` is the classic failure.** The flatpak manifest copies the
Linux executable by name, so anything that changes that name breaks it — which
is why `electron-builder.yml` pins `linux.executableName: AniMathIO`. The
flatpak job cannot be re-run against an existing tag (its build jobs are gated
on `release_created`), so fixing it requires a patch release.

## 6. Update the issue tracker

- Tick off what shipped in the roadmap issue and link the release
- Update the known-bugs issue with what was actually fixed and how

## 7. Update the website

In `AniMathIOWebsite`, `src/pages/Home.tsx` holds the `downloads` array. Update
every `installLink` to the new tag (macOS intentionally stays on legacy 1.3.0).
Then verify each URL actually resolves before pushing:

```bash
grep -o 'https://github.com/AniMathIO/AniMathIO/releases/download/[^"]*' src/pages/Home.tsx \
  | while read u; do echo "$(curl -s -o /dev/null -w '%{http_code}' -L -r 0-0 "$u")  $(basename $u)"; done
```

`200`/`206` is good. Then `npm run build` and push.

## 8. Update the docs

In `AniMathIODocs`:

- Add a release blog post under `blog/` following `YYYY-MM-DD-launching-vX.Y.Z.md`
- Document any new user-facing feature — a headline feature shipping undocumented
  is easy to miss, since nothing fails when docs are absent
- Update `docs/tutorial-basics/understanding-the-interface.md` if the sidebar,
  panels or menus changed
- Refresh screenshots for anything visibly different
- `npm run build` catches broken internal links; push once it's clean

## Capturing screenshots

The app is Electron, so browser automation can't attach directly. `npm run dev`
starts it with a remote debugging port, which Playwright can connect to:

```js
const browser = await chromium.connectOverCDP('http://127.0.0.1:5858');
const page = browser.contexts().flatMap(c => c.pages())
  .find(p => !p.url().startsWith('devtools://'));
```

Use `npm run dev:8889` if the default port is busy, and `fuser -k 8889/tcp` to
clear a stale instance.

**Press Play before capturing the canvas.** Imported and animated elements start
at zero opacity, so a screenshot taken at the start of the timeline shows an
empty canvas and looks like a bug. Scrubbing the ruler is not equivalent to
playing far enough for entrance animations to have run.

## Post-release

Merge `main` back into `staging` so the version bump and any release fixes are
not stranded.
