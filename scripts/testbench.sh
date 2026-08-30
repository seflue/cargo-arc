#!/usr/bin/env bash
# Set up the foreign workspaces the manual acceptance path runs against.
#
# The point is a fixed input: the first run records the commit each project was
# at, later runs check that commit out again. Without it, a differing report
# cannot be told apart from an upstream merge.
set -Eeuo pipefail

root="${ARC_TESTBENCH_ROOT:-$HOME/source/external/rust}"
work="${ARC_TESTBENCH_WORK:-/tmp/arc-abnahme}"
pins="$work/pins"
repin=false
fetch=true

usage() {
  cat <<'EOF'
Usage: scripts/testbench.sh [--repin] [--no-fetch]

  --repin      Move the pinned commits to each project's current default branch.
               Without it, recorded commits are restored and upstream is ignored.
  --no-fetch   Skip `cargo fetch`. The analysis reads `cargo metadata`, which
               needs resolvable dependencies, so only skip this when they are
               already there.

Environment:
  ARC_TESTBENCH_ROOT   where the clones live (default: ~/source/external/rust)
  ARC_TESTBENCH_WORK   rules files, baselines and pins (default: /tmp/arc-abnahme)
EOF
}

while [ $# -gt 0 ]; do
  case "$1" in
    --repin) repin=true ;;
    --no-fetch) fetch=false ;;
    -h | --help) usage; exit 0 ;;
    *) echo "❌ Unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
  shift
done

# name<TAB>url. The acceptance path names what each one is for.
projects=$(
  cat <<'EOF'
nushell	https://github.com/nushell/nushell.git
rust-analyzer	https://github.com/rust-lang/rust-analyzer.git
hyperswitch	https://github.com/juspay/hyperswitch.git
zebra	https://github.com/ZcashFoundation/zebra.git
EOF
)

# name<TAB>source repo<TAB>ref. A worktree of a repository already cloned above
# or already on disk. Two refs of one project are two entries here and cost one
# clone, and neither of them disturbs the source repository's own checkout.
worktrees=$(
  cat <<'EOF'
dioxus-0.7	dioxus	v0.7
dioxus-main	dioxus	main_upstream
EOF
)

mkdir -p "$root" "$work"
touch "$pins"

# Blobless clone: full history, so any pinned commit stays reachable, without
# paying for every blob of three large repositories up front.
clone_or_update() {
  local name="$1" url="$2"
  local dir="$root/$name"
  if [ -d "$dir/.git" ]; then
    echo "↻ $name: fetching"
    # A tag moved upstream fails the whole fetch. The pins are commits, so
    # nothing here reads a tag.
    git -C "$dir" fetch --quiet origin
  else
    echo "⇣ $name: cloning into $dir"
    git clone --quiet --filter=blob:none "$url" "$dir"
  fi
  # A repository cloned elsewhere may carry no origin/HEAD; pinning reads it.
  git -C "$dir" remote set-head --auto origin >/dev/null
}

worktree_or_add() {
  local name="$1" repo="$2" ref="$3"
  local dir="$root/$name" src="$root/$repo"
  if [ ! -d "$src/.git" ]; then
    echo "❌ $name: source repository $src is missing" >&2
    return 1
  fi
  if [ -d "$dir" ]; then
    echo "↻ $name: worktree is there"
  else
    echo "⇣ $name: worktree of $repo at $ref into $dir"
    git -C "$src" worktree add --quiet --detach "$dir" "$ref"
  fi
}

pinned_commit() {
  awk -F'\t' -v n="$1" '$1 == n { print $2 }' "$pins"
}

record_pin() {
  local name="$1" commit="$2" tmp
  tmp=$(mktemp)
  awk -F'\t' -v n="$name" '$1 != n' "$pins" >"$tmp"
  printf '%s\t%s\n' "$name" "$commit" >>"$tmp"
  sort -o "$pins" "$tmp"
  rm -f "$tmp"
}

checkout_pin() {
  local name="$1" default_ref="${2:-origin/HEAD}"
  local dir="$root/$name" commit
  commit=$(pinned_commit "$name")

  if [ -n "$commit" ] && [ "$repin" = false ]; then
    echo "📌 $name: restoring ${commit:0:8}"
  else
    commit=$(git -C "$dir" rev-parse "$default_ref")
    record_pin "$name" "$commit"
    echo "📌 $name: pinned to ${commit:0:8}"
  fi

  git -C "$dir" checkout --quiet --detach "$commit"
}

report_setup_failure() {
  [ "$BASH_SUBSHELL" = 0 ] || return 0
  echo "❌ $name: setup failed (reason above). Test bench is incomplete; do not source $work/env.sh." >&2
  exit 1
}

# set -E makes this trap fire inside clone_or_update and checkout_pin too.
# The BASH_SUBSHELL filter keeps a subshell failure from printing twice.
trap report_setup_failure ERR

while IFS=$'\t' read -r name url; do
  [ -n "$name" ] || continue
  clone_or_update "$name" "$url"
  checkout_pin "$name"
  if [ "$fetch" = true ]; then
    echo "📦 $name: cargo fetch"
    (cd "$root/$name" && cargo fetch --quiet)
  fi
done <<<"$projects"

while IFS=$'\t' read -r name repo ref; do
  [ -n "$name" ] || continue
  worktree_or_add "$name" "$repo" "$ref"
  checkout_pin "$name" "$ref"
  if [ "$fetch" = true ]; then
    echo "📦 $name: cargo fetch"
    (cd "$root/$name" && cargo fetch --quiet)
  fi
done <<<"$worktrees"
trap - ERR

env_file="$work/env.sh"
cat >"$env_file" <<EOF
# Written by scripts/testbench.sh. Source this before walking the acceptance path.
export NUSHELL=$root/nushell
export RUST_ANALYZER=$root/rust-analyzer
export HYPERSWITCH=$root/hyperswitch
export ZEBRA=$root/zebra
export DIOXUS_07=$root/dioxus-0.7
export DIOXUS_MAIN=$root/dioxus-main
export W=$work
EOF

# Starter rules, one per station that needs one. Never overwrite: the walkthrough
# has you edit them, and a second run must not discard that.
rules_src="$(dirname "$0")/../memories/references/abnahme-rules"
if [ -d "$rules_src" ]; then
  for rule in "$rules_src"/*.toml; do
    [ -e "$rule" ] || continue
    if [ -e "$work/$(basename "$rule")" ]; then
      echo "⏭  $(basename "$rule") liegt schon in $work, bleibt unverändert"
    else
      cp "$rule" "$work/"
      echo "📄 $(basename "$rule") → $work"
    fi
  done
fi

echo
echo "✅ Test bench ready. Pinned commits: $pins"
echo
echo "   source $env_file"
echo
echo "Then run everything from this repository, so cargo run builds the working tree:"
echo "   cargo run -- arc -m \$NUSHELL/Cargo.toml check"
