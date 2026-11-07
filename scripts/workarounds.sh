#!/bin/sh
# Regenerates the appendix of docs/WORKAROUNDS.md from the `shortcut:` comments in the code.
# Usage: scripts/workarounds.sh   (run from anywhere in the repo)
set -eu
cd "$(git rev-parse --show-toplevel)"
doc=docs/WORKAROUNDS.md
list=$(mktemp)
trap 'rm -f "$list"' EXIT
{
  echo "<!-- BEGIN GENERATED: scripts/workarounds.sh -->"
  echo
  echo "## Appendix: every \`shortcut:\` comment in the code"
  echo
  # One line per comment: the file:line, then the comment's first line.
  git grep -n -E '(//|#)!? *shortcut:' -- 'crates/*.rs' 'scripts/*' ':!scripts/workarounds.sh' |
    sed -E 's/^([^:]+):([0-9]+):[[:space:]]*(\/\/!?|#)[[:space:]]*shortcut:[[:space:]]*/- `\1:\2` /'
  echo
  echo "<!-- END GENERATED -->"
} > "$list"
awk -v list="$list" '
  /<!-- BEGIN GENERATED/ { while ((getline l < list) > 0) print l; skip = 1; next }
  /<!-- END GENERATED/ { skip = 0; next }
  !skip
' "$doc" > "$doc.tmp"
mv "$doc.tmp" "$doc"
echo "$(grep -c '^- `' "$doc") shortcut comments listed in $doc"
