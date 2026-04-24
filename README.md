# fast-robots

[![Crates.io](https://img.shields.io/crates/v/fast-robots)](https://crates.io/crates/fast-robots)
[![Crates.io Downloads](https://img.shields.io/crates/d/fast-robots)](https://crates.io/crates/fast-robots)
[![Docs.rs](https://img.shields.io/docsrs/fast-robots)](https://docs.rs/fast-robots)
[![License](https://img.shields.io/badge/license-Apache--2.0%2FMIT-blue)](#license)
[![MSRV](https://img.shields.io/badge/MSRV-1.85.1-orange)](https://blog.rust-lang.org/2025/02/20/Rust-1.85.0/)
[![Rust Edition](https://img.shields.io/badge/Rust-2024-blue)](https://doc.rust-lang.org/edition-guide/rust-2024/)
[![SIMD](https://img.shields.io/badge/SIMD-memchr-success)](https://docs.rs/memchr)
[![CLI](https://img.shields.io/badge/CLI-argh-informational)](https://github.com/google/argh)

A zero-copy `robots.txt` parser for Rust with SIMD-accelerated byte scanning, RFC 9309 access checks, feature-gated extension metadata, and a tiny `argh` CLI.

## Motivation

`robots.txt` is line-oriented and byte-oriented. That makes a hand-rolled parser a better fit than a big parser-combinator stack: fewer allocations, direct control over error recovery, and the hot path stays obvious.

The goal is simple: parse the standardized rules correctly, preserve useful ecosystem metadata like `Sitemap` and `Crawl-delay`, and use `memchr` where delimiter scanning actually matters.

## Features

- **Zero-copy parsing**: parsed agents, rules, and extension values borrow from the original input.
- **SIMD-backed scanning**: line splitting, comments, directive separators, and wildcard matching use `memchr`/`memmem` primitives.
- **RFC 9309 core**:
  - `User-agent`
  - `Allow`
  - `Disallow`
  - `#` comments
  - `*` wildcard matching
  - `$` end-anchor matching
- **Correct access semantics**:
  - matching groups are merged
  - `*` fallback group is used only when no exact user-agent group matches
  - longest matching rule wins
  - `Allow` wins ties
  - empty `Disallow:` does not block anything
  - `/robots.txt` is implicitly allowed
- **Feature-gated extensions**: `Sitemap`, `Crawl-delay`, `Host`, `Clean-param`, and unknown directives are collected behind the `extensions` feature.
- **CLI included**: inspect parsed files and check whether a path is allowed from the terminal.
- **Small dependency surface**: runtime dependencies are currently `memchr` and `argh`.

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
fast-robots = "0.1.0"
```

The `extensions` feature is enabled by default:

```toml
[dependencies]
fast-robots = { version = "0.1.0", default-features = false }
```

## Usage

```rust
use fast_robots::RobotsTxt;

let input = r#"
User-agent: *
Disallow: /private/
Allow: /private/public/
Sitemap: https://example.com/sitemap.xml
"#;

let robots = RobotsTxt::parse(input);

assert!(!robots.is_allowed("ExampleBot", "/private/file.html"));
assert!(robots.is_allowed("ExampleBot", "/private/public/file.html"));
```

### Extensions

With the default `extensions` feature, non-core records are preserved as metadata:

```rust
use fast_robots::RobotsTxt;

let robots = RobotsTxt::parse(r#"
Sitemap: https://example.com/sitemap.xml
User-agent: Bingbot
Crawl-delay: 5
Disallow: /slow/
Host: example.com
Clean-param: ref /shop
X-Experimental: yes
"#);

assert_eq!(robots.extensions.sitemaps, ["https://example.com/sitemap.xml"]);
assert_eq!(robots.extensions.crawl_delays[0].agents, ["Bingbot"]);
assert_eq!(robots.extensions.crawl_delays[0].value, "5");
assert!(!robots.is_allowed("Bingbot", "/slow/page.html"));
```

Extensions are metadata only. They do not affect `is_allowed()`.

### CLI

Parse a file:

```bash
cargo run -- parse robots.txt
```

Check a path:

```bash
cargo run -- check robots.txt --agent Googlebot --path /private/page.html
```

Exit codes for `check`:

- `0`: allowed
- `1`: disallowed
- `2`: file read error

## How it works

1. **Line scan**: the parser walks the input with `memchr(b'\n', ...)` and strips optional `\r`.
2. **Comment scan**: `memchr(b'#', ...)` removes inline comments.
3. **Directive split**: `memchr(b':', ...)` separates key/value records.
4. **Core parse**: `user-agent`, `allow`, and `disallow` are matched ASCII-case-insensitively.
5. **Extension collection**: when enabled, non-core records are stored without changing group boundaries.
6. **Access check**: matching groups are evaluated using longest-match semantics, with `Allow` preferred on equal specificity.

## Why not nom?

`nom` is good, but this format is mostly delimiter scanning and small state transitions. A manual parser keeps the important choices visible:

- which bytes are scanned with SIMD-backed routines
- how malformed lines recover
- when groups start and end
- which records are access-control rules versus metadata
- how much allocation happens

Parser combinators can still be useful for more complex formats. Here they would mostly hide a simple loop.

## Extension Semantics

`fast-robots` treats extensions conservatively:

- `Sitemap`: global metadata; can appear anywhere.
- `Crawl-delay`: stored with the current group agents when present.
- `Host`: stored as Yandex-style metadata.
- `Clean-param`: stored as Yandex-style metadata.
- unknown directives: stored as `Directive { key, value }`.

Other records must not terminate groups or interfere with RFC 9309 parsing.

## Building

```bash
cargo build
cargo test
cargo test --no-default-features
cargo clippy --all-targets --all-features
```

## Verification

<details>
<summary>Quick Checks (click to expand)</summary>

### Parse and inspect a file

```bash
cargo run -- parse robots.txt
```

### Check an allow/disallow decision

```bash
cargo run -- check robots.txt --agent ExampleBot --path /admin/
```

### Verify the extension gate

```bash
cargo test
cargo test --no-default-features
```

### Verify lint cleanliness

```bash
cargo fmt --check
cargo clippy --all-targets --all-features
```

</details>

## Benchmarks

Benchmarks use Criterion.rs and generated fixtures so large test data does not need to live in the repository. Current results are tracked in [BENCHMARK.md](BENCHMARK.md).

Current benchmark groups:

| Group | Workload | Goal |
|-------|----------|------|
| `parse` | tiny, common, many groups, many rules, wildcard-heavy, extension-heavy, 500 KiB | parser throughput |
| `match` | many rules, wildcard-heavy | `is_allowed()` throughput after parsing once |
| `parse_match` | tiny, common, many rules, 500 KiB | end-to-end parse plus access decision |

The `parse_match` group compares `fast-robots` against `robotstxt`, the Rust port of Google's robots.txt parser and matcher. This is an API-level comparison, not a claim that the two crates currently have identical behavior for every edge case.

Run all benchmarks:

```bash
cargo bench
```

Run only this crate's benchmark target:

```bash
cargo bench --bench robots
```

Quick local sanity check with a smaller sample size:

```bash
cargo bench --bench robots -- --sample-size 10 --warm-up-time 0.1 --measurement-time 0.2
```

## Caveats

- **Not an authorization system**: `robots.txt` is a crawler cooperation protocol, not access control.
- **Input is `&str` today**: the current API assumes UTF-8 input. A future `parse_bytes(&[u8])` API can handle invalid encoding explicitly.
- **No URI percent-normalization yet**: RFC 9309 has specific percent-encoding comparison rules. The current matcher focuses on path pattern semantics and should grow a normalization layer before claiming full crawler equivalence.
- **Extensions vary by crawler**: Google ignores `Crawl-delay`; Bing honors it; other crawlers differ. This crate stores extension metadata but does not enforce crawl scheduling.
- **SIMD is delegated**: `memchr` selects optimized implementations where supported and falls back safely elsewhere.

## Choosing Strictness

| Mode | Cargo config | Use case |
|------|--------------|----------|
| Core + extensions | `fast-robots = "0.1"` | most applications that want sitemaps and metadata |
| Core only | `fast-robots = { version = "0.1", default-features = false }` | strict RFC access checks with less metadata |

## Security

Please see [SECURITY.md](SECURITY.md) for vulnerability reporting.

## License

Licensed under either of:

- **Apache License, Version 2.0** ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- **MIT license** ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
