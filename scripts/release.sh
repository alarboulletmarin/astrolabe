#!/usr/bin/env bash
# Usage: scripts/release.sh X.Y.Z [notes-file]
#        DRY_RUN=1 scripts/release.sh X.Y.Z    (bump and test, then put everything back: nothing is
#                                               committed, tagged or pushed, and the network is not used)
#
# Bumps the version, tags, pushes, creates the GitHub release, then pins the tarball checksum in the
# PKGBUILD and regenerates .SRCINFO. Run it on main, clean and in sync with origin. Notes: the given
# file, else the X.Y.Z section of CHANGELOG.md, else the commit subjects since the previous tag
# (chore/build/refactor commits left out).
set -euo pipefail
cd "$(dirname "$0")/.."

die() { echo "release: $*" >&2; exit 1; }
dry=${DRY_RUN:-}

ver=${1:?usage: scripts/release.sh X.Y.Z [notes-file]}
[[ $ver =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "version must look like 1.2.3"
git diff --quiet && git diff --cached --quiet || die "uncommitted changes"
git rev-parse -q --verify "refs/tags/v$ver" >/dev/null && die "tag v$ver already exists"
if [ -z "$dry" ]; then
    [ "$(git branch --show-current)" = main ] || die "not on main"
    git fetch -q origin
    [ "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" ] || die "main differs from origin/main"
fi

# First release: there is no previous tag, the notes cover the whole history.
prev=$(git describe --tags --abbrev=0 2>/dev/null || git rev-list --max-parents=0 HEAD)
notes=$(mktemp)
trap 'rm -f "$notes"' EXIT
if [ -n "${2:-}" ]; then
    cp "$2" "$notes"
else
    { section=$(awk -v v="$ver" '/^## /{on = ($2 == v || $2 == "[" v "]")} on' CHANGELOG.md 2>/dev/null | sed 1d)
      if [ -n "$section" ]; then
          echo "## Changes"; echo; echo "$section"
      else
          echo "## Changes"; echo
          git log "$prev..HEAD" --no-merges --pretty='- %s' | grep -vE '^- (chore|build|refactor)(\(.*\))?: ' || true
      fi
      echo; echo "## Install and upgrade"; echo
      echo "See the [README](https://github.com/alarboulletmarin/astrolabe#install). The downloads appear in the assets below within about 15 minutes, once GitHub has built them:"
      echo "- Arch: \`astrolabe-x86_64.pkg.tar.zst\`, then \`sudo pacman -U ./astrolabe-x86_64.pkg.tar.zst\`"
      echo "- macOS: \`astrolabe-macos.dmg\`"
      echo "- Windows: \`astrolabe-windows-x86_64-setup.exe\`, or \`astrolabe-windows-x86_64.zip\` to install nothing"
      echo; echo "The macOS and Windows files are not signed: the README says how to open them the first time."
    } >"$notes"
fi

sed -i "0,/^version = .*/s//version = \"$ver\"/" Cargo.toml
sed -i "s/^pkgver=.*/pkgver=$ver/" PKGBUILD
cargo update -p astrolabe --offline -q
cargo test --locked -q

if [ -n "$dry" ]; then
    echo "--- release notes ---"; cat "$notes"; echo "--- version diff ---"; git diff --stat; git diff -U0 -- Cargo.toml PKGBUILD
    git checkout -- Cargo.toml Cargo.lock PKGBUILD
    echo "dry run done: files restored"
    exit 0
fi

git add Cargo.toml Cargo.lock PKGBUILD
# Nothing to commit when the files already carry this version (first release).
git diff --cached --quiet || git commit -q -m "chore: release $ver"
git tag -a "v$ver" -m "Astrolabe $ver"
git push origin main "v$ver"
gh release create "v$ver" --title "Astrolabe $ver" --notes-file "$notes"

# The tarball GitHub serves for the tag is what the PKGBUILD downloads: hash that one.
url="https://github.com/alarboulletmarin/astrolabe/archive/refs/tags/v$ver.tar.gz"
sha=
for _ in 1 2 3 4 5; do
    sha=$(curl -fsL "$url" | sha256sum | cut -d' ' -f1) && [ -n "$sha" ] && break
    sleep 3
done
[ -n "$sha" ] || die "could not hash $url; pin it by hand"
sed -i "s/^sha256sums=.*/sha256sums=('$sha')/" PKGBUILD
makepkg --printsrcinfo >.SRCINFO
git add PKGBUILD .SRCINFO
git commit -q -m "build: pin v$ver checksum for the AUR"
git push origin main
echo "released v$ver ($sha)"
