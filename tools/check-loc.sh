#!/usr/bin/env bash
# Rust LOC enforcement -- fails CI if any source file exceeds its limit.
# Limits come from AGENTS.md:
#   .rs files:  400 lines (general)
#   main.rs:    150 lines
#   lib.rs:     100 lines
#   mod.rs:     100 lines
#   build.rs:   200 lines
#
# No exceptions -- every file must comply.
#
# Usage: bash tools/check-loc.sh
# Exit code: 0 if all files pass, 1 if any violation found.

set -euo pipefail

VIOLATIONS=0

echo "=== Rust LOC Enforcement Check ==="
echo ""

check_file() {
  local file="$1"
  local limit="$2"
  local label="$3"
  local lines
  lines=$(wc -l < "$file")
  if [ "$lines" -gt "$limit" ]; then
    echo "VIOLATION: $file ($lines lines, limit $limit) [$label]"
    VIOLATIONS=$((VIOLATIONS + 1))
  fi
}

# main.rs files: limit 150
echo "--- main.rs files (limit 150) ---"
while IFS= read -r file; do
  [ -z "$file" ] && continue
  check_file "$file" 150 "main.rs"
done < <(find . -name 'main.rs' -path '*/src/*' ! -path '*/target/*' 2>/dev/null)

# lib.rs files: limit 100
echo "--- lib.rs files (limit 100) ---"
while IFS= read -r file; do
  [ -z "$file" ] && continue
  check_file "$file" 100 "lib.rs"
done < <(find . -name 'lib.rs' -path '*/src/*' ! -path '*/target/*' 2>/dev/null)

# mod.rs files: limit 100
echo "--- mod.rs files (limit 100) ---"
while IFS= read -r file; do
  [ -z "$file" ] && continue
  check_file "$file" 100 "mod.rs"
done < <(find . -name 'mod.rs' -path '*/src/*' ! -path '*/target/*' 2>/dev/null)

# build.rs files: limit 200
echo "--- build.rs files (limit 200) ---"
while IFS= read -r file; do
  [ -z "$file" ] && continue
  check_file "$file" 200 "build.rs"
done < <(find . -name 'build.rs' ! -path '*/target/*' 2>/dev/null)

# All other .rs files: limit 400
echo "--- .rs files (limit 400) ---"
while IFS= read -r file; do
  [ -z "$file" ] && continue
  basename=$(basename "$file")
  # Skip files with specific limits (already checked above)
  case "$basename" in
    main.rs|lib.rs|mod.rs|build.rs) continue ;;
  esac
  check_file "$file" 400 ".rs"
done < <(find . -name '*.rs' -path '*/src/*' ! -path '*/target/*' 2>/dev/null)

echo ""
if [ "$VIOLATIONS" -gt 0 ]; then
  echo "FAILED: $VIOLATIONS file(s) exceed LOC limits."
  exit 1
else
  echo "PASSED: All Rust files within LOC limits."
  exit 0
fi
