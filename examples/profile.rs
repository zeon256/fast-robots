use std::{env, hint::black_box, process::ExitCode};

use fast_robots::RobotsTxt;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> ExitCode {
    let args = env::args().collect::<Vec<_>>();
    let Some(workload) = args.get(1).map(String::as_str) else {
        print_usage(&args[0]);
        return ExitCode::from(2);
    };
    let iterations = match args.get(2) {
        Some(value) => match value.parse::<usize>() {
            Ok(iterations) => Some(iterations),
            Err(error) => {
                eprintln!("invalid iteration count: {error}");
                return ExitCode::from(2);
            }
        },
        None => None,
    };

    let result = match workload {
        "parse-tiny" => run_parse(tiny_fixture(), iterations.unwrap_or(5_000_000)),
        "parse-common" => run_parse(common_fixture(), iterations.unwrap_or(2_000_000)),
        "parse-many-groups" => run_parse(many_groups_fixture(), iterations.unwrap_or(20_000)),
        "parse-many-rules" => run_parse(many_rules_fixture(), iterations.unwrap_or(30_000)),
        "parse-wildcard-heavy" => run_parse(wildcard_heavy_fixture(), iterations.unwrap_or(40_000)),
        "parse-extension-heavy" => {
            run_parse(extension_heavy_fixture(), iterations.unwrap_or(20_000))
        }
        "parse-large" => run_parse(large_500k_fixture(), iterations.unwrap_or(2_000)),
        "match-many-rules" => run_match(many_rules_fixture(), iterations.unwrap_or(300_000)),
        "match-wildcard-heavy" => {
            run_match(wildcard_heavy_fixture(), iterations.unwrap_or(200_000))
        }
        "parse-match-common" => run_parse_match(common_fixture(), iterations.unwrap_or(1_000_000)),
        "parse-match-large" => run_parse_match(large_500k_fixture(), iterations.unwrap_or(2_000)),
        _ => {
            print_usage(&args[0]);
            return ExitCode::from(2);
        }
    };

    println!("{result}");
    ExitCode::SUCCESS
}

fn print_usage(program: &str) {
    eprintln!("usage: {program} <workload> [iterations]");
    eprintln!("workloads:");
    for workload in vec![
        "parse-tiny",
        "parse-common",
        "parse-many-groups",
        "parse-many-rules",
        "parse-wildcard-heavy",
        "parse-extension-heavy",
        "parse-large",
        "match-many-rules",
        "match-wildcard-heavy",
        "parse-match-common",
        "parse-match-large",
    ] {
        eprintln!("  {workload}");
    }
}

fn run_parse(input: String, iterations: usize) -> usize {
    let mut total = 0;
    for _ in 0..iterations {
        let robots = RobotsTxt::parse(black_box(input.as_str()));
        total += black_box(robots.groups.len());
    }
    total
}

fn run_match(input: String, iterations: usize) -> usize {
    let robots = RobotsTxt::parse(&input);
    let queries = vec![
        ("ExampleBot", "/private/0/page.html"),
        ("ExampleBot", "/private/10/public/file.html"),
        ("ExampleBot", "/assets/alpha/private/image.gif"),
        ("ExampleBot", "/assets/alpha/private/image.gif?size=large"),
        ("OtherBot", "/fallback/blocked"),
        ("OtherBot", "/"),
    ];

    let mut total = 0;
    for _ in 0..iterations {
        for (agent, path) in &queries {
            total += usize::from(black_box(
                robots.is_allowed(black_box(agent), black_box(path)),
            ));
        }
    }
    total
}

fn run_parse_match(input: String, iterations: usize) -> usize {
    let mut total = 0;
    for _ in 0..iterations {
        let robots = RobotsTxt::parse(black_box(input.as_str()));
        total += usize::from(black_box(
            robots.is_allowed(black_box("ExampleBot"), black_box("/private/10/page.html")),
        ));
    }
    total
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
