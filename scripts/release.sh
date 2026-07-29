#!/usr/bin/env bash
# cargo-release bumps the version, stamps the CHANGELOG, commits, tags,
# pushes and publishes. This adds the GitHub release on top.
set -euo pipefail

level="${1:?usage: release.sh <version|major|minor|patch>}"

cargo release "$level" --execute

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
