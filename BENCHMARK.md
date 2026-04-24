# Benchmarks

This document records the current `fast-robots` benchmark results after adding `mimalloc` to the benchmark target, along with the environment they were captured on.

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

These results were captured after adding benchmark-only `mimalloc`, so Criterion labels use `fast-robots`.

## Parse Throughput

| Benchmark | Median Time | Throughput |
|-----------|-------------|------------|
| `parse/fast-robots/tiny` | 87.760 ns | 369.47 MiB/s |
| `parse/fast-robots/common` | 361.51 ns | 775.58 MiB/s |
| `parse/fast-robots/many_groups` | 79.837 us | 952.67 MiB/s |
| `parse/fast-robots/many_rules` | 58.936 us | 1022.3 MiB/s |
| `parse/fast-robots/wildcard_heavy` | 38.782 us | 1.7258 GiB/s |
| `parse/fast-robots/extension_heavy` | 92.462 us | 1.1004 GiB/s |
| `parse/fast-robots/large_500k` | 772.79 us | 647.09 MiB/s |

## Match Throughput

These benchmarks parse once, then repeatedly call `RobotsTxt::is_allowed()` over a small batch of access checks.

| Benchmark | Median Time | Throughput |
|-----------|-------------|------------|
| `match/fast-robots/many_rules` | 79.143 us | 75.812 Kelem/s |
| `match/fast-robots/wildcard_heavy` | 131.30 us | 45.697 Kelem/s |

## Parse + Match Comparison

This group parses the robots.txt input and immediately checks one path. It compares `fast-robots` with [`robotstxt`](https://crates.io/crates/robotstxt), the Rust port of Google's robots.txt parser and matcher.

This is an API-level comparison, not a claim that the two crates have identical semantics for every edge case.

| Fixture | `fast-robots` Median | `robotstxt` Median | Speedup |
|---------|----------------------|---------------------|---------|
| tiny | 98.884 ns | 375.35 ns | 3.8x |
| common | 384.97 ns | 2.4791 us | 6.4x |
| many_rules | 77.929 us | 651.74 us | 8.4x |
| large_500k | 783.63 us | 4.5773 ms | 5.8x |

Detailed results:

| Benchmark | Median Time | Throughput |
|-----------|-------------|------------|
| `parse_match/fast-robots/tiny` | 98.884 ns | 327.91 MiB/s |
| `parse_match/robotstxt-google-port/tiny` | 375.35 ns | 86.385 MiB/s |
| `parse_match/fast-robots/common` | 384.97 ns | 728.31 MiB/s |
| `parse_match/robotstxt-google-port/common` | 2.4791 us | 113.10 MiB/s |
| `parse_match/fast-robots/many_rules` | 77.929 us | 773.16 MiB/s |
| `parse_match/robotstxt-google-port/many_rules` | 651.74 us | 92.446 MiB/s |
| `parse_match/fast-robots/large_500k` | 783.63 us | 638.14 MiB/s |
| `parse_match/robotstxt-google-port/large_500k` | 4.5773 ms | 109.25 MiB/s |

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
