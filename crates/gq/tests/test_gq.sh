#!/usr/bin/env bash
# gq test suite — runs against the aq repo as a fixture
set -euo pipefail

GQ="/Users/develop/local-stacks/gq/bin/gq"
TEST_REPO="/Users/develop/local-stacks/aq"
PASS=0
FAIL=0

pass() { ((PASS++)); echo "  ✓ $1"; }
fail() { ((FAIL++)); echo "  ✗ $1: $2"; }

assert_json() {
  local desc="$1" output="$2"
  if echo "$output" | jq . >/dev/null 2>&1; then
    pass "$desc"
  else
    fail "$desc" "invalid JSON"
  fi
}

assert_count() {
  local desc="$1" output="$2" expected="$3"
  local actual
  actual=$(echo "$output" | jq 'length')
  if [[ "$actual" -eq "$expected" ]]; then
    pass "$desc"
  else
    fail "$desc" "expected $expected, got $actual"
  fi
}

assert_gte() {
  local desc="$1" output="$2" min="$3"
  local actual
  actual=$(echo "$output" | jq 'length')
  if [[ "$actual" -ge "$min" ]]; then
    pass "$desc"
  else
    fail "$desc" "expected >= $min, got $actual"
  fi
}

assert_field() {
  local desc="$1" output="$2" query="$3" expected="$4"
  local actual
  actual=$(echo "$output" | jq -r "$query")
  if [[ "$actual" == "$expected" ]]; then
    pass "$desc"
  else
    fail "$desc" "expected '$expected', got '$actual'"
  fi
}

assert_nonempty() {
  local desc="$1" output="$2"
  if [[ -n "$output" ]] && [[ "$output" != "null" ]] && [[ "$output" != "[]" ]]; then
    pass "$desc"
  else
    fail "$desc" "empty output"
  fi
}

cd "$TEST_REPO"

# ─── --version ───
echo "Testing --version"
out=$($GQ --version)
assert_field "--version returns tool name" "$out" '.tool' "gq"
assert_field "--version returns version" "$out" '.version' "0.1.0"

# ─── --at ───
echo "Testing --at"
out=$($GQ --at HEAD Cargo.toml)
assert_nonempty "--at HEAD Cargo.toml returns content" "$out"
echo "$out" | grep -q '\[workspace\]' && pass "--at contains expected content" || fail "--at contains expected content" "missing [workspace]"

# --at with invalid ref
out=$($GQ --at nonexistentref999 Cargo.toml 2>&1 || true)
echo "$out" | grep -q 'error' && pass "--at invalid ref returns error" || fail "--at invalid ref returns error" "no error"

# ─── --changed-since ───
echo "Testing --changed-since"
out=$($GQ --changed-since 2020-01-01)
assert_json "--changed-since returns JSON" "$out"
assert_gte "--changed-since has results" "$out" 1
# Each entry should have path and change_count
assert_field "--changed-since has path field" "$out" '.[0].path' "$(echo "$out" | jq -r '.[0].path')"
echo "$out" | jq '.[0].change_count' | grep -qE '^[0-9]+$' && pass "--changed-since has change_count" || fail "--changed-since has change_count" "missing"

# ─── --log ───
echo "Testing --log"
out=$($GQ --log -n 3)
assert_json "--log returns JSON" "$out"
assert_count "--log -n 3 returns 3 entries" "$out" 3
# Verify fields
assert_field "--log has hash" "$out" '.[0] | has("hash")' "true"
assert_field "--log has author" "$out" '.[0] | has("author")' "true"
assert_field "--log has date" "$out" '.[0] | has("date")' "true"
assert_field "--log has message" "$out" '.[0] | has("message")' "true"
assert_field "--log has files array" "$out" '.[0].files | type' "array"
# Hash is 8 chars
hash_len=$(echo "$out" | jq -r '.[0].hash | length')
[[ "$hash_len" -eq 8 ]] && pass "--log hash is 8 chars" || fail "--log hash is 8 chars" "got $hash_len"

# ─── --diff ───
echo "Testing --diff"
out=$($GQ --diff HEAD~1..HEAD --files-only)
assert_json "--diff --files-only returns JSON" "$out"
assert_gte "--diff --files-only has results" "$out" 1
assert_field "--diff has status field" "$out" '.[0] | has("status")' "true"
assert_field "--diff has path field" "$out" '.[0] | has("path")' "true"

# Full diff (numstat)
out=$($GQ --diff HEAD~1..HEAD)
assert_json "--diff full returns JSON" "$out"
assert_gte "--diff full has results" "$out" 1
assert_field "--diff has insertions" "$out" '.[0] | has("insertions")' "true"
assert_field "--diff has deletions" "$out" '.[0] | has("deletions")' "true"

# ─── --blame ───
echo "Testing --blame"
out=$($GQ --blame Cargo.toml)
assert_json "--blame returns JSON" "$out"
assert_gte "--blame has results" "$out" 1
assert_field "--blame has line" "$out" '.[0] | has("line")' "true"
assert_field "--blame has hash" "$out" '.[0] | has("hash")' "true"
assert_field "--blame has author" "$out" '.[0] | has("author")' "true"
assert_field "--blame has timestamp" "$out" '.[0] | has("timestamp")' "true"
assert_field "--blame has content" "$out" '.[0] | has("content")' "true"
# Lines should be sorted
first_line=$(echo "$out" | jq '.[0].line')
last_line=$(echo "$out" | jq '.[-1].line')
[[ "$first_line" -le "$last_line" ]] && pass "--blame lines are sorted" || fail "--blame lines are sorted" "$first_line > $last_line"

# ─── --churn ───
echo "Testing --churn"
out=$($GQ --churn)
assert_json "--churn returns JSON" "$out"
assert_gte "--churn has results" "$out" 1
assert_field "--churn has path" "$out" '.[0] | has("path")' "true"
assert_field "--churn has commits" "$out" '.[0] | has("commits")' "true"
assert_field "--churn has insertions" "$out" '.[0] | has("insertions")' "true"
assert_field "--churn has deletions" "$out" '.[0] | has("deletions")' "true"
# Sorted by commits descending
first_commits=$(echo "$out" | jq '.[0].commits')
second_commits=$(echo "$out" | jq '.[1].commits')
[[ "$first_commits" -ge "$second_commits" ]] && pass "--churn sorted by commits desc" || fail "--churn sorted by commits desc" "$first_commits < $second_commits"

# --churn with --since
out=$($GQ --churn --since 2025-03-08)
assert_json "--churn --since returns JSON" "$out"
assert_gte "--churn --since has results" "$out" 1

# ─── Composition: gq | jq ───
echo "Testing composition"
out=$($GQ --log -n 5 | jq '[.[].message]')
assert_json "gq --log | jq messages" "$out"
assert_count "gq --log -n 5 | jq gets 5 messages" "$out" 5

out=$($GQ --churn | jq '[.[:3][].path]')
assert_json "gq --churn | jq top 3 paths" "$out"
assert_count "gq --churn | jq top 3" "$out" 3

# ─── Summary ───
echo ""
echo "Results: $PASS passed, $FAIL failed ($(( PASS + FAIL )) total)"
[[ "$FAIL" -eq 0 ]] && exit 0 || exit 1
