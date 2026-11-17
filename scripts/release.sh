#!/usr/bin/env bash
# Cut a release: bump the semver version, update CHANGELOG.md, tag, build,
# package and publish a GitHub release that installed apps update from.
#
#   scripts/release.sh <major|minor|patch> [--pre <id>] [--skip-tests] [--dry-run]
#
#   patch      0.2.0 -> 0.2.1      bug fixes
#   minor      0.2.1 -> 0.3.0      new features
#   major      0.3.0 -> 1.0.0      breaking changes
#   --pre beta 0.2.1 -> 0.3.0-beta.1 (minor) / 0.3.0-beta.1 -> 0.3.0-beta.2 (any bump
#              while already on a beta just increments it); published as a GitHub
#              pre-release, only offered to users who opted in
#
# Release notes come from the "## [Unreleased]" section of CHANGELOG.md, or from
# the commit subjects since the previous tag when that section is empty.
# Releases should normally be cut from master after the PRs are merged.
set -euo pipefail

REPO_SLUG="Alteian/needmediacenter"
cd "$(dirname "$0")/.."

BUMP="${1:-}"; shift || true
PRE=""; SKIP_TESTS=0; DRY=0
while [ $# -gt 0 ]; do
    case "$1" in
        --pre) PRE="$2"; shift 2 ;;
        --skip-tests) SKIP_TESTS=1; shift ;;
        --dry-run) DRY=1; shift ;;
        *) echo "unknown option $1" >&2; exit 2 ;;
    esac
done
case "$BUMP" in major|minor|patch) ;; *) sed -n '2,20p' "$0"; exit 2 ;; esac

[ -z "$(git status --porcelain --untracked-files=no)" ] || { echo "working tree has uncommitted changes" >&2; exit 1; }
gh auth status >/dev/null 2>&1 || { echo "gh is not logged in (gh auth login)" >&2; exit 1; }
BRANCH=$(git rev-parse --abbrev-ref HEAD)
case "$BRANCH" in master|main) ;; *) echo "note: releasing from branch '$BRANCH' (not master)" >&2 ;; esac

CUR=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
CORE=${CUR%%-*}
IFS=. read -r MA MI PA <<<"$CORE"
CUR_PRE=""; [ "$CUR" != "$CORE" ] && CUR_PRE=${CUR#*-}
if [ -n "$PRE" ] && [[ "$CUR_PRE" == "$PRE".* ]]; then
    NEW="$CORE-$PRE.$(( ${CUR_PRE##*.} + 1 ))"
elif [ -n "$CUR_PRE" ] && [ -z "$PRE" ]; then
    NEW="$CORE"   # finalize the pre-release
else
    case "$BUMP" in
        major) NEW="$((MA + 1)).0.0" ;;
        minor) NEW="$MA.$((MI + 1)).0" ;;
        patch) NEW="$MA.$MI.$((PA + 1))" ;;
    esac
    [ -n "$PRE" ] && NEW="$NEW-$PRE.1"
fi
TAG="v$NEW"
git rev-parse -q --verify "refs/tags/$TAG" >/dev/null && { echo "tag $TAG already exists" >&2; exit 1; }
echo "Releasing $CUR -> $NEW ($TAG) from $BRANCH"

PREV_TAG=$(git describe --tags --abbrev=0 --match 'v[0-9]*' 2>/dev/null || true)
NOTES=$(mktemp); trap 'rm -f "$NOTES"' EXIT
[ -f CHANGELOG.md ] || printf '# Changelog\n\nAll notable changes to RustMediaHub. Versions follow [semver](https://semver.org).\n\n## [Unreleased]\n' >CHANGELOG.md
# Unreleased section body (until the next "## " heading)
awk '/^## \[Unreleased\]/{f=1;next} /^## /{f=0} f' CHANGELOG.md | sed '/^[[:space:]]*$/d' >"$NOTES" || true
if [ ! -s "$NOTES" ]; then
    RANGE=${PREV_TAG:+$PREV_TAG..}HEAD
    git log --no-merges --pretty='- %s' "$RANGE" | grep -v '^- release: ' >"$NOTES" || true
fi
[ -s "$NOTES" ] || echo "- Maintenance release" >"$NOTES"
echo "--- release notes ---"; cat "$NOTES"; echo "---------------------"
[ $DRY -eq 1 ] && { echo "dry run: nothing changed"; exit 0; }

restore() { git checkout -- Cargo.toml Cargo.lock CHANGELOG.md 2>/dev/null || true; }
trap 'restore; rm -f "$NOTES"' ERR

# bump version (workspace.package) and the lockfile
sed -i "/^\[workspace.package\]/,/^\[/s/^version = \".*\"/version = \"$NEW\"/" Cargo.toml
# changelog: empty Unreleased, add the new section below it
DATE=$(date +%Y-%m-%d)
SECTION=$(mktemp)
{ echo "## [$NEW] - $DATE"; echo; cat "$NOTES"; echo; } >"$SECTION"
awk -v sec="$SECTION" '
    /^## \[Unreleased\]/ { print; print ""; while ((getline l < sec) > 0) print l; skip=1; next }
    skip && /^## / { skip=0 }
    !skip { print }
' CHANGELOG.md >CHANGELOG.md.new && mv CHANGELOG.md.new CHANGELOG.md
rm -f "$SECTION"

[ $SKIP_TESTS -eq 1 ] || cargo test --workspace --quiet
TARBALL=$(scripts/package.sh | tail -n 1)
SUMFILE="$TARBALL.sha256"

git add Cargo.toml Cargo.lock CHANGELOG.md
git commit -q -m "release: $TAG"
git tag -a "$TAG" -m "RustMediaHub $NEW"
trap - ERR
git push -q origin "HEAD:refs/heads/$BRANCH"
git push -q origin "$TAG"

PRE_FLAG=(); [[ "$NEW" == *-* ]] && PRE_FLAG=(--prerelease)
gh release create "$TAG" "$TARBALL" "$SUMFILE" \
    --repo "$REPO_SLUG" \
    --title "RustMediaHub $NEW" \
    --notes-file "$NOTES" \
    --verify-tag "${PRE_FLAG[@]}"
echo "Released $TAG: $(gh release view "$TAG" --repo "$REPO_SLUG" --json url -q .url)"
