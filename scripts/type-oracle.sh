#!/usr/bin/env bash
# Type oracle — does the value SurrealDB actually returns inhabit the kind the
# analyzer inferred?
#
#   scripts/type-oracle.sh check     gate every mismatch against the baseline
#   scripts/type-oracle.sh update    rewrite the baseline, keeping the verdicts
#   scripts/type-oracle.sh language-tests | corpus    one source, report only
#
# The harness lives in its own workspace at tools/type-oracle so that the
# embedded SurrealDB engine it needs (`kv-mem`, which pulls surrealdb-core and
# about two gigabytes of debug rlib) is never built by `cargo test --workspace`.
# That is also why it is a script rather than a test: it is opt-in, exactly like
# scripts/oracle.py.
#
# `check` and `update` need SurrealDB's language-test corpus. Point at a
# checkout with SURREALDB_REPO, or leave one beside this repository:
#
#   git clone --depth 1 --branch v3.2.3 https://github.com/surrealdb/surrealdb ../surrealdb
#
# The tag matters: the committed baseline records what 3.2.3 returns, and the
# engine dependency is pinned to the same version. Bumping either is a
# deliberate edit whose baseline diff is the report.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
exec cargo run --quiet --manifest-path "$root/tools/type-oracle/Cargo.toml" -- "$@"
