#![cfg_attr(docsrs, feature(doc_cfg))]
//! Fast, zero-copy parsing and matching for `robots.txt` files.
//!
//! `fast-robots` parses the standardized `User-agent`, `Allow`, and
//! `Disallow` records used by crawlers, then evaluates paths using the RFC 9309
//! matching rules: exact user-agent groups are preferred over `*`, the longest
//! matching rule wins, and `Allow` wins ties.
//!
//! Parsed values borrow from the original input, so parsing avoids copying rule
//! strings, user agents, and extension metadata. Keep the input string or byte
//! buffer alive for as long as the returned [`RobotsTxt`] is used.
//!
//! # Quick Start
//!
//! ```
//! use fast_robots::RobotsTxt;
//!
//! let robots = RobotsTxt::parse(
//!     "User-agent: *\n\
//!      Disallow: /private/\n\
//!      Allow: /private/public/\n",
//! );
//!
//! assert!(!robots.is_allowed("ExampleBot", "/private/file.html"));
//! assert!(robots.is_allowed("ExampleBot", "/private/public/file.html"));
//! ```
//!
//! # Fallible Byte Parsing
//!
//! Use the byte APIs when reading directly from files or HTTP responses. They
//! reject invalid UTF-8 and inputs larger than [`DEFAULT_MAX_BYTES`] by default.
//!
//! ```
//! # fn main() -> Result<(), fast_robots::ParseError> {
//! use fast_robots::RobotsTxt;
//!
//! let robots = RobotsTxt::parse_bytes(b"User-agent: *\nDisallow: /tmp\n")?;
//! assert!(!robots.is_allowed("ExampleBot", "/tmp/cache"));
//! # Ok(())
//! # }
//! ```
//!
//! # Diagnostics
//!
//! The parser is tolerant by default and ignores malformed lines it can recover
//! from. Use diagnostics when you want validator-style warnings alongside the
//! parsed rules.
//!
//! ```rust
//! use fast_robots::{ParseWarningKind, RobotsTxt};
//!
//! let report = RobotsTxt::parse_with_diagnostics(
//!     "Disallow: /\nMissing separator\nUser-agent: *\nDisallow: /private\n",
//! );
//!
//! assert!(matches!(
//!     report.warnings[0].kind,
//!     ParseWarningKind::RuleBeforeUserAgent { .. }
//! ));
//! assert!(matches!(
//!     report.warnings[1].kind,
//!     ParseWarningKind::MissingSeparator { .. }
//! ));
//! assert!(!report.robots.is_allowed("ExampleBot", "/private"));
//! ```
//!
//! # Extension Metadata
//!
//! With the default `extensions` feature, non-core directives such as `Sitemap`
//! and `Crawl-delay` are preserved as metadata. Extension metadata never changes
//! [`RobotsTxt::is_allowed`] decisions.
//!
//! ```rust
//! # #[cfg(feature = "extensions")]
//! # {
//! use fast_robots::RobotsTxt;
//!
//! let robots = RobotsTxt::parse(
//!     "Sitemap: https://example.com/sitemap.xml\n\
//!      User-agent: SlowBot\n\
//!      Crawl-delay: 5\n\
//!      Disallow: /slow/\n",
//! );
//!
//! assert_eq!(robots.extensions.sitemaps, ["https://example.com/sitemap.xml"]);
//! assert_eq!(robots.extensions.crawl_delays[0].agents, ["SlowBot"]);
//! assert!(!robots.is_allowed("SlowBot", "/slow/page.html"));
//! # }
//! ```

use std::collections::HashMap;

use memchr::{memchr, memmem};
use thiserror::Error;

/// Default maximum accepted input size for fallible parsing APIs.
///
/// This matches the 500 KiB minimum fetch limit specified by RFC 9309 and is
/// used by [`ParseOptions::default`]. Set [`ParseOptions::max_bytes`] to `None`
/// to disable the limit.
pub const DEFAULT_MAX_BYTES: usize = 512 * 1024;

/// Errors returned by fallible parsing APIs.
///
/// Soft syntax issues, such as missing separators, are not hard errors because
/// crawlers are expected to recover from malformed `robots.txt` files where
/// possible. Use [`RobotsTxt::parse_with_diagnostics`] or
/// [`RobotsTxt::parse_bytes_with_diagnostics`] to collect those warnings.
#[derive(Debug, Error)]
pub enum ParseError {
    /// The input bytes were not valid UTF-8.
    #[error("robots.txt is not valid UTF-8")]
    Utf8(#[from] std::str::Utf8Error),

    /// The input length exceeded [`ParseOptions::max_bytes`].
    #[error("robots.txt is too large: {len} bytes exceeds limit of {max} bytes")]
    TooLarge {
        /// Actual input length in bytes.
        len: usize,
        /// Configured maximum input length in bytes.
        max: usize,
    },
}

/// Options shared by fallible parsing APIs.
///
/// # Examples
///
/// ```
/// # fn main() -> Result<(), fast_robots::ParseError> {
/// use fast_robots::{ParseOptions, RobotsTxt};
///
/// let robots = RobotsTxt::parse_with_options(
///     "User-agent: *\nDisallow: /private\n",
///     ParseOptions { max_bytes: Some(1024) },
/// )?;
///
/// assert!(!robots.is_allowed("ExampleBot", "/private"));
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseOptions {
    /// Maximum accepted input size in bytes.
    ///
    /// `Some(DEFAULT_MAX_BYTES)` is used by default. Set to `None` to disable
    /// size checks for trusted inputs.
    pub max_bytes: Option<usize>,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            max_bytes: Some(DEFAULT_MAX_BYTES),
        }
    }
}

/// Parsed rules plus any diagnostics collected during parsing.
///
/// Returned by diagnostics APIs. The parser output remains available even when
/// warnings were emitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseReport<'a> {
    /// Parsed `robots.txt` rules and extension metadata.
    pub robots: RobotsTxt<'a>,
    /// Recoverable parse warnings in source order.
    pub warnings: Vec<ParseWarning<'a>>,
}

/// A recoverable parse issue with its one-based line number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseWarning<'a> {
    /// One-based line number where the warning was found.
    pub line: usize,
    /// Warning category and borrowed source data, when relevant.
    pub kind: ParseWarningKind<'a>,
}

/// Recoverable parse warning categories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseWarningKind<'a> {
    /// A non-empty, non-comment line did not contain a `:` separator.
    MissingSeparator {
        /// Trimmed line contents.
        line: &'a str,
    },
    /// A directive had a `:` separator but no key before it.
    EmptyDirectiveKey,
    /// A `User-agent` directive had an empty value.
    EmptyUserAgent,
    /// An `Allow` or `Disallow` directive appeared before any `User-agent`.
    RuleBeforeUserAgent {
        /// Directive key that appeared before a group was started.
        key: &'a str,
    },
}

/// Parsed `robots.txt` data.
///
/// Values inside this type borrow from the original input. Use
/// [`RobotsTxt::is_allowed`] for access checks and inspect [`RobotsTxt::groups`]
/// when you need the parsed rule structure.
///
/// # Examples
///
/// ```
/// use fast_robots::{RobotsTxt, RuleKind};
///
/// let robots = RobotsTxt::parse("User-agent: *\nDisallow: /admin\n");
///
/// assert_eq!(robots.groups[0].agents, ["*"]);
/// assert_eq!(robots.groups[0].rules[0].kind, RuleKind::Disallow);
/// assert_eq!(robots.groups[0].rules[0].pattern, "/admin");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RobotsTxt<'a> {
    /// Standard access-control groups in source order.
    pub groups: Vec<Group<'a>>,
    /// Non-core metadata collected when the `extensions` feature is enabled.
    #[cfg(feature = "extensions")]
    #[cfg_attr(docsrs, doc(cfg(feature = "extensions")))]
    pub extensions: Extensions<'a>,
}

/// A `robots.txt` group containing one or more user agents and their rules.
///
/// Consecutive `User-agent` records before the first rule belong to the same
/// group. A later `User-agent` starts a new group after any `Allow` or
/// `Disallow` record has been seen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group<'a> {
    /// User-agent product tokens covered by this group.
    pub agents: Vec<&'a str>,
    /// Access-control rules associated with [`Group::agents`].
    pub rules: Vec<Rule<'a>>,
}

/// A single `Allow` or `Disallow` rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rule<'a> {
    /// Whether this rule allows or disallows matching paths.
    pub kind: RuleKind,
    /// Path pattern borrowed from the directive value.
    ///
    /// Patterns may contain `*` wildcards and a trailing `$` end anchor.
    pub pattern: &'a str,
}

/// Access-control directive kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleKind {
    /// An `Allow` directive.
    Allow,
    /// A `Disallow` directive.
    Disallow,
}

/// Precompiled matcher for repeated access checks against one [`RobotsTxt`].
///
/// Build this with [`RobotsTxt::matcher`] when checking many paths against the
/// same parsed file. Construction allocates an index and precomputes rule
/// metadata, so [`RobotsTxt::is_allowed`] remains the lower-overhead option for
/// one-off checks.
#[derive(Debug, Clone)]
pub struct RobotsMatcher<'a> {
    agent_groups: HashMap<String, Vec<usize>>,
    fallback_groups: Vec<usize>,
    compiled_rules: Vec<Vec<CompiledRule<'a>>>,
}

#[derive(Debug, Clone, Copy)]
struct CompiledRule<'a> {
    kind: RuleKind,
    pattern: &'a str,
    anchored: bool,
    has_wildcard: bool,
    specificity: usize,
}

/// Feature-gated metadata for common non-standard directives.
///
/// These values are collected for callers that need them, but they do not affect
/// access decisions returned by [`RobotsTxt::is_allowed`].
///
/// # Examples
///
/// ```
/// use fast_robots::RobotsTxt;
///
/// let robots = RobotsTxt::parse(
///     "Sitemap: https://example.com/sitemap.xml\n\
///      Host: example.com\n\
///      User-agent: *\n\
///      Crawl-delay: 10\n",
/// );
///
/// assert_eq!(robots.extensions.sitemaps, ["https://example.com/sitemap.xml"]);
/// assert_eq!(robots.extensions.hosts, ["example.com"]);
/// assert_eq!(robots.extensions.crawl_delays[0].value, "10");
/// ```
#[cfg(feature = "extensions")]
#[cfg_attr(docsrs, doc(cfg(feature = "extensions")))]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Extensions<'a> {
    /// `Sitemap` directive values.
    pub sitemaps: Vec<&'a str>,
    /// `Crawl-delay` directive values, including the current group agents.
    pub crawl_delays: Vec<CrawlDelay<'a>>,
    /// `Host` directive values.
    pub hosts: Vec<&'a str>,
    /// `Clean-param` directive values.
    pub clean_params: Vec<CleanParam<'a>>,
    /// Unknown non-core directives preserved as key/value pairs.
    pub other: Vec<Directive<'a>>,
}

/// A `Crawl-delay` directive and the group agents active when it appeared.
#[cfg(feature = "extensions")]
#[cfg_attr(docsrs, doc(cfg(feature = "extensions")))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrawlDelay<'a> {
    /// Current group agents at the point where the directive appeared.
    ///
    /// This is empty when `Crawl-delay` appears before any `User-agent`.
    pub agents: Vec<&'a str>,
    /// Raw `Crawl-delay` value.
    pub value: &'a str,
}

/// A `Clean-param` directive value.
#[cfg(feature = "extensions")]
#[cfg_attr(docsrs, doc(cfg(feature = "extensions")))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CleanParam<'a> {
    /// Raw `Clean-param` value.
    pub value: &'a str,
}

/// A non-core directive preserved as a raw key/value pair.
#[cfg(feature = "extensions")]
#[cfg_attr(docsrs, doc(cfg(feature = "extensions")))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Directive<'a> {
    /// Directive key as written before the `:` separator, after ASCII trim.
    pub key: &'a str,
    /// Directive value as written after the `:` separator, after ASCII trim.
    pub value: &'a str,
}

impl<'a> RobotsTxt<'a> {
    /// Parses a UTF-8 `robots.txt` string into access rules.
    ///
    /// This is tolerant and infallible: malformed lines are ignored where the
    /// parser can recover. Use [`RobotsTxt::parse_with_diagnostics`] to collect
    /// warnings, or [`RobotsTxt::parse_with_options`] to enforce a size limit.
    ///
    /// # Examples
    ///
    /// ```
    /// use fast_robots::RobotsTxt;
    ///
    /// let robots = RobotsTxt::parse("User-agent: *\nDisallow: /private\n");
    ///
    /// assert!(!robots.is_allowed("ExampleBot", "/private/file.html"));
    /// assert!(robots.is_allowed("ExampleBot", "/public/file.html"));
    /// ```
    pub fn parse(input: &'a str) -> Self {
        parse_inner(input, false).robots
    }

    /// Parses UTF-8 bytes into access rules using [`ParseOptions::default`].
    ///
    /// Returns [`ParseError::Utf8`] for invalid UTF-8 and
    /// [`ParseError::TooLarge`] when the input is larger than
    /// [`DEFAULT_MAX_BYTES`].
    ///
    /// # Examples
    ///
    /// ```
    /// # fn main() -> Result<(), fast_robots::ParseError> {
    /// use fast_robots::RobotsTxt;
    ///
    /// let robots = RobotsTxt::parse_bytes(b"User-agent: *\nDisallow: /tmp\n")?;
    /// assert!(!robots.is_allowed("ExampleBot", "/tmp/cache"));
    /// # Ok(())
    /// # }
    /// ```
    pub fn parse_bytes(input: &'a [u8]) -> Result<Self, ParseError> {
        Self::parse_bytes_with_options(input, ParseOptions::default())
    }

    /// Parses UTF-8 bytes into access rules with explicit options.
    ///
    /// Use this when reading raw bytes and you need a custom size limit.
    ///
    /// # Examples
    ///
    /// ```
    /// # fn main() -> Result<(), fast_robots::ParseError> {
    /// use fast_robots::{ParseOptions, RobotsTxt};
    ///
    /// let robots = RobotsTxt::parse_bytes_with_options(
    ///     b"User-agent: *\nDisallow: /cache\n",
    ///     ParseOptions { max_bytes: Some(1024) },
    /// )?;
    ///
    /// assert!(!robots.is_allowed("ExampleBot", "/cache/file"));
    /// # Ok(())
    /// # }
    /// ```
    pub fn parse_bytes_with_options(
        input: &'a [u8],
        options: ParseOptions,
    ) -> Result<Self, ParseError> {
        check_size(input.len(), options)?;
        let input = std::str::from_utf8(input)?;
        Ok(Self::parse(input))
    }

    /// Parses a UTF-8 string into access rules with explicit options.
    ///
    /// This is useful when the input is already a `str` but should still be
    /// checked against a maximum size.
    ///
    /// # Examples
    ///
    /// ```
    /// # fn main() -> Result<(), fast_robots::ParseError> {
    /// use fast_robots::{ParseOptions, RobotsTxt};
    ///
    /// let robots = RobotsTxt::parse_with_options(
    ///     "User-agent: *\nDisallow: /private\n",
    ///     ParseOptions { max_bytes: Some(1024) },
    /// )?;
    ///
    /// assert!(!robots.is_allowed("ExampleBot", "/private"));
    /// # Ok(())
    /// # }
    /// ```
    pub fn parse_with_options(input: &'a str, options: ParseOptions) -> Result<Self, ParseError> {
        check_size(input.len(), options)?;
        Ok(Self::parse(input))
    }

    /// Parses a UTF-8 string and records recoverable syntax warnings.
    ///
    /// Diagnostics do not change parser recovery behavior; they only expose the
    /// issues that tolerant parsing skipped.
    ///
    /// # Examples
    ///
    /// ```
    /// use fast_robots::{ParseWarningKind, RobotsTxt};
    ///
    /// let report = RobotsTxt::parse_with_diagnostics(
    ///     "Disallow: /\nMissing separator\nUser-agent: *\nDisallow: /private\n",
    /// );
    ///
    /// assert_eq!(report.warnings.len(), 2);
    /// assert!(matches!(
    ///     report.warnings[0].kind,
    ///     ParseWarningKind::RuleBeforeUserAgent { .. }
    /// ));
    /// assert!(!report.robots.is_allowed("ExampleBot", "/private"));
    /// ```
    pub fn parse_with_diagnostics(input: &'a str) -> ParseReport<'a> {
        parse_inner(input, true)
    }

    /// Parses a UTF-8 string with diagnostics and explicit options.
    ///
    /// # Examples
    ///
    /// ```
    /// # fn main() -> Result<(), fast_robots::ParseError> {
    /// use fast_robots::{ParseOptions, RobotsTxt};
    ///
    /// let report = RobotsTxt::parse_with_diagnostics_options(
    ///     "User-agent: *\nDisallow: /private\n",
    ///     ParseOptions { max_bytes: Some(1024) },
    /// )?;
    ///
    /// assert!(report.warnings.is_empty());
    /// assert!(!report.robots.is_allowed("ExampleBot", "/private"));
    /// # Ok(())
    /// # }
    /// ```
    pub fn parse_with_diagnostics_options(
        input: &'a str,
        options: ParseOptions,
    ) -> Result<ParseReport<'a>, ParseError> {
        check_size(input.len(), options)?;
        Ok(parse_inner(input, true))
    }

    /// Parses UTF-8 bytes and records recoverable syntax warnings.
    ///
    /// Uses [`ParseOptions::default`] for size checking.
    ///
    /// # Examples
    ///
    /// ```
    /// # fn main() -> Result<(), fast_robots::ParseError> {
    /// use fast_robots::RobotsTxt;
    ///
    /// let report = RobotsTxt::parse_bytes_with_diagnostics(
    ///     b"User-agent: *\nDisallow: /private\n",
    /// )?;
    ///
    /// assert!(report.warnings.is_empty());
    /// assert!(!report.robots.is_allowed("ExampleBot", "/private"));
    /// # Ok(())
    /// # }
    /// ```
    pub fn parse_bytes_with_diagnostics(input: &'a [u8]) -> Result<ParseReport<'a>, ParseError> {
        Self::parse_bytes_with_diagnostics_options(input, ParseOptions::default())
    }

    /// Parses UTF-8 bytes with diagnostics and explicit options.
    ///
    /// # Examples
    ///
    /// ```
    /// # fn main() -> Result<(), fast_robots::ParseError> {
    /// use fast_robots::{ParseOptions, RobotsTxt};
    ///
    /// let report = RobotsTxt::parse_bytes_with_diagnostics_options(
    ///     b"User-agent: *\nDisallow: /private\n",
    ///     ParseOptions { max_bytes: Some(1024) },
    /// )?;
    ///
    /// assert!(report.warnings.is_empty());
    /// # Ok(())
    /// # }
    /// ```
    pub fn parse_bytes_with_diagnostics_options(
        input: &'a [u8],
        options: ParseOptions,
    ) -> Result<ParseReport<'a>, ParseError> {
        check_size(input.len(), options)?;
        let input = std::str::from_utf8(input)?;
        Ok(parse_inner(input, true))
    }

    /// Builds an indexed matcher for repeated access checks.
    ///
    /// The returned matcher borrows this parsed file, indexes user-agent groups,
    /// and precomputes rule metadata. Use it when checking many URLs against the
    /// same `robots.txt`; for one-off checks, [`RobotsTxt::is_allowed`] avoids
    /// the upfront allocation cost.
    ///
    /// # Examples
    ///
    /// ```
    /// use fast_robots::RobotsTxt;
    ///
    /// let robots = RobotsTxt::parse("User-agent: *\nDisallow: /private\n");
    /// let matcher = robots.matcher();
    ///
    /// assert!(!matcher.is_allowed("ExampleBot", "/private/file"));
    /// assert!(matcher.is_allowed("ExampleBot", "/public/file"));
    /// ```
    pub fn matcher(&'a self) -> RobotsMatcher<'a> {
        RobotsMatcher::new(self)
    }

    /// Returns whether `user_agent` may crawl `path`.
    ///
    /// The matcher implements the core RFC 9309 access semantics used by this
    /// crate: exact user-agent groups are considered before the `*` fallback,
    /// matching exact groups are merged, the longest matching pattern wins, and
    /// `Allow` wins ties. `/robots.txt` is always allowed.
    ///
    /// `path` should be the URL path and optional query string, not a full URL.
    ///
    /// # Examples
    ///
    /// ```
    /// use fast_robots::RobotsTxt;
    ///
    /// let robots = RobotsTxt::parse(
    ///     "User-agent: *\n\
    ///      Disallow: /private\n\
    ///      Allow: /private/public\n",
    /// );
    ///
    /// assert!(!robots.is_allowed("ExampleBot", "/private/file"));
    /// assert!(robots.is_allowed("ExampleBot", "/private/public/file"));
    /// assert!(robots.is_allowed("ExampleBot", "/robots.txt"));
    /// ```
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

        rule_decision(best)
    }
}

impl<'a> RobotsMatcher<'a> {
    fn new(robots: &'a RobotsTxt<'a>) -> Self {
        let groups = robots.groups.as_slice();
        let mut agent_groups: HashMap<String, Vec<usize>> = HashMap::new();
        let mut fallback_groups = Vec::new();
        let mut compiled_rules = Vec::with_capacity(groups.len());

        for (group_index, group) in groups.iter().enumerate() {
            for agent in &group.agents {
                if *agent == "*" {
                    fallback_groups.push(group_index);
                } else {
                    let indexes = agent_groups.entry(agent.to_ascii_lowercase()).or_default();
                    if !indexes.contains(&group_index) {
                        indexes.push(group_index);
                    }
                }
            }

            compiled_rules.push(group.rules.iter().filter_map(CompiledRule::new).collect());
        }

        Self {
            agent_groups,
            fallback_groups,
            compiled_rules,
        }
    }

    /// Returns whether `user_agent` may crawl `path` using the prebuilt index.
    ///
    /// This has the same access semantics as [`RobotsTxt::is_allowed`], including
    /// exact user-agent precedence over `*`, merged exact groups, longest-match
    /// rule selection, `Allow` tie wins, and implicit allowance for `/robots.txt`.
    pub fn is_allowed(&self, user_agent: &str, path: &str) -> bool {
        if path == "/robots.txt" {
            return true;
        }

        let mut best: Option<(usize, RuleKind)> = None;
        let agent = user_agent.to_ascii_lowercase();

        if let Some(group_indexes) = self.agent_groups.get(&agent) {
            self.apply_group_indexes(group_indexes, path, &mut best);
        } else {
            self.apply_group_indexes(&self.fallback_groups, path, &mut best);
        }

        rule_decision(best)
    }

    fn apply_group_indexes(
        &self,
        group_indexes: &[usize],
        path: &str,
        best: &mut Option<(usize, RuleKind)>,
    ) {
        for &group_index in group_indexes {
            apply_compiled_rules(&self.compiled_rules[group_index], path, best);
        }
    }
}

impl<'a> CompiledRule<'a> {
    fn new(rule: &Rule<'a>) -> Option<Self> {
        if rule.pattern.is_empty() {
            return None;
        }

        let (pattern, anchored) = strip_end_anchor(rule.pattern);

        Some(Self {
            kind: rule.kind,
            pattern,
            anchored,
            has_wildcard: pattern.as_bytes().contains(&b'*'),
            specificity: pattern.len(),
        })
    }

    fn matching_specificity(self, path: &str) -> Option<usize> {
        let matched = if self.has_wildcard {
            glob_matches(self.pattern.as_bytes(), path.as_bytes(), self.anchored)
        } else if self.anchored {
            path == self.pattern
        } else {
            path.starts_with(self.pattern)
        };

        matched.then_some(self.specificity)
    }
}

/// Checks an input length against the configured parser size limit.
fn check_size(len: usize, options: ParseOptions) -> Result<(), ParseError> {
    if let Some(max) = options.max_bytes {
        if len > max {
            return Err(ParseError::TooLarge { len, max });
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DirectiveKind {
    UserAgent,
    Allow,
    Disallow,
    #[cfg(feature = "extensions")]
    Sitemap,
    #[cfg(feature = "extensions")]
    CrawlDelay,
    #[cfg(feature = "extensions")]
    Host,
    #[cfg(feature = "extensions")]
    CleanParam,
    Other,
}

fn classify_directive_key(key: &str) -> DirectiveKind {
    match key.as_bytes() {
        b"Allow" | b"allow" => return DirectiveKind::Allow,
        b"Disallow" | b"disallow" => return DirectiveKind::Disallow,
        b"User-agent" | b"user-agent" => return DirectiveKind::UserAgent,
        #[cfg(feature = "extensions")]
        b"Host" | b"host" => return DirectiveKind::Host,
        #[cfg(feature = "extensions")]
        b"Sitemap" | b"sitemap" => return DirectiveKind::Sitemap,
        #[cfg(feature = "extensions")]
        b"Crawl-delay" | b"crawl-delay" => return DirectiveKind::CrawlDelay,
        #[cfg(feature = "extensions")]
        b"Clean-param" | b"clean-param" => return DirectiveKind::CleanParam,
        _ => {}
    }

    classify_directive_key_ignore_case(key)
}

#[cold]
#[inline(never)]
fn classify_directive_key_ignore_case(key: &str) -> DirectiveKind {
    match key.len() {
        5 if key.eq_ignore_ascii_case("allow") => DirectiveKind::Allow,
        8 if key.eq_ignore_ascii_case("disallow") => DirectiveKind::Disallow,
        10 if key.eq_ignore_ascii_case("user-agent") => DirectiveKind::UserAgent,
        #[cfg(feature = "extensions")]
        4 if key.eq_ignore_ascii_case("host") => DirectiveKind::Host,
        #[cfg(feature = "extensions")]
        7 if key.eq_ignore_ascii_case("sitemap") => DirectiveKind::Sitemap,
        #[cfg(feature = "extensions")]
        11 if key.eq_ignore_ascii_case("crawl-delay") => DirectiveKind::CrawlDelay,
        #[cfg(feature = "extensions")]
        11 if key.eq_ignore_ascii_case("clean-param") => DirectiveKind::CleanParam,
        _ => DirectiveKind::Other,
    }
}

fn new_group<'a>(agent: &'a str) -> Group<'a> {
    Group {
        agents: vec![agent],
        rules: Vec::with_capacity(4),
    }
}

/// Shared parser implementation for tolerant and diagnostics-enabled parsing.
///
/// The parser walks the file one line at a time, strips comments and ASCII
/// whitespace, tracks the current user-agent group, and optionally records soft
/// failures as [`ParseWarning`] values.
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

        let directive = classify_directive_key(key);

        match directive {
            DirectiveKind::UserAgent => {
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
                        current = Some(new_group(value));
                        current_has_rules = false;
                    }
                    None => {
                        current = Some(new_group(value));
                    }
                }
            }
            DirectiveKind::Allow | DirectiveKind::Disallow => {
                let Some(group) = current.as_mut() else {
                    if diagnostics {
                        warnings.push(ParseWarning {
                            line: line_number,
                            kind: ParseWarningKind::RuleBeforeUserAgent { key },
                        });
                    }
                    continue;
                };

                let kind = match directive {
                    DirectiveKind::Allow => RuleKind::Allow,
                    DirectiveKind::Disallow => RuleKind::Disallow,
                    _ => unreachable!("only allow/disallow directives reach this branch"),
                };

                group.rules.push(Rule {
                    kind,
                    pattern: value,
                });
                current_has_rules = true;
            }
            _ => {
                #[cfg(feature = "extensions")]
                collect_extension(&mut extensions, current.as_ref(), directive, key, value);
            }
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

/// Applies matching rules from a group to the current best access decision.
///
/// `best` stores the specificity and kind of the strongest matching rule seen
/// so far. More specific patterns replace less specific ones, and `Allow`
/// replaces `Disallow` on ties.
fn apply_group_rules(group: &Group<'_>, path: &str, best: &mut Option<(usize, RuleKind)>) {
    for rule in &group.rules {
        let Some(specificity) = matching_specificity(rule.pattern, path) else {
            continue;
        };

        apply_rule_decision(specificity, rule.kind, best);
    }
}

fn apply_compiled_rules(
    rules: &[CompiledRule<'_>],
    path: &str,
    best: &mut Option<(usize, RuleKind)>,
) {
    for rule in rules {
        let Some(specificity) = rule.matching_specificity(path) else {
            continue;
        };

        apply_rule_decision(specificity, rule.kind, best);
    }
}

fn apply_rule_decision(specificity: usize, kind: RuleKind, best: &mut Option<(usize, RuleKind)>) {
    let should_replace = !matches!(
        *best,
        Some((best_specificity, best_kind))
            if specificity < best_specificity
                || (specificity == best_specificity
                    && !(kind == RuleKind::Allow && best_kind == RuleKind::Disallow))
    );

    if should_replace {
        *best = Some((specificity, kind));
    }
}

fn rule_decision(best: Option<(usize, RuleKind)>) -> bool {
    match best {
        Some((_, RuleKind::Allow)) | None => true,
        Some((_, RuleKind::Disallow)) => false,
    }
}

/// Returns matching specificity for robots longest-match rule selection.
///
/// Patterns without wildcards use the common prefix fast path. A trailing `$`
/// requires the match to consume the whole path but does not increase
/// specificity.
fn matching_specificity(pattern: &str, path: &str) -> Option<usize> {
    if pattern.is_empty() {
        return None;
    }

    let (pattern, anchored) = strip_end_anchor(pattern);
    let matched = if pattern.as_bytes().contains(&b'*') {
        glob_matches(pattern.as_bytes(), path.as_bytes(), anchored)
    } else if anchored {
        path == pattern
    } else {
        path.starts_with(pattern)
    };

    matched.then_some(pattern.len())
}

/// Matches a `*` wildcard pattern against a path byte slice.
///
/// The first pattern segment must match at the start of the path; remaining
/// segments are located in order with SIMD-backed substring search.
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

fn strip_end_anchor(pattern: &str) -> (&str, bool) {
    match pattern.strip_suffix('$') {
        Some(pattern) => (pattern, true),
        None => (pattern, false),
    }
}

#[cfg(feature = "extensions")]
/// Stores a non-core directive in the feature-gated extension metadata.
///
/// Extension directives intentionally do not alter group boundaries or access
/// rules. `Crawl-delay` snapshots the current group agents so callers can
/// associate the value with the group where it appeared.
fn collect_extension<'a>(
    extensions: &mut Extensions<'a>,
    current: Option<&Group<'a>>,
    directive: DirectiveKind,
    key: &'a str,
    value: &'a str,
) {
    match directive {
        DirectiveKind::Sitemap => {
            if !value.is_empty() {
                extensions.sitemaps.push(value);
            }
        }
        DirectiveKind::CrawlDelay => {
            extensions.crawl_delays.push(CrawlDelay {
                agents: current
                    .map(|group| group.agents.clone())
                    .unwrap_or_default(),
                value,
            });
        }
        DirectiveKind::Host => {
            if !value.is_empty() {
                extensions.hosts.push(value);
            }
        }
        DirectiveKind::CleanParam => {
            if !value.is_empty() {
                extensions.clean_params.push(CleanParam { value });
            }
        }
        _ => {
            extensions.other.push(Directive { key, value });
        }
    }
}

/// Removes an inline `#` comment from a line.
fn strip_comment(line: &str) -> &str {
    match memchr(b'#', line.as_bytes()) {
        Some(index) => &line[..index],
        None => line,
    }
}

/// Splits a directive line into raw key and value slices.
///
/// Only the first `:` is structural; additional colons remain part of the value.
fn split_directive(line: &str) -> Option<(&str, &str)> {
    let index = memchr(b':', line.as_bytes())?;
    Some((&line[..index], &line[index + 1..]))
}

/// Trims ASCII spaces and tabs from both ends of a directive fragment.
///
/// Robots directives are byte-oriented, so this deliberately avoids full
/// Unicode whitespace handling.
fn trim_ascii(value: &str) -> &str {
    let bytes = value.as_bytes();
    let Some((&first, rest)) = bytes.split_first() else {
        return value;
    };
    let last = rest.last().copied().unwrap_or(first);

    if !matches!(first, b' ' | b'\t') && !matches!(last, b' ' | b'\t') {
        return value;
    }

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

/// Iterator over input lines with one-based source line numbers.
///
/// Handles both LF and CRLF endings while keeping returned line slices borrowed
/// from the original input.
struct Lines<'a> {
    input: &'a str,
    offset: usize,
    line: usize,
}

impl<'a> Lines<'a> {
    /// Creates a line iterator for `input`.
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

    /// Returns the next line and its one-based line number.
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
    fn parses_directive_keys_case_insensitively() {
        let robots =
            RobotsTxt::parse("uSeR-aGeNt: FooBot\nDiSaLlOw: /private\nAlLoW: /private/public\n");

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
    fn compiled_matcher_matches_regular_matcher_for_core_rules() {
        let robots = RobotsTxt::parse(
            "User-agent: FooBot\n\
            Disallow: /foo\n\
            \n\
            User-agent: FooBot\n\
            Disallow: /bar\n\
            Allow: /bar/public\n\
            Disallow: /tie\n\
            Allow: /tie\n\
            \n\
            User-agent: ImageBot\n\
            Disallow: /*.gif$\n\
            Allow: /public/*.gif$\n\
            \n\
            User-agent: *\n\
            Disallow: /fallback\n",
        );
        let matcher = robots.matcher();

        for (agent, path) in [
            ("FooBot", "/foo/page"),
            ("FooBot", "/bar/page"),
            ("FooBot", "/bar/public/page"),
            ("FooBot", "/tie"),
            ("ImageBot", "/images/a.gif"),
            ("ImageBot", "/images/a.gif?size=large"),
            ("ImageBot", "/public/a.gif"),
            ("OtherBot", "/fallback/page"),
            ("OtherBot", "/public/page"),
            ("OtherBot", "/robots.txt"),
        ] {
            assert_eq!(
                matcher.is_allowed(agent, path),
                robots.is_allowed(agent, path),
                "compiled matcher differed for {agent} {path}"
            );
        }
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
