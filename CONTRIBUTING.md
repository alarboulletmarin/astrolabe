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

The system packages needed on each platform are listed in the [install guide](book/src/install.md). The tests need no display, no network and no Garmin account: they run on GPUI's test platform, the FIT parsing runs on the files of `tests/fixtures`, and the sync flows run against a local stand-in for Garmin.

## Rules of the code base

[ARCHITECTURE.md](ARCHITECTURE.md) is the map: the layers, how the data moves from Garmin to the disk and the views, the files on disk, and the kinds of tests.

- **Layers**: `logic` ← `platform` ← `ui` ← `data` ← `views` ← `app`, each one only sees those before it. The test `layers_only_depend_downwards` (in `src/main.rs`) enforces it.
- **gpui-kit is pinned** (0.8) and its API moves: check a signature in the source of that version, not from memory.
- **No new dependency** without an agreement in the issue. No global `cargo update`; one dependency at a time.
- **Code, comments**, commit messages, the README and issues are in English. The interface goes through `tr("English", "Français")`; a test fails on French text outside it.
- **Health data stay local**: the key goes to the system keyring or to the encrypted file, never in the clear; local files are written atomically and readable by the user alone; nothing is destroyed on disk. Tests never touch the real keyring.
- **A deliberate shortcut** carries a `// ponytail:` comment that names its limit and what would come next.

## Tests

- Pure functions are tested in their own module (`src/logic/` needs neither gpui nor disk).
- Every user path is tested on GPUI's test platform in `src/app/tests.rs`. A new feature adds its path there.
- FIT parsing runs on the files of `tests/fixtures`. Add a small file there rather than reading one from your own account.
- Sync flows run against a local stand-in for Garmin; no test touches the network or the real keyring.
- Ignored benchmarks time a large made-up history. Run them with `cargo test --release --locked -- --ignored --nocapture`; their results are in the guide's [Performance](book/src/performance.md) page.

## Documentation

Each kind of reader has one place:

| Reader | Place |
|---|---|
| someone deciding whether to try Astrolabe | `README.md` and the website, `docs/index.html`: what it is, why, how to install. Short. |
| a user | the user guide, `book/src/` (an [mdBook](https://rust-lang.github.io/mdBook/)), one page per screen or subject |
| a contributor | `CONTRIBUTING.md`, `ARCHITECTURE.md`, `RELEASING.md` |

A visible feature updates:

- the page of the guide on its subject, or a new page listed in `book/src/SUMMARY.md`; and `book/src/shortcuts.md` for a key;
- the README and the feature list of `docs/index.html`, only for a feature worth a line there;
- `CHANGELOG.md`.

Write the guide in short sentences, one idea per bullet, keys in backticks, button and command names in bold, interface labels as they appear in English (the interface follows the system language, English or French). Describe what the code does, and name no other app. Do not add screenshots made from real data: tracks, places and health figures are private. Where a screenshot belongs, leave a `<!-- screenshot: … -->` comment. `scripts/demo-data.py DIR` writes a made-up athlete's folder; open it with `XDG_DATA_HOME=DIR`.

CI builds the guide and checks every link (`.github/workflows/docs.yml`); pushes to `main` publish it with the website.

## Commits and pull requests

- Subjects start with `feat:`, `fix:`, `docs:`, `test:`, `refactor:`, `build:` or `chore:`, in the imperative; the body explains why.
- One commit per coherent step, tests passing at each commit.
- The history stays linear: rebase on `develop`, no merge commits.
- CI builds and tests on Linux, macOS and Windows; it has to pass.

## Releasing (maintainer)

`develop` is the working branch; `main` only moves by fast-forward from it, when releasing.

1. CI is green on `develop`. Add the version's section to `CHANGELOG.md`.
2. Rehearse with `DRY_RUN=1 scripts/release.sh X.Y.Z`; managed files are restored even if tests fail.
3. Fast-forward `main` to `develop`, synchronize it with `origin`, then run `scripts/release.sh X.Y.Z` from clean `main`.
4. The script creates a draft and explicitly dispatches **Native release**. It is published only after Arch, macOS and Windows packaging succeeds and every expected download has been attached and checked.

Read [RELEASING.md](RELEASING.md) for prerequisites, recovery after failure, artifact-only builds and the release regression tests. macOS and Windows downloads remain unsigned and unnotarized.
