#!/usr/bin/env bash
# cargo-release bumps the version, stamps the CHANGELOG, commits, tags and
# pushes. This publishes the crate and adds the GitHub release on top.
set -euo pipefail

level="${1:?usage: release.sh <version|major|minor|patch>}"

# The crate ships the page script bundles; a checkout never keeps them.
# cargo publish counts them as uncommitted changes, hence --allow-dirty;
# cargo-release has already refused any other change in the tree.
trap 'rm -rf js/dist' EXIT
bun build js/svg_script.js js/hotspot_script.js --format=iife --outdir js/dist

# Package and build it before tagging, so a broken package stops the
# release early.
cargo package --allow-dirty

cargo release "$level" --execute

cargo publish --allow-dirty

version=$(cargo metadata --no-deps --format-version=1 |
  grep -oP '"version":"\K[^"]+' | head -1)
tag="v${version}"

if gh release view "$tag" >/dev/null 2>&1; then
  echo "⏭️  GitHub release $tag already exists"
else
  notes=$(sed -n '/^## \['"${version}"'\]/,/^## \[/{/^## \['"${version}"'\]/d;/^## \[/d;p}' CHANGELOG.md)
  gh release create "$tag" --title "$tag" --notes "$notes"
fi

echo ""
echo "✅ Released ${tag}"
