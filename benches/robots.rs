use std::error::Error;
use std::fs;
use std::hint::black_box;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use fast_robots::{RobotsMatcher, RobotsTxt};
use flate2::read::GzDecoder;
use robotstxt::DefaultMatcher;
use sha2::{Digest, Sha256};

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

const REAL_CORPUS_ENV: &str = "FAST_ROBOTS_REAL_CORPUS";
const REAL_CORPUS_URL: &str = "https://raw.githubusercontent.com/nzrsky/robotstxt-benchmark-data/335730ed4ac3a4b64afe6a8a92c5e9eec59d704a/robots_all.bin.gz";
const REAL_CORPUS_GZ_BYTES: usize = 1_489_845;
const REAL_CORPUS_GZ_SHA256: &str =
    "1e14cdc25afb5376e66064d0c106bc44820892614b16255c2861dca2a6aa3a03";
const REAL_CORPUS_BIN_BYTES: usize = 9_517_428;
const REAL_CORPUS_CONTENT_BYTES: usize = 9_489_976;
const REAL_CORPUS_BIN_SHA256: &str =
    "31f51d142a14f249745688be682a25d960f7a83ae3141b584868b4703c23b6d1";
const REAL_CORPUS_RECORDS: usize = 6_863;

struct Fixture {
    name: &'static str,
    input: String,
}

struct RealCorpus {
    inputs: Vec<String>,
    content_bytes: usize,
}

fn bench_parse(c: &mut Criterion) {
    let fixtures = parse_fixtures();
    let mut group = c.benchmark_group("parse");

    for fixture in &fixtures {
        group.throughput(Throughput::Bytes(fixture.input.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("fast-robots", fixture.name),
            fixture.input.as_str(),
            |b, input| b.iter(|| black_box(RobotsTxt::parse(black_box(input)))),
        );
    }

    group.finish();
}

fn bench_match(c: &mut Criterion) {
    let fixtures = [
        Fixture {
            name: "many_rules",
            input: many_rules_fixture(),
        },
        Fixture {
            name: "wildcard_heavy",
            input: wildcard_heavy_fixture(),
        },
    ];
    let queries = [
        ("ExampleBot", "/private/0/page.html"),
        ("ExampleBot", "/private/10/public/file.html"),
        ("ExampleBot", "/assets/alpha/private/image.gif"),
        ("ExampleBot", "/assets/alpha/private/image.gif?size=large"),
        ("OtherBot", "/fallback/blocked"),
        ("OtherBot", "/"),
    ];

    let mut group = c.benchmark_group("match");
    group.throughput(Throughput::Elements(queries.len() as u64));

    for fixture in &fixtures {
        let robots = RobotsTxt::parse(&fixture.input);
        group.bench_with_input(
            BenchmarkId::new("fast-robots", fixture.name),
            &robots,
            |b, robots| b.iter(|| black_box(match_batch(black_box(robots), black_box(&queries)))),
        );

        let matcher = robots.matcher();
        group.bench_with_input(
            BenchmarkId::new("fast-robots-compiled", fixture.name),
            &matcher,
            |b, matcher| {
                b.iter(|| {
                    black_box(match_batch_compiled(
                        black_box(matcher),
                        black_box(&queries),
                    ))
                })
            },
        );
    }

    group.finish();
}

fn bench_parse_match(c: &mut Criterion) {
    let fixtures = [
        Fixture {
            name: "tiny",
            input: tiny_fixture(),
        },
        Fixture {
            name: "common",
            input: common_fixture(),
        },
        Fixture {
            name: "many_rules",
            input: many_rules_fixture(),
        },
        Fixture {
            name: "large_500k",
            input: large_500k_fixture(),
        },
    ];
    let agent = "ExampleBot";
    let path = "/private/10/page.html";
    let url = "https://example.com/private/10/page.html";

    let mut group = c.benchmark_group("parse_match");

    for fixture in &fixtures {
        group.throughput(Throughput::Bytes(fixture.input.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("fast-robots", fixture.name),
            fixture.input.as_str(),
            |b, input| {
                b.iter(|| {
                    let robots = RobotsTxt::parse(black_box(input));
                    black_box(robots.is_allowed(black_box(agent), black_box(path)))
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("robotstxt-google-port", fixture.name),
            fixture.input.as_str(),
            |b, input| {
                let mut matcher = DefaultMatcher::default();
                b.iter(|| {
                    black_box(matcher.one_agent_allowed_by_robots(
                        black_box(input),
                        black_box(agent),
                        black_box(url),
                    ))
                })
            },
        );
    }

    group.finish();
}

fn bench_real_corpus(c: &mut Criterion) {
    if std::env::var_os(REAL_CORPUS_ENV).is_none() {
        eprintln!(
            "skipping real_corpus benchmarks; set {REAL_CORPUS_ENV}=1 to download/cache the pinned nzrsky corpus"
        );
        return;
    }

    let corpus =
        load_real_corpus().unwrap_or_else(|error| panic!("failed to load real corpus: {error}"));
    let agent = "Googlebot";
    let path = "/";
    let mut group = c.benchmark_group("real_corpus");
    group.throughput(Throughput::Bytes(corpus.content_bytes as u64));

    group.bench_with_input(
        BenchmarkId::new("fast-robots-parse", "nzrsky_6863"),
        &corpus.inputs,
        |b, inputs| {
            b.iter(|| {
                let parsed_groups = inputs
                    .iter()
                    .map(|input| RobotsTxt::parse(black_box(input.as_str())).groups.len())
                    .sum::<usize>();
                black_box(parsed_groups)
            })
        },
    );

    group.bench_with_input(
        BenchmarkId::new("fast-robots-parse-match", "nzrsky_6863"),
        &corpus.inputs,
        |b, inputs| {
            b.iter(|| {
                let allowed = inputs
                    .iter()
                    .filter(|input| {
                        let robots = RobotsTxt::parse(black_box(input.as_str()));
                        robots.is_allowed(black_box(agent), black_box(path))
                    })
                    .count();
                black_box(allowed)
            })
        },
    );

    group.finish();
}

fn load_real_corpus() -> Result<RealCorpus, Box<dyn Error>> {
    let path = ensure_real_corpus()?;
    let bytes = fs::read(&path)?;
    verify_bytes(
        "cached real corpus",
        &bytes,
        REAL_CORPUS_BIN_BYTES,
        REAL_CORPUS_BIN_SHA256,
    )?;
    parse_real_corpus(&bytes).map_err(Into::into)
}

fn ensure_real_corpus() -> Result<PathBuf, Box<dyn Error>> {
    let path = real_corpus_path();

    if path.exists() {
        let bytes = fs::read(&path)?;
        if verify_bytes(
            "cached real corpus",
            &bytes,
            REAL_CORPUS_BIN_BYTES,
            REAL_CORPUS_BIN_SHA256,
        )
        .is_ok()
        {
            return Ok(path);
        }

        eprintln!(
            "cached real corpus at {} failed validation; refreshing",
            path.display()
        );
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let compressed = download_real_corpus()?;
    verify_bytes(
        "compressed real corpus",
        &compressed,
        REAL_CORPUS_GZ_BYTES,
        REAL_CORPUS_GZ_SHA256,
    )?;

    let mut decoder = GzDecoder::new(&compressed[..]);
    let mut decompressed = Vec::with_capacity(REAL_CORPUS_BIN_BYTES);
    decoder.read_to_end(&mut decompressed)?;
    verify_bytes(
        "decompressed real corpus",
        &decompressed,
        REAL_CORPUS_BIN_BYTES,
        REAL_CORPUS_BIN_SHA256,
    )?;

    let temp_path = path.with_extension("bin.tmp");
    fs::write(&temp_path, &decompressed)?;
    if path.exists() {
        fs::remove_file(&path)?;
    }
    fs::rename(temp_path, &path)?;

    Ok(path)
}

fn download_real_corpus() -> Result<Vec<u8>, Box<dyn Error>> {
    eprintln!("downloading real corpus from {REAL_CORPUS_URL}");
    let mut response = ureq::get(REAL_CORPUS_URL).call()?;
    let bytes = response
        .body_mut()
        .with_config()
        .limit((REAL_CORPUS_GZ_BYTES + 1024) as u64)
        .read_to_vec()?;
    Ok(bytes)
}

fn parse_real_corpus(bytes: &[u8]) -> io::Result<RealCorpus> {
    let mut inputs = Vec::with_capacity(REAL_CORPUS_RECORDS);
    let mut content_bytes = 0usize;
    let mut offset = 0usize;

    while offset < bytes.len() {
        let record_index = inputs.len();
        if bytes.len() - offset < 4 {
            return Err(invalid_data(format!(
                "record {record_index} has truncated length prefix"
            )));
        }

        let length = u32::from_le_bytes(
            bytes[offset..offset + 4]
                .try_into()
                .expect("length prefix slice has four bytes"),
        ) as usize;
        offset += 4;

        let end = offset
            .checked_add(length)
            .ok_or_else(|| invalid_data(format!("record {record_index} length overflows usize")))?;
        if end > bytes.len() {
            return Err(invalid_data(format!(
                "record {record_index} length {length} exceeds remaining corpus bytes"
            )));
        }

        let input = std::str::from_utf8(&bytes[offset..end]).map_err(|error| {
            invalid_data(format!("record {record_index} is not valid UTF-8: {error}"))
        })?;
        inputs.push(input.to_owned());
        content_bytes += length;
        offset = end;
    }

    if inputs.len() != REAL_CORPUS_RECORDS {
        return Err(invalid_data(format!(
            "expected {REAL_CORPUS_RECORDS} records, found {}",
            inputs.len()
        )));
    }

    if content_bytes != REAL_CORPUS_CONTENT_BYTES {
        return Err(invalid_data(format!(
            "expected {REAL_CORPUS_CONTENT_BYTES} content bytes, found {content_bytes}"
        )));
    }

    Ok(RealCorpus {
        inputs,
        content_bytes,
    })
}

fn verify_bytes(
    label: &str,
    bytes: &[u8],
    expected_len: usize,
    expected_sha256: &str,
) -> io::Result<()> {
    if bytes.len() != expected_len {
        return Err(invalid_data(format!(
            "{label} size mismatch: expected {expected_len} bytes, found {}",
            bytes.len()
        )));
    }

    let actual_sha256 = sha256_hex(bytes);
    if actual_sha256 != expected_sha256 {
        return Err(invalid_data(format!(
            "{label} SHA256 mismatch: expected {expected_sha256}, found {actual_sha256}"
        )));
    }

    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn real_corpus_path() -> PathBuf {
    if let Some(target_dir) = std::env::var_os("CARGO_TARGET_DIR") {
        return PathBuf::from(target_dir)
            .join("bench-data")
            .join("robots_all.bin");
    }

    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("bench-data")
        .join("robots_all.bin")
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn parse_fixtures() -> Vec<Fixture> {
    vec![
        Fixture {
            name: "tiny",
            input: tiny_fixture(),
        },
        Fixture {
            name: "common",
            input: common_fixture(),
        },
        Fixture {
            name: "many_groups",
            input: many_groups_fixture(),
        },
        Fixture {
            name: "many_rules",
            input: many_rules_fixture(),
        },
        Fixture {
            name: "wildcard_heavy",
            input: wildcard_heavy_fixture(),
        },
        Fixture {
            name: "extension_heavy",
            input: extension_heavy_fixture(),
        },
        Fixture {
            name: "large_500k",
            input: large_500k_fixture(),
        },
    ]
}

fn match_batch(robots: &RobotsTxt<'_>, queries: &[(&str, &str)]) -> usize {
    queries
        .iter()
        .filter(|(agent, path)| robots.is_allowed(agent, path))
        .count()
}

fn match_batch_compiled(matcher: &RobotsMatcher<'_>, queries: &[(&str, &str)]) -> usize {
    queries
        .iter()
        .filter(|(agent, path)| matcher.is_allowed(agent, path))
        .count()
}

fn tiny_fixture() -> String {
    "User-agent: *\nDisallow: /private/\n".to_owned()
}

fn common_fixture() -> String {
    r#"
# Common robots.txt shape.
Sitemap: https://example.com/sitemap.xml
User-agent: *
Disallow: /private/
Disallow: /tmp/
Allow: /private/public/

User-agent: ExampleBot
Disallow: /private/10/
Allow: /private/10/public/
Crawl-delay: 5

User-agent: ImageBot
Disallow: /*.gif$
Allow: /public/*.gif$
"#
    .to_owned()
}

fn many_groups_fixture() -> String {
    let mut input = String::new();
    input.push_str("Sitemap: https://example.com/sitemap.xml\n");

    for index in 0..1_000 {
        input.push_str(&format!(
            "User-agent: Bot{index}\nDisallow: /bot/{index}/private/\nAllow: /bot/{index}/private/public/\n\n"
        ));
    }

    input.push_str("User-agent: *\nDisallow: /fallback/blocked\n");
    input
}

fn many_rules_fixture() -> String {
    let mut input = String::new();
    input.push_str("User-agent: ExampleBot\n");

    for index in 0..2_000 {
        input.push_str(&format!("Disallow: /private/{index}/\n"));
        if index % 4 == 0 {
            input.push_str(&format!("Allow: /private/{index}/public/\n"));
        }
    }

    input.push_str("\nUser-agent: *\nDisallow: /fallback/blocked\n");
    input
}

fn wildcard_heavy_fixture() -> String {
    let mut input = String::new();
    input.push_str("User-agent: ExampleBot\n");

    for index in 0..1_000 {
        input.push_str(&format!(
            "Disallow: /assets/{index}/*/private/*.gif$\nAllow: /assets/{index}/public/*.gif$\n"
        ));
    }

    input.push_str("Disallow: /assets/*/private/*.gif$\nAllow: /assets/public/*.gif$\n");
    input
}

fn extension_heavy_fixture() -> String {
    let mut input = String::new();

    for index in 0..1_000 {
        input.push_str(&format!(
            "Sitemap: https://cdn{index}.example.com/sitemap.xml\nX-Meta-{index}: value-{index}\n"
        ));
    }

    input.push_str("User-agent: ExampleBot\n");
    for index in 0..500 {
        input.push_str(&format!(
            "Crawl-delay: {}\nClean-param: ref{} /shop\nHost: example.com\nDisallow: /ext/{index}/\n",
            (index % 20) + 1,
            index
        ));
    }

    input
}

fn large_500k_fixture() -> String {
    let mut input = String::with_capacity(512 * 1024 + 1024);
    input.push_str("Sitemap: https://example.com/sitemap.xml\n");

    let mut index = 0;
    while input.len() < 512 * 1024 {
        input.push_str(&format!(
            "User-agent: Bot{index}\nDisallow: /private/{index}/\nAllow: /private/{index}/public/\nCrawl-delay: {}\nX-Trace: value-{index}\n\n",
            (index % 20) + 1
        ));
        index += 1;
    }

    input
}

criterion_group!(
    benches,
    bench_parse,
    bench_match,
    bench_parse_match,
    bench_real_corpus
);
criterion_main!(benches);
