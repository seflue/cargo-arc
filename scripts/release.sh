#!/usr/bin/env bash
# Idempotent release: bump version + changelog first, then run this.
# Each step checks whether it already ran and skips itself.
set -euo pipefail

version=$(cargo metadata --no-deps --format-version=1 | grep -oP '"version":"\K[^"]+')
tag="v${version}"

# guard: changelog must mention this version
if ! grep -qF "[${version}]" CHANGELOG.md; then
  echo "❌ CHANGELOG.md has no entry for [${version}]. Add one first." >&2; exit 1
fi

echo "Releasing ${tag}..."

# quality gate
just lint
just test
cargo package --allow-dirty

# tag (skip if already exists — and skip commit+push too, release is done)
if git rev-parse "$tag" >/dev/null 2>&1; then
  echo "⏭️  Tag $tag already exists, skipping to GitHub release"
else
  # commit (only as part of a new release)
  if ! git diff --quiet HEAD || [ -n "$(git ls-files --others --exclude-standard)" ]; then
    git add -A
    git commit -m "Release ${tag}"
  fi
  git tag "$tag"
fi

# push (skip if remote already has the tag)
if git ls-remote --tags origin "$tag" | grep -q "$tag"; then
  echo "⏭️  Tag $tag already pushed"
else
  git push origin HEAD --tags
fi

# github release (skip if already exists)
if gh release view "$tag" >/dev/null 2>&1; then
  echo "⏭️  GitHub release $tag already exists"
else
  notes=$(sed -n '/^## \['"${version}"'\]/,/^## \[/{/^## \['"${version}"'\]/d;/^## \[/d;p}' CHANGELOG.md)
  gh release create "$tag" --title "$tag" --notes "$notes"
fi

echo ""
echo "✅ Released ${tag}"
echo "   Remaining manual step: cargo publish"
