# Benchmarks

This document records the current `fast-robots` benchmark results and the environment they were captured on.

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

These results were captured before the package rename from `robots-simd` to `fast-robots`; the benchmark labels in the raw Criterion output used `robots-simd`. The benchmark target now emits `fast-robots` labels for the same implementation.

## Parse Throughput

| Benchmark | Median Time | Throughput |
|-----------|-------------|------------|
| `parse/fast-robots/tiny` | 122.61 ns | 264.47 MiB/s |
| `parse/fast-robots/common` | 516.13 ns | 543.23 MiB/s |
| `parse/fast-robots/many_groups` | 111.92 us | 679.61 MiB/s |
| `parse/fast-robots/many_rules` | 57.770 us | 1.0185 GiB/s |
| `parse/fast-robots/wildcard_heavy` | 39.014 us | 1.7156 GiB/s |
| `parse/fast-robots/extension_heavy` | 98.638 us | 1.0315 GiB/s |
| `parse/fast-robots/large_500k` | 948.81 us | 527.05 MiB/s |

## Match Throughput

These benchmarks parse once, then repeatedly call `RobotsTxt::is_allowed()` over a small batch of access checks.

| Benchmark | Median Time | Throughput |
|-----------|-------------|------------|
| `match/fast-robots/many_rules` | 79.781 us | 75.206 Kelem/s |
| `match/fast-robots/wildcard_heavy` | 131.08 us | 45.775 Kelem/s |

## Parse + Match Comparison

This group parses the robots.txt input and immediately checks one path. It compares `fast-robots` with [`robotstxt`](https://crates.io/crates/robotstxt), the Rust port of Google's robots.txt parser and matcher.

This is an API-level comparison, not a claim that the two crates have identical semantics for every edge case.

| Fixture | `fast-robots` Median | `robotstxt` Median | Speedup |
|---------|----------------------|---------------------|---------|
| tiny | 132.88 ns | 516.14 ns | 3.9x |
| common | 539.32 ns | 3.6445 us | 6.8x |
| many_rules | 76.952 us | 918.74 us | 11.9x |
| large_500k | 958.84 us | 7.5167 ms | 7.8x |

Detailed results:

| Benchmark | Median Time | Throughput |
|-----------|-------------|------------|
| `parse_match/fast-robots/tiny` | 132.88 ns | 244.02 MiB/s |
| `parse_match/robotstxt-google-port/tiny` | 516.14 ns | 62.822 MiB/s |
| `parse_match/fast-robots/common` | 539.32 ns | 519.88 MiB/s |
| `parse_match/robotstxt-google-port/common` | 3.6445 us | 76.932 MiB/s |
| `parse_match/fast-robots/many_rules` | 76.952 us | 782.98 MiB/s |
| `parse_match/robotstxt-google-port/many_rules` | 918.74 us | 65.581 MiB/s |
| `parse_match/fast-robots/large_500k` | 958.84 us | 521.54 MiB/s |
| `parse_match/robotstxt-google-port/large_500k` | 7.5167 ms | 66.528 MiB/s |

## Notes

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
