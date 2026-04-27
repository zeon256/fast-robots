# Benchmarks

This document records the current `fast-robots` benchmark results after parser hot-path optimizations, the opt-in compiled matcher, and benchmark-only `mimalloc`, along with the environment they were captured on.

The benchmark source lives in [`benches/robots.rs`](benches/robots.rs). The implementation under test is mostly in [`src/lib.rs`](src/lib.rs), with crate metadata in [`Cargo.toml`](Cargo.toml) and user-facing docs in [`README.md`](README.md).

## Environment

| Item | Value |
|------|-------|
| Machine | Apple M1 |
| Memory | 8 GiB |
| OS | macOS/Darwin 25.2.0 arm64 |
| Kernel | `Darwin Kernel Version 25.2.0: Tue Nov 18 21:09:55 PST 2025; root:xnu-12377.61.12~1/RELEASE_ARM64_T8103` |
| Rust | `rustc 1.97.0-nightly (bf4fbfb7a 2026-04-11)` |
| Host | `aarch64-apple-darwin` |
| LLVM | 22.1.2 |
| Compiler flags | `RUSTFLAGS='-C target-cpu=native'` |

Command:

```bash
RUSTFLAGS='-C target-cpu=native' cargo bench --bench robots
```

These results were captured after parser hot-path optimizations and adding benchmark-only `mimalloc`, so Criterion labels use `fast-robots`.

## Parse Throughput

| Benchmark | Median Time | Throughput |
|-----------|-------------|------------|
| `parse/fast-robots/tiny` | 71.074 ns | 456.21 MiB/s |
| `parse/fast-robots/common` | 285.700 ns | 981.38 MiB/s |
| `parse/fast-robots/many_groups` | 63.803 us | 1.1641 GiB/s |
| `parse/fast-robots/many_rules` | 48.622 us | 1.2101 GiB/s |
| `parse/fast-robots/wildcard_heavy` | 31.603 us | 2.1179 GiB/s |
| `parse/fast-robots/extension_heavy` | 77.236 us | 1.3173 GiB/s |
| `parse/fast-robots/large_500k` | 643.442 us | 777.18 MiB/s |

## Match Throughput

These benchmarks parse once, then repeatedly run a small batch of access checks. `fast-robots` calls `RobotsTxt::is_allowed()` directly; `fast-robots-compiled` builds `robots.matcher()` before the timed loop and measures repeated checks through the precompiled index.

| Benchmark | Median Time | Throughput |
|-----------|-------------|------------|
| `match/fast-robots/many_rules` | 81.627 us | 73.505 Kelem/s |
| `match/fast-robots-compiled/many_rules` | 26.217 us | 228.857 Kelem/s |
| `match/fast-robots/wildcard_heavy` | 130.401 us | 46.012 Kelem/s |
| `match/fast-robots-compiled/wildcard_heavy` | 71.866 us | 83.489 Kelem/s |

## Parse + Match Comparison

This group parses the robots.txt input and immediately checks one path. It compares `fast-robots` with [`robotstxt`](https://crates.io/crates/robotstxt), the Rust port of Google's robots.txt parser and matcher.

This is an API-level comparison, not a claim that the two crates have identical semantics for every edge case.

| Fixture | `fast-robots` Median | `robotstxt` Median | Speedup |
|---------|----------------------|---------------------|---------|
| tiny | 82.463 ns | 344.206 ns | 4.2x |
| common | 311.850 ns | 2.303 us | 7.4x |
| many_rules | 69.171 us | 619.738 us | 9.0x |
| large_500k | 656.506 us | 4.169 ms | 6.4x |

Detailed results:

| Benchmark | Median Time | Throughput |
|-----------|-------------|------------|
| `parse_match/fast-robots/tiny` | 82.463 ns | 393.21 MiB/s |
| `parse_match/robotstxt-google-port/tiny` | 344.206 ns | 94.20 MiB/s |
| `parse_match/fast-robots/common` | 311.850 ns | 899.09 MiB/s |
| `parse_match/robotstxt-google-port/common` | 2.303 us | 121.74 MiB/s |
| `parse_match/fast-robots/many_rules` | 69.171 us | 871.05 MiB/s |
| `parse_match/robotstxt-google-port/many_rules` | 619.738 us | 97.22 MiB/s |
| `parse_match/fast-robots/large_500k` | 656.506 us | 761.71 MiB/s |
| `parse_match/robotstxt-google-port/large_500k` | 4.169 ms | 119.94 MiB/s |

## Notes

- `RobotsTxt::matcher()` has an upfront indexing cost but improves repeated matching on rule-heavy inputs in this run: about 3.1x for `many_rules` and 1.8x for `wildcard_heavy` versus direct `is_allowed()` in the repeated-match benchmark.
- `fast-robots` intentionally keeps parsing line-oriented and zero-copy over `&str`.
- Delimiter scanning and wildcard segment search use [`memchr`](https://docs.rs/memchr), which selects SIMD implementations on supported targets.
- Native CPU tuning is significant on this machine, especially for large/generated fixtures.
- Criterion reports are generated locally under `target/criterion/` after running `cargo bench`.
- Gnuplot was not installed for this run, so Criterion used the plotters backend.

## Reproducing

Run the complete benchmark suite:

```bash
RUSTFLAGS='-C target-cpu=native' cargo bench --bench robots
```

Run a quick sanity check:

```bash
RUSTFLAGS='-C target-cpu=native' cargo bench --bench robots -- --sample-size 10 --warm-up-time 0.1 --measurement-time 0.2
```

## Flamegraph Profiling

Use [`cargo-flamegraph`](https://github.com/flamegraph-rs/flamegraph) against the focused profiling example instead of the full Criterion suite.

Install once:

```bash
cargo install flamegraph
```

Build/profile with native CPU tuning, frame pointers, and unstripped debug symbols:

```bash
RUSTFLAGS='-C target-cpu=native -C force-frame-pointers=yes' cargo flamegraph --profile flamegraph --example profile -- parse-common
```

The dedicated `flamegraph` profile matters because `[profile.release]` strips
symbols. Setting `CARGO_PROFILE_RELEASE_DEBUG=true` alone still leaves the
profiled binary stripped.

Useful workloads:

```bash
RUSTFLAGS='-C target-cpu=native -C force-frame-pointers=yes' cargo flamegraph --profile flamegraph --example profile -- parse-tiny
RUSTFLAGS='-C target-cpu=native -C force-frame-pointers=yes' cargo flamegraph --profile flamegraph --example profile -- parse-common
RUSTFLAGS='-C target-cpu=native -C force-frame-pointers=yes' cargo flamegraph --profile flamegraph --example profile -- parse-many-groups
RUSTFLAGS='-C target-cpu=native -C force-frame-pointers=yes' cargo flamegraph --profile flamegraph --example profile -- parse-large
RUSTFLAGS='-C target-cpu=native -C force-frame-pointers=yes' cargo flamegraph --profile flamegraph --example profile -- match-many-rules
RUSTFLAGS='-C target-cpu=native -C force-frame-pointers=yes' cargo flamegraph --profile flamegraph --example profile -- parse-match-common
```

On macOS, `cargo flamegraph` may require elevated DTrace permissions. If needed, preserve the environment with `sudo -E env`:

```bash
sudo -E env RUSTFLAGS='-C target-cpu=native -C force-frame-pointers=yes' cargo flamegraph --profile flamegraph --example profile -- parse-common
```
