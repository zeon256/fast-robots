# Matcher Profiling Findings

Date: 2026-05-08

Machine: local Linux workstation. These numbers should not be compared directly with the MacBook M1 Air numbers in `BENCHMARK.md`.

## Context

The goal was to follow a 1BRC-style measurement loop before choosing any matcher optimization:

- add focused profiling workloads
- benchmark direct and compiled matching locally
- inspect where time is actually going
- avoid optimizing plausible but unproven costs

## Measurement-Only Workloads Added

`examples/profile.rs` now includes:

- `match-compiled-many-rules`
- `match-compiled-wildcard-heavy`
- `matcher-build-many-rules`
- `matcher-build-wildcard-heavy`

These are profiling helpers only. They do not change library behavior.

## Criterion Snapshot

Command:

```bash
RUSTFLAGS='-C target-cpu=native' cargo bench --bench robots match -- --sample-size 20 --warm-up-time 0.5 --measurement-time 1.0
```

Relevant local results:

| Workload | Median Time | Throughput |
|----------|-------------|------------|
| `match/fast-robots/many_rules` | 73.744 us | 81.362 Kelem/s |
| `match/fast-robots-compiled/many_rules` | 36.395 us | 164.86 Kelem/s |
| `match/fast-robots/wildcard_heavy` | 123.73 us | 48.491 Kelem/s |
| `match/fast-robots-compiled/wildcard_heavy` | 83.919 us | 71.497 Kelem/s |

Compiled matcher speedups on this machine:

- `many_rules`: about 2.0x faster than direct matching
- `wildcard_heavy`: about 1.47x faster than direct matching

## Perf Counter Snapshot

Focused runs used the already-built profiling binary:

```bash
RUSTFLAGS='-C target-cpu=native -C force-frame-pointers=yes' cargo build --profile flamegraph --example profile
perf stat -d target/flamegraph/examples/profile match-many-rules 300000
perf stat -d target/flamegraph/examples/profile match-wildcard-heavy 200000
perf stat -d target/flamegraph/examples/profile match-compiled-many-rules 300000
perf stat -d target/flamegraph/examples/profile match-compiled-wildcard-heavy 200000
```

| Workload | Elapsed | Instructions | Cycles | IPC | Branch Miss Rate | L1D Miss Rate |
|----------|---------|--------------|--------|-----|------------------|---------------|
| direct `many_rules` | 20.953 s | 351.2B | 92.1B | 3.8 | 0.3% | 2.9% |
| direct `wildcard_heavy` | 27.856 s | 456.0B | 122.4B | 3.7 | 0.1% | 1.4% |
| compiled `many_rules` | 10.479 s | 122.2B | 46.2B | 2.6 | 0.5% | 6.6% |
| compiled `wildcard_heavy` | 19.666 s | 281.9B | 86.5B | 3.3 | 0.2% | 2.2% |

## Hotspot Findings

Direct `many_rules`:

- `apply_group_rules`: about 99.8%
- `matching_specificity`: about 99.0%
- `starts_with`: about 3.0%
- user-agent case comparison was negligible

Direct `wildcard_heavy`:

- `apply_group_rules`: about 98.2%
- `matching_specificity`: about 96.9%
- `glob_matches`: about 74.5%
- wildcard split/scan iterator path: about 37%
- `starts_with`: about 36%

Compiled `many_rules`:

- `apply_compiled_rules`: about 98.9%
- `matching_specificity`: about 92.3%
- `to_ascii_lowercase`: about 0.3%
- `HashMap` lookup and hashing: under 1%

Compiled `wildcard_heavy`:

- `apply_compiled_rules`: about 97.5%
- `matching_specificity`: about 95.6%
- `glob_matches`: about 93.6%
- `starts_with`: about 51.0%
- wildcard split/scan iterator path: about 40.6%
- `to_ascii_lowercase`: negligible

## Conclusions

The initial hypothesis that per-query lowercase allocation was the highest-ROI target was not supported by profiling. It is measurable but not material for these workloads.

Most remaining time is real rule matching work. Further gains are likely algorithmic rather than micro-optimizations.

Best candidates if optimization continues:

- index or otherwise reduce plain prefix rule scans for `many_rules`
- precompile wildcard pattern segments for `wildcard_heavy`
- add a real-corpus repeated-match benchmark before investing heavily in either

Pragmatic recommendation: pause further matcher micro-optimization unless real users need repeated checks against large rule sets. The compiled matcher already gives useful speedups, and the next wins will add representation complexity.

## Profiling Note

Full `cargo flamegraph`/`perf record` runs produced large temporary `perf.data` files and were heavy on this machine. For future investigations, prefer `perf stat -d` and targeted short recordings before generating full flamegraphs.
