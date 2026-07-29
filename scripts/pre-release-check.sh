#!/usr/bin/env bash
# cargo-release pre-release-hook: refuse to release code CI has not approved,
# then run the local quality gate.
set -euo pipefail

git fetch --quiet origin main
head=$(git rev-parse HEAD)
if [ "$head" != "$(git rev-parse origin/main)" ]; then
  echo "❌ HEAD is not origin/main. Push your work and let CI run first." >&2; exit 1
fi

checks=$(gh api "repos/{owner}/{repo}/commits/${head}/check-runs" \
  --jq '.check_runs[] | [.name, .status, (.conclusion // "pending")] | @tsv')
if [ -z "$checks" ]; then
  echo "❌ No CI runs for origin/main (${head:0:8})." >&2; exit 1
fi
while IFS=$'\t' read -r check status conclusion; do
  case "${status}:${conclusion}" in
    completed:success | completed:skipped | completed:neutral) ;;
    completed:*) echo "❌ CI check '${check}': ${conclusion}." >&2; exit 1 ;;
    *) echo "❌ CI check '${check}' is still ${status}." >&2; exit 1 ;;
  esac
done <<<"$checks"
echo "✅ CI green on origin/main (${head:0:8})"

if [ "${DRY_RUN:-false}" = "true" ]; then
  echo "⏭️  Dry run, skipping lint and tests"
  exit 0
fi

just lint
just test
