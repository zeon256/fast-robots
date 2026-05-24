# Compiled Matcher Indexing Follow-up

Date: 2026-05-25

Machine: local Linux workstation. These numbers are comparable to
`2026-05-08-matcher-profiling.md` on this machine, not to the Apple M1 numbers
in `BENCHMARK.md`.

## Direction From Prior Findings

The previous matcher profile showed repeated matching dominated by rule scans:

- compiled `many_rules`: `apply_compiled_rules` and `matching_specificity`
- compiled `wildcard_heavy`: `glob_matches`, `starts_with`, and wildcard
  split/scan work

The recommended next candidates were:

- index or reduce plain prefix rule scans
- precompile wildcard pattern segments

## Change

`RobotsMatcher` now compiles each group into specialized indexes:

- plain non-anchored rules are inserted into a byte trie and checked by walking
  the request path once
- anchored non-wildcard rules are stored as exact lookups
- wildcard rules are indexed by the literal prefix before the first `*`
- wildcard suffix segments are split once at matcher build time

The direct one-off `RobotsTxt::is_allowed()` path is intentionally unchanged.

## Criterion Snapshot

Baseline was captured immediately before the change with:

```bash
env CARGO_BUILD_RUSTC_WRAPPER= RUSTFLAGS='-C target-cpu=native' cargo bench --bench robots match -- --sample-size 10 --warm-up-time 0.1 --measurement-time 0.2
```

Post-change numbers were captured with the same match-group timing settings used
in the prior findings note:

```bash
env CARGO_BUILD_RUSTC_WRAPPER= RUSTFLAGS='-C target-cpu=native' cargo bench --bench robots '^match/' -- --sample-size 20 --warm-up-time 0.5 --measurement-time 1.0
```

| Workload | Before Median | After Median | Result |
|----------|---------------|--------------|--------|
| `match/fast-robots-compiled/many_rules` | 36.564 us | 314.33 ns | 116x faster |
| `match/fast-robots-compiled/wildcard_heavy` | 85.221 us | 318.47 ns | 268x faster |

Direct matching numbers were not targeted by this change.

## Perf Counter Snapshot

Command:

```bash
env CARGO_BUILD_RUSTC_WRAPPER= RUSTFLAGS='-C target-cpu=native -C force-frame-pointers=yes' cargo build --profile flamegraph --example profile
perf stat -d target/flamegraph/examples/profile match-compiled-many-rules 300000
perf stat -d target/flamegraph/examples/profile match-compiled-wildcard-heavy 200000
perf stat -d target/flamegraph/examples/profile matcher-build-many-rules 1000
perf stat -d target/flamegraph/examples/profile matcher-build-wildcard-heavy 1000
```

| Workload | Elapsed | Instructions | Cycles | IPC | Branch Miss Rate | L1D Miss Rate |
|----------|---------|--------------|--------|-----|------------------|---------------|
| compiled `many_rules` | 0.111 s | 1.45B | 461.3M | 3.1 | 0.0% | 0.1% |
| compiled `wildcard_heavy` | 0.075 s | 1.04B | 310.1M | 3.3 | 0.1% | 0.3% |
| build `many_rules` matcher, 1000x | 0.229 s | 2.78B | 971.8M | 2.9 | 0.9% | 5.1% |
| build `wildcard_heavy` matcher, 1000x | 0.279 s | 3.51B | 1.17B | 3.0 | 0.6% | 7.3% |

For comparison, the previous run recorded 10.479 s for compiled `many_rules`
and 19.666 s for compiled `wildcard_heavy` with the same iteration counts.

The indexed matcher does more construction work than the old flat compiled rule
list. It remains intended for repeated checks against a parsed file, not
one-off access decisions.

## Verification

```bash
env CARGO_BUILD_RUSTC_WRAPPER= cargo test
env CARGO_BUILD_RUSTC_WRAPPER= cargo test --no-default-features
env CARGO_BUILD_RUSTC_WRAPPER= cargo clippy --all-targets --all-features
```

Result: all unit tests, doctests, no-default-features tests, and Clippy passed.
