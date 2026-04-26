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
| `parse/fast-robots/tiny` | 77.757 ns | 417.00 MiB/s |
| `parse/fast-robots/common` | 309.599 ns | 905.62 MiB/s |
| `parse/fast-robots/many_groups` | 67.724 us | 1.0967 GiB/s |
| `parse/fast-robots/many_rules` | 53.125 us | 1.1076 GiB/s |
| `parse/fast-robots/wildcard_heavy` | 34.402 us | 1.9456 GiB/s |
| `parse/fast-robots/extension_heavy` | 81.170 us | 1.2535 GiB/s |
| `parse/fast-robots/large_500k` | 663.904 us | 753.22 MiB/s |

## Match Throughput

These benchmarks parse once, then repeatedly run a small batch of access checks. `fast-robots` calls `RobotsTxt::is_allowed()` directly; `fast-robots-compiled` builds `robots.matcher()` before the timed loop and measures repeated checks through the precompiled index.

| Benchmark | Median Time | Throughput |
|-----------|-------------|------------|
| `match/fast-robots/many_rules` | 84.374 us | 71.112 Kelem/s |
| `match/fast-robots-compiled/many_rules` | 27.006 us | 222.173 Kelem/s |
| `match/fast-robots/wildcard_heavy` | 132.148 us | 45.404 Kelem/s |
| `match/fast-robots-compiled/wildcard_heavy` | 73.480 us | 81.655 Kelem/s |

## Parse + Match Comparison

This group parses the robots.txt input and immediately checks one path. It compares `fast-robots` with [`robotstxt`](https://crates.io/crates/robotstxt), the Rust port of Google's robots.txt parser and matcher.

This is an API-level comparison, not a claim that the two crates have identical semantics for every edge case.

| Fixture | `fast-robots` Median | `robotstxt` Median | Speedup |
|---------|----------------------|---------------------|---------|
| tiny | 86.244 ns | 348.524 ns | 4.0x |
| common | 333.243 ns | 2.3094 us | 6.9x |
| many_rules | 73.857 us | 638.990 us | 8.7x |
| large_500k | 678.757 us | 4.4226 ms | 6.5x |

Detailed results:

| Benchmark | Median Time | Throughput |
|-----------|-------------|------------|
| `parse_match/fast-robots/tiny` | 86.244 ns | 375.97 MiB/s |
| `parse_match/robotstxt-google-port/tiny` | 348.524 ns | 93.03 MiB/s |
| `parse_match/fast-robots/common` | 333.243 ns | 841.37 MiB/s |
| `parse_match/robotstxt-google-port/common` | 2.3094 us | 121.41 MiB/s |
| `parse_match/fast-robots/many_rules` | 73.857 us | 815.79 MiB/s |
| `parse_match/robotstxt-google-port/many_rules` | 638.990 us | 94.29 MiB/s |
| `parse_match/fast-robots/large_500k` | 678.757 us | 736.74 MiB/s |
| `parse_match/robotstxt-google-port/large_500k` | 4.4226 ms | 113.07 MiB/s |

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

Build/profile with native CPU tuning, frame pointers, and release debug symbols:

```bash
CARGO_PROFILE_RELEASE_DEBUG=true RUSTFLAGS='-C target-cpu=native -C force-frame-pointers=yes' cargo flamegraph --example profile -- parse-common
```

Useful workloads:

```bash
CARGO_PROFILE_RELEASE_DEBUG=true RUSTFLAGS='-C target-cpu=native -C force-frame-pointers=yes' cargo flamegraph --example profile -- parse-tiny
CARGO_PROFILE_RELEASE_DEBUG=true RUSTFLAGS='-C target-cpu=native -C force-frame-pointers=yes' cargo flamegraph --example profile -- parse-common
CARGO_PROFILE_RELEASE_DEBUG=true RUSTFLAGS='-C target-cpu=native -C force-frame-pointers=yes' cargo flamegraph --example profile -- parse-many-groups
CARGO_PROFILE_RELEASE_DEBUG=true RUSTFLAGS='-C target-cpu=native -C force-frame-pointers=yes' cargo flamegraph --example profile -- parse-large
CARGO_PROFILE_RELEASE_DEBUG=true RUSTFLAGS='-C target-cpu=native -C force-frame-pointers=yes' cargo flamegraph --example profile -- match-many-rules
CARGO_PROFILE_RELEASE_DEBUG=true RUSTFLAGS='-C target-cpu=native -C force-frame-pointers=yes' cargo flamegraph --example profile -- parse-match-common
```

On macOS, `cargo flamegraph` may require elevated DTrace permissions. If needed, preserve the environment with `sudo -E env`:

```bash
sudo -E env CARGO_PROFILE_RELEASE_DEBUG=true RUSTFLAGS='-C target-cpu=native -C force-frame-pointers=yes' cargo flamegraph --example profile -- parse-common
```
