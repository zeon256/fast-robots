# Benchmarks

This document records the current `fast-robots` benchmark results after the fallible parsing and diagnostics changes, along with the environment they were captured on.

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

These results were captured after the package rename, so Criterion labels use `fast-robots`.

## Parse Throughput

| Benchmark | Median Time | Throughput |
|-----------|-------------|------------|
| `parse/fast-robots/tiny` | 129.80 ns | 249.81 MiB/s |
| `parse/fast-robots/common` | 544.96 ns | 514.50 MiB/s |
| `parse/fast-robots/many_groups` | 118.93 us | 639.51 MiB/s |
| `parse/fast-robots/many_rules` | 59.122 us | 1019.1 MiB/s |
| `parse/fast-robots/wildcard_heavy` | 40.218 us | 1.6642 GiB/s |
| `parse/fast-robots/extension_heavy` | 99.977 us | 1.0177 GiB/s |
| `parse/fast-robots/large_500k` | 976.38 us | 512.16 MiB/s |

## Match Throughput

These benchmarks parse once, then repeatedly call `RobotsTxt::is_allowed()` over a small batch of access checks.

| Benchmark | Median Time | Throughput |
|-----------|-------------|------------|
| `match/fast-robots/many_rules` | 80.082 us | 74.924 Kelem/s |
| `match/fast-robots/wildcard_heavy` | 130.93 us | 45.827 Kelem/s |

## Parse + Match Comparison

This group parses the robots.txt input and immediately checks one path. It compares `fast-robots` with [`robotstxt`](https://crates.io/crates/robotstxt), the Rust port of Google's robots.txt parser and matcher.

This is an API-level comparison, not a claim that the two crates have identical semantics for every edge case.

| Fixture | `fast-robots` Median | `robotstxt` Median | Speedup |
|---------|----------------------|---------------------|---------|
| tiny | 142.50 ns | 513.08 ns | 3.6x |
| common | 574.48 ns | 3.6392 us | 6.3x |
| many_rules | 78.762 us | 915.42 us | 11.6x |
| large_500k | 982.88 us | 7.5116 ms | 7.6x |

Detailed results:

| Benchmark | Median Time | Throughput |
|-----------|-------------|------------|
| `parse_match/fast-robots/tiny` | 142.50 ns | 227.55 MiB/s |
| `parse_match/robotstxt-google-port/tiny` | 513.08 ns | 63.197 MiB/s |
| `parse_match/fast-robots/common` | 574.48 ns | 488.06 MiB/s |
| `parse_match/robotstxt-google-port/common` | 3.6392 us | 77.044 MiB/s |
| `parse_match/fast-robots/many_rules` | 78.762 us | 764.98 MiB/s |
| `parse_match/robotstxt-google-port/many_rules` | 915.42 us | 65.818 MiB/s |
| `parse_match/fast-robots/large_500k` | 982.88 us | 508.78 MiB/s |
| `parse_match/robotstxt-google-port/large_500k` | 7.5116 ms | 66.573 MiB/s |

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
