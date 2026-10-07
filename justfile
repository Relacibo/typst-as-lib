default:
    @just --list

# Release: explicit version required — the tag push triggers CI, which runs
# the full test matrix and then publishes to crates.io (publish job in ci.yml).
#   just release v0.18.0   → test, bump Cargo.toml, commit, tag, push
# No auto-increment on purpose: a release should be an intentional act, so
# an accidental `just release` without a version aborts.
release version:
    #!/usr/bin/env bash
    set -euo pipefail

    if [ -z "{{version}}" ]; then
        echo "ERROR: no version given. A tag push publishes to crates.io — bump intentionally:" >&2
        echo "  just release v0.18.0" >&2
        exit 1
    fi

    case "{{version}}" in
        v*) tag="{{version}}" ;;
        *)  tag="v{{version}}" ;;
    esac
    ver="${tag#v}"

    if ! git diff --quiet || ! git diff --cached --quiet; then
        echo "ERROR: working tree is dirty — commit or stash first (CI publishes the tagged tree)" >&2
        exit 1
    fi

    branch="$(git branch --show-current)"
    if [ "$branch" != "main" ]; then
        echo "ERROR: not on main (on $branch) — release from main" >&2
        exit 1
    fi

    git fetch origin main --quiet
    if [ -n "$(git rev-list --count HEAD..origin/main)" ]; then
        echo "ERROR: origin/main has commits not in HEAD — pull first" >&2
        exit 1
    fi

    if git rev-parse -q --verify "refs/tags/${tag}" >/dev/null; then
        echo "ERROR: tag ${tag} already exists — pick the next version" >&2
        exit 1
    fi

    if ! grep -q "^## \[${ver}\]" CHANGELOG.md; then
        echo "ERROR: CHANGELOG.md has no '## [${ver}]' entry — add it first" >&2
        exit 1
    fi

    echo "Releasing ${tag} (test → bump → commit → tag → push)…"

    cargo test --quiet

    sed -i 's/^version = ".*"/version = "'"${ver}"'"/' Cargo.toml
    # Refresh Cargo.lock (the crate's version field lives there too).
    cargo metadata --quiet > /dev/null

    git add Cargo.toml Cargo.lock
    git commit -m "release: ${tag}"
    git tag "${tag}"
    git push origin main "${tag}"

    echo ""
    echo "Pushed ${tag}. CI runs the test matrix, then the publish job releases to crates.io."
    echo "Watch: gh run watch -R Relacibo/typst-as-lib"
