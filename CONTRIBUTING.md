# Contributing to Astrolabe

Thank you for taking the time. Astrolabe is a small app with a narrow goal: a journal of your sport activities, kept on your machine, in a single binary. Contributions that keep it that way are welcome.

By taking part you agree to the [code of conduct](CODE_OF_CONDUCT.md). For a security problem, follow the [security policy](SECURITY.md) instead of opening an issue. **Never put your Garmin key, a token or real activity data (tracks, places, health figures) in an issue or a pull request.**

## Before you write code

- **A bug**: open an issue with the version, the system and the steps to reproduce.
- **A feature**: open an issue first and wait for an answer. Many ideas are declined to keep the app small; it is better to know before the work is done.
- **A typo, a small fix**: a pull request is enough.

## Building and testing

```bash
cargo run                 # debug build
cargo run --release       # what gets installed
cargo test --locked       # or: make test
```

The system packages needed on each platform are listed in the README (*Install*). The tests need no display: they run on GPUI's test platform, and the FIT parsing runs on the files of `tests/fixtures`.

## Rules of the code base

- **Layers**: `logic` ← `platform` ← `ui` ← `data` ← `views` ← `app`, each one only sees those before it. The test `layers_only_depend_downwards` (in `src/main.rs`) enforces it.
- **gpui-kit is pinned** (0.8) and its API moves: check a signature in the source of that version, not from memory.
- **No new dependency** without an agreement in the issue. No global `cargo update`; one dependency at a time.
- **Code, comments**, commit messages, the README and issues are in English. The interface goes through `tr("English", "Français")`; a test fails on French text outside it.
- **Health data stay local**: the key goes to the system keyring or to the encrypted file, never in the clear; local files are written atomically and readable by the user alone; nothing is destroyed on disk. Tests never touch the real keyring.
- **A deliberate shortcut** carries a `// ponytail:` comment that names its limit and what would come next.

## Documentation

A visible feature updates the README and the changelog. Do not add screenshots made from real data: tracks, places and health figures are private.

## Commits and pull requests

- Subjects start with `feat:`, `fix:`, `docs:`, `test:`, `refactor:`, `build:` or `chore:`, in the imperative; the body explains why.
- One commit per coherent step, tests passing at each commit.
- The history stays linear: rebase on `develop`, no merge commits.
- CI builds and tests on Linux, macOS and Windows; it has to pass.

## Releasing (maintainer)

`develop` is the working branch; `main` only moves by fast-forward from it, when releasing.

1. CI is green on `develop`. Add the version's section to `CHANGELOG.md` (the release notes come from it).
2. Optionally try the script without side effects: `DRY_RUN=1 scripts/release.sh X.Y.Z`. It bumps `Cargo.toml`, `Cargo.lock` and the `PKGBUILD`, runs the tests, prints the notes, and puts everything back.
3. Fast-forward `main` to `develop`, then on `main`, clean and in sync with `origin`: `scripts/release.sh X.Y.Z`. It commits the bump, tags `vX.Y.Z`, pushes, creates the GitHub release, then pins the checksum of the tag's tarball in the `PKGBUILD` and regenerates `.SRCINFO`.
4. Publishing the release starts two workflows that attach the downloads: *Binaries* (Windows installer and `.zip`, macOS `.dmg`) and *Arch package*. They take about 15 minutes. `gh workflow run Binaries -f tag=develop -f attach=false` builds the binaries from a branch without touching any release.
5. The macOS and Windows files are not signed or notarized; the README says how to open them.
