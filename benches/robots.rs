use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use fast_robots::RobotsTxt;
use robotstxt::DefaultMatcher;

struct Fixture {
    name: &'static str,
    input: String,
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

criterion_group!(benches, bench_parse, bench_match, bench_parse_match);
criterion_main!(benches);
