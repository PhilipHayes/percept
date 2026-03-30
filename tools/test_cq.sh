#!/usr/bin/env bash
# cq test suite
set -euo pipefail

CQ="$(cd "$(dirname "$0")/.." && pwd)/bin/cq"
PASS=0 FAIL=0 TOTAL=0

assert_eq() {
  local label="$1" expected="$2" actual="$3"
  TOTAL=$((TOTAL + 1))
  if [[ "$expected" == "$actual" ]]; then
    PASS=$((PASS + 1))
    echo "  ✓ $label"
  else
    FAIL=$((FAIL + 1))
    echo "  ✗ $label"
    echo "    expected: $expected"
    echo "    actual:   $actual"
  fi
}

assert_json_eq() {
  local label="$1" expected="$2" actual="$3"
  TOTAL=$((TOTAL + 1))
  # Normalize JSON for comparison
  local e_norm a_norm
  e_norm=$(echo "$expected" | jq -cS '.' 2>/dev/null) || e_norm="$expected"
  a_norm=$(echo "$actual" | jq -cS '.' 2>/dev/null) || a_norm="$actual"
  if [[ "$e_norm" == "$a_norm" ]]; then
    PASS=$((PASS + 1))
    echo "  ✓ $label"
  else
    FAIL=$((FAIL + 1))
    echo "  ✗ $label"
    echo "    expected: $e_norm"
    echo "    actual:   $a_norm"
  fi
}

# ─── Version ───
echo "--- version ---"
out=$($CQ --version 2>&1)
assert_eq "prints version" "cq 0.1.0" "$out"

# ─── Join: inner ───
echo "--- join inner ---"
LEFT='[{"name":"a","v":1},{"name":"b","v":2},{"name":"c","v":3}]'
RIGHT='[{"id":"a","x":10},{"id":"b","x":20},{"id":"d","x":40}]'

out=$($CQ join <(echo "$LEFT") <(echo "$RIGHT") -l '.name' -r '.id' 2>&1)
count=$(echo "$out" | jq 'length')
assert_eq "inner join returns matching count" "2" "$count"

first_left=$(echo "$out" | jq -c '.[0]._left.name')
assert_eq "inner join first match" '"a"' "$first_left"

first_right=$(echo "$out" | jq -c '.[0]._right.x')
assert_eq "inner join carries right data" '10' "$first_right"

# ─── Join: left ───
echo "--- join left ---"
out=$($CQ join <(echo "$LEFT") <(echo "$RIGHT") -l '.name' -r '.id' -t left 2>&1)
count=$(echo "$out" | jq 'length')
assert_eq "left join returns all left items" "3" "$count"

null_right=$(echo "$out" | jq -c '.[2]._right')
assert_eq "left join null for unmatched" 'null' "$null_right"

# ─── Join: anti ───
echo "--- join anti ---"
out=$($CQ join <(echo "$LEFT") <(echo "$RIGHT") -l '.name' -r '.id' -t anti 2>&1)
count=$(echo "$out" | jq 'length')
assert_eq "anti join returns unmatched" "1" "$count"

name=$(echo "$out" | jq -c '.[0].name')
assert_eq "anti join returns c" '"c"' "$name"

# ─── Join: semi ───
echo "--- join semi ---"
out=$($CQ join <(echo "$LEFT") <(echo "$RIGHT") -l '.name' -r '.id' -t semi 2>&1)
count=$(echo "$out" | jq 'length')
assert_eq "semi join returns matched left items" "2" "$count"

names=$(echo "$out" | jq -c '[.[].name]')
assert_eq "semi join returns a,b" '["a","b"]' "$names"

# ─── Join: empty result ───
echo "--- join empty ---"
out=$($CQ join <(echo '[{"k":"x"}]') <(echo '[{"k":"y"}]') -l '.k' -r '.k' 2>&1)
assert_json_eq "inner join with no matches" '[]' "$out"

# ─── Join: numeric keys ───
echo "--- join numeric keys ---"
out=$($CQ join <(echo '[{"id":1,"v":"a"},{"id":2,"v":"b"}]') <(echo '[{"num":2,"w":"x"}]') -l '.id' -r '.num' 2>&1)
count=$(echo "$out" | jq 'length')
assert_eq "join on numeric keys" "1" "$count"

# ─── Lookup ───
echo "--- lookup ---"
out=$($CQ lookup <(echo "$LEFT") <(echo "$RIGHT") -l '.name' -r '.id' 2>&1)
count=$(echo "$out" | jq 'length')
assert_eq "lookup returns all left items" "3" "$count"

has_lookup=$(echo "$out" | jq -c '.[0]._lookup.x')
assert_eq "lookup enriches matched" '10' "$has_lookup"

miss_lookup=$(echo "$out" | jq -c '.[2]._lookup')
assert_eq "lookup null for unmatched" 'null' "$miss_lookup"

# ─── Lookup with default ───
echo "--- lookup with default ---"
out=$($CQ lookup <(echo "$LEFT") <(echo "$RIGHT") -l '.name' -r '.id' -d '{"x":0}' 2>&1)
default_val=$(echo "$out" | jq -c '.[2]._lookup.x')
assert_eq "lookup uses default for miss" '0' "$default_val"

# ─── Group ───
echo "--- group ---"
DATA='[{"name":"a","kind":"fn"},{"name":"b","kind":"struct"},{"name":"c","kind":"fn"}]'
out=$($CQ group <(echo "$DATA") -k '.kind' 2>&1)
count=$(echo "$out" | jq 'length')
assert_eq "group returns correct groups" "2" "$count"

fn_count=$(echo "$out" | jq -c '[.[] | select(.key=="fn")][0].count')
assert_eq "group counts fn" '2' "$fn_count"

struct_count=$(echo "$out" | jq -c '[.[] | select(.key=="struct")][0].count')
assert_eq "group counts struct" '1' "$struct_count"

# ─── Group with -v ───
echo "--- group with -v ---"
out=$($CQ group <(echo "$DATA") -k '.kind' -v '.name' 2>&1)
fn_values=$(echo "$out" | jq -c '[.[] | select(.key=="fn")][0].values')
assert_eq "group -v projects values" '["a","c"]' "$fn_values"

# ─── Diff ───
echo "--- diff ---"
out=$($CQ diff <(echo "$LEFT") <(echo "$RIGHT") -l '.name' -r '.id' 2>&1)
only_left=$(echo "$out" | jq '.only_left | length')
only_right=$(echo "$out" | jq '.only_right | length')
both=$(echo "$out" | jq '.both | length')
assert_eq "diff only_left count" "1" "$only_left"
assert_eq "diff only_right count" "1" "$only_right"
assert_eq "diff both count" "2" "$both"

left_name=$(echo "$out" | jq -c '.only_left[0].name')
assert_eq "diff only_left is c" '"c"' "$left_name"

right_id=$(echo "$out" | jq -c '.only_right[0].id')
assert_eq "diff only_right is d" '"d"' "$right_id"

# ─── NDJSON auto-wrap ───
echo "--- ndjson auto-wrap ---"
NDJSON=$'{"name":"a"}\n{"name":"b"}'
out=$($CQ join <(echo "$NDJSON") <(echo '[{"id":"a","x":1}]') -l '.name' -r '.id' 2>&1)
count=$(echo "$out" | jq 'length')
assert_eq "auto-wraps ndjson to array" "1" "$count"

# ─── Error handling ───
echo "--- errors ---"
out=$($CQ join /nonexistent /dev/null -l '.k' -r '.k' 2>&1) || true
has_error=$(echo "$out" | jq -r '.error // empty' 2>/dev/null || echo "$out")
TOTAL=$((TOTAL + 1))
if [[ -n "$has_error" ]]; then
  PASS=$((PASS + 1))
  echo "  ✓ error on missing file"
else
  FAIL=$((FAIL + 1))
  echo "  ✗ error on missing file"
fi

out=$($CQ join <(echo '[{"k":"a"}]') <(echo '[{"k":"a"}]') -l '.k' 2>&1) || true
has_error=$(echo "$out" | jq -r '.error // empty' 2>/dev/null || echo "$out")
TOTAL=$((TOTAL + 1))
if [[ -n "$has_error" ]]; then
  PASS=$((PASS + 1))
  echo "  ✓ error on missing -r flag"
else
  FAIL=$((FAIL + 1))
  echo "  ✗ error on missing -r flag"
fi

out=$($CQ join <(echo '[{"k":"a"}]') <(echo '[{"k":"a"}]') -l '.k' -r '.k' -t bogus 2>&1) || true
has_error=$(echo "$out" | jq -r '.error // empty' 2>/dev/null || echo "$out")
TOTAL=$((TOTAL + 1))
if [[ -n "$has_error" ]]; then
  PASS=$((PASS + 1))
  echo "  ✓ error on invalid join type"
else
  FAIL=$((FAIL + 1))
  echo "  ✗ error on invalid join type"
fi

# ─── Summary ───
echo ""
echo "═══════════════════════════"
echo "  $PASS / $TOTAL passed ($FAIL failed)"
echo "═══════════════════════════"
[[ $FAIL -eq 0 ]] || exit 1
