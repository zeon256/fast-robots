use memchr::{memchr, memmem};
use thiserror::Error;

pub const DEFAULT_MAX_BYTES: usize = 512 * 1024;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("robots.txt is not valid UTF-8")]
    Utf8(#[from] std::str::Utf8Error),

    #[error("robots.txt is too large: {len} bytes exceeds limit of {max} bytes")]
    TooLarge { len: usize, max: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseOptions {
    pub max_bytes: Option<usize>,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            max_bytes: Some(DEFAULT_MAX_BYTES),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseReport<'a> {
    pub robots: RobotsTxt<'a>,
    pub warnings: Vec<ParseWarning<'a>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseWarning<'a> {
    pub line: usize,
    pub kind: ParseWarningKind<'a>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseWarningKind<'a> {
    MissingSeparator { line: &'a str },
    EmptyDirectiveKey,
    EmptyUserAgent,
    RuleBeforeUserAgent { key: &'a str },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RobotsTxt<'a> {
    pub groups: Vec<Group<'a>>,
    #[cfg(feature = "extensions")]
    pub extensions: Extensions<'a>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group<'a> {
    pub agents: Vec<&'a str>,
    pub rules: Vec<Rule<'a>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rule<'a> {
    pub kind: RuleKind,
    pub pattern: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleKind {
    Allow,
    Disallow,
}

#[cfg(feature = "extensions")]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Extensions<'a> {
    pub sitemaps: Vec<&'a str>,
    pub crawl_delays: Vec<CrawlDelay<'a>>,
    pub hosts: Vec<&'a str>,
    pub clean_params: Vec<CleanParam<'a>>,
    pub other: Vec<Directive<'a>>,
}

#[cfg(feature = "extensions")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrawlDelay<'a> {
    pub agents: Vec<&'a str>,
    pub value: &'a str,
}

#[cfg(feature = "extensions")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CleanParam<'a> {
    pub value: &'a str,
}

#[cfg(feature = "extensions")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Directive<'a> {
    pub key: &'a str,
    pub value: &'a str,
}

impl<'a> RobotsTxt<'a> {
    pub fn parse(input: &'a str) -> Self {
        parse_inner(input, false).robots
    }

    pub fn parse_bytes(input: &'a [u8]) -> Result<Self, ParseError> {
        Self::parse_bytes_with_options(input, ParseOptions::default())
    }

    pub fn parse_bytes_with_options(
        input: &'a [u8],
        options: ParseOptions,
    ) -> Result<Self, ParseError> {
        check_size(input.len(), options)?;
        let input = std::str::from_utf8(input)?;
        Ok(Self::parse(input))
    }

    pub fn parse_with_options(input: &'a str, options: ParseOptions) -> Result<Self, ParseError> {
        check_size(input.len(), options)?;
        Ok(Self::parse(input))
    }

    pub fn parse_with_diagnostics(input: &'a str) -> ParseReport<'a> {
        parse_inner(input, true)
    }

    pub fn parse_with_diagnostics_options(
        input: &'a str,
        options: ParseOptions,
    ) -> Result<ParseReport<'a>, ParseError> {
        check_size(input.len(), options)?;
        Ok(parse_inner(input, true))
    }

    pub fn parse_bytes_with_diagnostics(input: &'a [u8]) -> Result<ParseReport<'a>, ParseError> {
        Self::parse_bytes_with_diagnostics_options(input, ParseOptions::default())
    }

    pub fn parse_bytes_with_diagnostics_options(
        input: &'a [u8],
        options: ParseOptions,
    ) -> Result<ParseReport<'a>, ParseError> {
        check_size(input.len(), options)?;
        let input = std::str::from_utf8(input)?;
        Ok(parse_inner(input, true))
    }

    pub fn is_allowed(&self, user_agent: &str, path: &str) -> bool {
        if path == "/robots.txt" {
            return true;
        }

        let mut exact_match = false;
        let mut best: Option<(usize, RuleKind)> = None;

        for group in &self.groups {
            if group
                .agents
                .iter()
                .any(|agent| *agent != "*" && agent.eq_ignore_ascii_case(user_agent))
            {
                exact_match = true;
                apply_group_rules(group, path, &mut best);
            }
        }

        if !exact_match {
            for group in &self.groups {
                if group.agents.contains(&"*") {
                    apply_group_rules(group, path, &mut best);
                }
            }
        }

        match best {
            Some((_, RuleKind::Allow)) | None => true,
            Some((_, RuleKind::Disallow)) => false,
        }
    }
}

fn check_size(len: usize, options: ParseOptions) -> Result<(), ParseError> {
    if let Some(max) = options.max_bytes {
        if len > max {
            return Err(ParseError::TooLarge { len, max });
        }
    }

    Ok(())
}

fn parse_inner<'a>(input: &'a str, diagnostics: bool) -> ParseReport<'a> {
    let mut groups = vec![];
    let mut current: Option<Group<'a>> = None;
    let mut current_has_rules = false;
    let mut warnings = vec![];

    #[cfg(feature = "extensions")]
    let mut extensions = Extensions::default();

    for (line_number, line) in Lines::new(input) {
        let line = trim_ascii(strip_comment(line));
        if line.is_empty() {
            continue;
        }

        let Some((key, value)) = split_directive(line) else {
            if diagnostics {
                warnings.push(ParseWarning {
                    line: line_number,
                    kind: ParseWarningKind::MissingSeparator { line },
                });
            }
            continue;
        };

        let key = trim_ascii(key);
        let value = trim_ascii(value);
        if key.is_empty() {
            if diagnostics {
                warnings.push(ParseWarning {
                    line: line_number,
                    kind: ParseWarningKind::EmptyDirectiveKey,
                });
            }
            continue;
        }

        if key.eq_ignore_ascii_case("user-agent") {
            if value.is_empty() {
                if diagnostics {
                    warnings.push(ParseWarning {
                        line: line_number,
                        kind: ParseWarningKind::EmptyUserAgent,
                    });
                }
                continue;
            };

            match current.as_mut() {
                Some(group) if !current_has_rules => group.agents.push(value),
                Some(_) => {
                    groups.push(current.take().expect("current group exists"));
                    current = Some(Group {
                        agents: vec![value],
                        rules: vec![],
                    });
                    current_has_rules = false;
                }
                None => {
                    current = Some(Group {
                        agents: vec![value],
                        rules: vec![],
                    });
                }
            }
        } else if key.eq_ignore_ascii_case("allow") || key.eq_ignore_ascii_case("disallow") {
            let Some(group) = current.as_mut() else {
                if diagnostics {
                    warnings.push(ParseWarning {
                        line: line_number,
                        kind: ParseWarningKind::RuleBeforeUserAgent { key },
                    });
                }
                continue;
            };
            let kind = if key.eq_ignore_ascii_case("allow") {
                RuleKind::Allow
            } else {
                RuleKind::Disallow
            };
            group.rules.push(Rule {
                kind,
                pattern: value,
            });
            current_has_rules = true;
        } else {
            #[cfg(feature = "extensions")]
            collect_extension(&mut extensions, current.as_ref(), key, value);
        }
    }

    if let Some(group) = current {
        groups.push(group);
    }

    ParseReport {
        robots: RobotsTxt {
            groups,
            #[cfg(feature = "extensions")]
            extensions,
        },
        warnings,
    }
}

fn apply_group_rules(group: &Group<'_>, path: &str, best: &mut Option<(usize, RuleKind)>) {
    for rule in &group.rules {
        if rule.pattern.is_empty() || !pattern_matches(rule.pattern, path) {
            continue;
        }

        let specificity = pattern_specificity(rule.pattern);
        match *best {
            Some((best_specificity, best_kind))
                if specificity < best_specificity
                    || (specificity == best_specificity
                        && !(rule.kind == RuleKind::Allow && best_kind == RuleKind::Disallow)) =>
            {
                continue;
            }
            _ => *best = Some((specificity, rule.kind)),
        }
    }
}

fn pattern_matches(pattern: &str, path: &str) -> bool {
    let (pattern, anchored) = match pattern.strip_suffix('$') {
        Some(pattern) => (pattern, true),
        None => (pattern, false),
    };

    if !pattern.as_bytes().contains(&b'*') {
        return if anchored {
            path == pattern
        } else {
            path.starts_with(pattern)
        };
    }

    glob_matches(pattern.as_bytes(), path.as_bytes(), anchored)
}

fn glob_matches(pattern: &[u8], path: &[u8], anchored: bool) -> bool {
    let mut parts = pattern.split(|byte| *byte == b'*');
    let Some(first) = parts.next() else {
        return true;
    };

    if !path.starts_with(first) {
        return false;
    }

    let mut offset = first.len();
    let mut ends_with_star = pattern.last() == Some(&b'*');

    for part in parts {
        if part.is_empty() {
            ends_with_star = true;
            continue;
        }

        ends_with_star = false;
        let Some(found) = memmem::find(&path[offset..], part) else {
            return false;
        };
        offset += found + part.len();
    }

    !anchored || ends_with_star || offset == path.len()
}

fn pattern_specificity(pattern: &str) -> usize {
    pattern.strip_suffix('$').unwrap_or(pattern).len()
}

#[cfg(feature = "extensions")]
fn collect_extension<'a>(
    extensions: &mut Extensions<'a>,
    current: Option<&Group<'a>>,
    key: &'a str,
    value: &'a str,
) {
    if key.eq_ignore_ascii_case("sitemap") {
        if !value.is_empty() {
            extensions.sitemaps.push(value);
        }
    } else if key.eq_ignore_ascii_case("crawl-delay") {
        extensions.crawl_delays.push(CrawlDelay {
            agents: current
                .map(|group| group.agents.clone())
                .unwrap_or_default(),
            value,
        });
    } else if key.eq_ignore_ascii_case("host") {
        if !value.is_empty() {
            extensions.hosts.push(value);
        }
    } else if key.eq_ignore_ascii_case("clean-param") {
        if !value.is_empty() {
            extensions.clean_params.push(CleanParam { value });
        }
    } else {
        extensions.other.push(Directive { key, value });
    }
}

fn strip_comment(line: &str) -> &str {
    match memchr(b'#', line.as_bytes()) {
        Some(index) => &line[..index],
        None => line,
    }
}

fn split_directive(line: &str) -> Option<(&str, &str)> {
    let index = memchr(b':', line.as_bytes())?;
    Some((&line[..index], &line[index + 1..]))
}

fn trim_ascii(value: &str) -> &str {
    let bytes = value.as_bytes();
    let mut start = 0;
    let mut end = bytes.len();

    while start < end && matches!(bytes[start], b' ' | b'\t') {
        start += 1;
    }
    while end > start && matches!(bytes[end - 1], b' ' | b'\t') {
        end -= 1;
    }

    &value[start..end]
}

struct Lines<'a> {
    input: &'a str,
    offset: usize,
    line: usize,
}

impl<'a> Lines<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input,
            offset: 0,
            line: 1,
        }
    }
}

impl<'a> Iterator for Lines<'a> {
    type Item = (usize, &'a str);

    fn next(&mut self) -> Option<Self::Item> {
        if self.offset > self.input.len() {
            return None;
        }

        let remaining = &self.input[self.offset..];
        if remaining.is_empty() {
            self.offset += 1;
            return None;
        }

        let line_end = memchr(b'\n', remaining.as_bytes()).unwrap_or(remaining.len());
        let mut line = &remaining[..line_end];
        if let Some(stripped) = line.strip_suffix('\r') {
            line = stripped;
        }

        let line_number = self.line;
        self.line += 1;
        self.offset += line_end + 1;
        Some((line_number, line))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_groups_comments_and_crlf() {
        let robots = RobotsTxt::parse(
            "# ignored\r\nUser-agent: FooBot\r\nUser-agent: BarBot # same group\r\nDisallow: /private\r\nAllow: /private/public\r\n",
        );

        assert_eq!(robots.groups.len(), 1);
        assert_eq!(robots.groups[0].agents, vec!["FooBot", "BarBot"]);
        assert_eq!(robots.groups[0].rules.len(), 2);
        assert!(!robots.is_allowed("FooBot", "/private/file"));
        assert!(robots.is_allowed("FooBot", "/private/public/file"));
    }

    #[test]
    fn ignores_rules_before_first_user_agent() {
        let robots = RobotsTxt::parse("Disallow: /\nUser-agent: *\nAllow: /\n");

        assert!(robots.is_allowed("AnyBot", "/anything"));
    }

    #[test]
    fn starts_new_group_after_rules() {
        let robots = RobotsTxt::parse(
            "User-agent: FooBot\nDisallow: /foo\nUser-agent: BarBot\nDisallow: /bar\n",
        );

        assert_eq!(robots.groups.len(), 2);
        assert!(!robots.is_allowed("FooBot", "/foo"));
        assert!(robots.is_allowed("FooBot", "/bar"));
        assert!(!robots.is_allowed("BarBot", "/bar"));
    }

    #[test]
    fn merges_multiple_exact_matching_groups() {
        let robots = RobotsTxt::parse(
            "User-agent: FooBot\nDisallow: /foo\n\nUser-agent: FooBot\nDisallow: /bar\n",
        );

        assert!(!robots.is_allowed("FooBot", "/foo"));
        assert!(!robots.is_allowed("FooBot", "/bar"));
    }

    #[test]
    fn falls_back_to_star_group() {
        let robots =
            RobotsTxt::parse("User-agent: *\nDisallow: /all\nUser-agent: FooBot\nAllow: /\n");

        assert!(!robots.is_allowed("OtherBot", "/all"));
        assert!(robots.is_allowed("FooBot", "/all"));
    }

    #[test]
    fn longest_match_wins_and_allow_wins_ties() {
        let robots = RobotsTxt::parse(
            "User-agent: *\nDisallow: /example/\nAllow: /example/public\nDisallow: /tie\nAllow: /tie\n",
        );

        assert!(!robots.is_allowed("AnyBot", "/example/private"));
        assert!(robots.is_allowed("AnyBot", "/example/public/page"));
        assert!(robots.is_allowed("AnyBot", "/tie"));
    }

    #[test]
    fn supports_wildcard_and_end_anchor() {
        let robots = RobotsTxt::parse("User-agent: *\nDisallow: /*.gif$\nAllow: /public/*.gif$\n");

        assert!(!robots.is_allowed("AnyBot", "/images/a.gif"));
        assert!(robots.is_allowed("AnyBot", "/images/a.gif?size=large"));
        assert!(robots.is_allowed("AnyBot", "/public/a.gif"));
    }

    #[test]
    fn empty_disallow_does_not_block() {
        let robots = RobotsTxt::parse("User-agent: *\nDisallow:\n");

        assert!(robots.is_allowed("AnyBot", "/anything"));
    }

    #[test]
    fn robots_txt_is_implicitly_allowed() {
        let robots = RobotsTxt::parse("User-agent: *\nDisallow: /\n");

        assert!(robots.is_allowed("AnyBot", "/robots.txt"));
    }

    #[test]
    fn parse_bytes_rejects_invalid_utf8() {
        let error = RobotsTxt::parse_bytes(&[0xff]).expect_err("invalid UTF-8 should fail");

        assert!(matches!(error, ParseError::Utf8(_)));
    }

    #[test]
    fn parse_with_options_rejects_oversized_input() {
        let error =
            RobotsTxt::parse_with_options("User-agent: *\n", ParseOptions { max_bytes: Some(4) })
                .expect_err("oversized input should fail");

        assert!(matches!(error, ParseError::TooLarge { len: 14, max: 4 }));
    }

    #[test]
    fn parse_with_options_allows_disabled_limit() {
        let robots = RobotsTxt::parse_with_options(
            "User-agent: *\nDisallow: /private\n",
            ParseOptions { max_bytes: None },
        )
        .expect("disabled size limit should parse");

        assert!(!robots.is_allowed("AnyBot", "/private"));
    }

    #[test]
    fn diagnostics_report_soft_parse_issues() {
        let report = RobotsTxt::parse_with_diagnostics(
            "Disallow: /\nMissing separator\n: value\nUser-agent:\nUser-agent: *\nDisallow: /private\n",
        );

        assert_eq!(report.warnings.len(), 4);
        assert_eq!(
            report.warnings,
            vec![
                ParseWarning {
                    line: 1,
                    kind: ParseWarningKind::RuleBeforeUserAgent { key: "Disallow" },
                },
                ParseWarning {
                    line: 2,
                    kind: ParseWarningKind::MissingSeparator {
                        line: "Missing separator",
                    },
                },
                ParseWarning {
                    line: 3,
                    kind: ParseWarningKind::EmptyDirectiveKey,
                },
                ParseWarning {
                    line: 4,
                    kind: ParseWarningKind::EmptyUserAgent,
                },
            ]
        );
        assert!(!report.robots.is_allowed("AnyBot", "/private"));
    }

    #[cfg(feature = "extensions")]
    #[test]
    fn collects_extensions_without_changing_groups() {
        let robots = RobotsTxt::parse(
            "Sitemap: https://example.com/sitemap.xml\nUser-agent: Bingbot\nCrawl-delay: 5\nDisallow: /slow\nHost: example.com\nClean-param: ref /shop\nX-Test: value\n",
        );

        assert_eq!(
            robots.extensions.sitemaps,
            vec!["https://example.com/sitemap.xml"]
        );
        assert_eq!(robots.extensions.crawl_delays.len(), 1);
        assert_eq!(robots.extensions.crawl_delays[0].agents, vec!["Bingbot"]);
        assert_eq!(robots.extensions.crawl_delays[0].value, "5");
        assert_eq!(robots.extensions.hosts, vec!["example.com"]);
        assert_eq!(robots.extensions.clean_params[0].value, "ref /shop");
        assert_eq!(robots.extensions.other[0].key, "X-Test");
        assert!(!robots.is_allowed("Bingbot", "/slow"));
    }
}
