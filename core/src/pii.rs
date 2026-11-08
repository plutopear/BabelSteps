//! Pattern detection of personal data and secrets in text (text half of B13).
//!
//! **Detection misses things and sometimes flags harmless text.** It works from
//! patterns, so a password typed into an ordinary field, a name, a street
//! address or a phone number in an unusual format all pass straight through.
//! Nothing here may be used to claim that text or an export is free of
//! sensitive data. The user reviews before exporting.
//!
//! Known false positives, accepted because a wrong placeholder is cheaper than
//! a leak: a four-part version number such as `1.2.3.4` is a valid IPv4
//! address, any 10-digit number that fits North American numbering looks like
//! a phone number, about one 13-19 digit number in ten passes the Luhn check,
//! and UUIDs, hashes and long digit-bearing paths look like tokens.
//!
//! All offsets are byte offsets into the input.

use regex::{Regex, RegexBuilder};
use std::fmt;
use std::net::{Ipv4Addr, Ipv6Addr}; // Address parsers only. Nothing here opens a socket.
use std::sync::OnceLock;

/// What a match was detected as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Email,
    Phone,
    CardNumber,
    Ssn,
    IpAddress,
    Token,
    PrivateKey,
    Custom,
}

impl Kind {
    /// The text that replaces a match of this kind.
    pub fn placeholder(self) -> &'static str {
        match self {
            Kind::Email => "[email]",
            Kind::Phone => "[phone]",
            Kind::CardNumber => "[card number]",
            Kind::Ssn => "[ssn]",
            Kind::IpAddress => "[ip address]",
            Kind::Token => "[token]",
            Kind::PrivateKey => "[private key]",
            Kind::Custom => "[custom]",
        }
    }
}

/// One detected span of the input, in byte offsets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Match {
    pub start: usize,
    pub end: usize,
    pub kind: Kind,
}

/// Why user rules could not be built.
#[derive(Clone, PartialEq, Eq)]
pub enum CustomRuleError {
    /// A custom word or regex does not compile. `index` is 1-based within its
    /// own list (words or regexes), so the UI can point at it.
    InvalidRegex { word: bool, index: usize, pattern: String, reason: String },
    /// A custom word is empty or only whitespace, so it could match nothing useful.
    EmptyWord,
}

/// Kind only: the pattern is the user's own text and the regex message quotes
/// it, so neither goes into logged state. `Display` shows only the position;
/// the UI reads `pattern` and `reason` from the fields when it shows the user.
impl fmt::Debug for CustomRuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CustomRuleError::InvalidRegex { .. } => f.write_str("CustomRuleError::InvalidRegex"),
            CustomRuleError::EmptyWord => f.write_str("CustomRuleError::EmptyWord"),
        }
    }
}

impl fmt::Display for CustomRuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CustomRuleError::InvalidRegex { word, index, .. } => {
                let kind = if *word { "word" } else { "pattern" };
                write!(f, "custom {kind} {index} is not valid")
            }
            CustomRuleError::EmptyWord => write!(f, "a custom word is empty"),
        }
    }
}

impl std::error::Error for CustomRuleError {}

/// Compiled-size cap for a user pattern, so one like `(\w{100}){100}` is
/// refused instead of using lots of memory. Matching time is already linear.
const CUSTOM_REGEX_SIZE_LIMIT: usize = 1 << 20;

/// The user's own words and regexes. Words match whole words, ignoring case.
#[derive(Clone, Default)]
pub struct CustomRules {
    rules: Vec<Regex>,
    words: usize,
}

impl CustomRules {
    /// No custom rules.
    pub fn none() -> Self {
        Self::default()
    }

    /// Builds the rules. A regex that does not compile is an error: it is never
    /// skipped, because the user would believe it was protecting them.
    pub fn new<W, R>(words: W, regexes: R) -> Result<Self, CustomRuleError>
    where
        W: IntoIterator,
        W::Item: AsRef<str>,
        R: IntoIterator,
        R::Item: AsRef<str>,
    {
        let mut rules = Vec::new();
        for (i, word) in words.into_iter().enumerate() {
            let word = word.as_ref().trim();
            if word.is_empty() {
                return Err(CustomRuleError::EmptyWord);
            }
            let is_word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
            let start = if is_word(word.chars().next()) { r"\b" } else { "" };
            let end = if is_word(word.chars().next_back()) { r"\b" } else { "" };
            let pattern = format!("{start}{}{end}", regex::escape(word));
            let rule = RegexBuilder::new(&pattern).case_insensitive(true).build().map_err(|e| {
                CustomRuleError::InvalidRegex {
                    word: true,
                    index: i + 1,
                    pattern: word.to_string(),
                    reason: e.to_string(),
                }
            })?;
            rules.push(rule);
        }
        let words = rules.len();
        for (i, pattern) in regexes.into_iter().enumerate() {
            let pattern = pattern.as_ref();
            let rule = RegexBuilder::new(pattern)
                .size_limit(CUSTOM_REGEX_SIZE_LIMIT)
                .build()
                .map_err(|e| CustomRuleError::InvalidRegex {
                    word: false,
                    index: i + 1,
                    pattern: pattern.to_string(),
                    reason: e.to_string(),
                })?;
            rules.push(rule);
        }
        Ok(Self { rules, words })
    }
}

/// Counts only: custom words are often customer names or internal hostnames,
/// and this type ends up inside logged state.
impl fmt::Debug for CustomRules {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CustomRules")
            .field("words", &self.words)
            .field("regexes", &(self.rules.len() - self.words))
            .finish()
    }
}

/// Replaces every detected span with its placeholder. See the module note:
/// detection misses things, so the result is not guaranteed clean.
pub fn scrub(text: &str, custom: &CustomRules) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pos = 0;
    for m in find(text, custom) {
        out.push_str(&text[pos..m.start]);
        out.push_str(m.kind.placeholder());
        pos = m.end;
    }
    out.push_str(&text[pos..]);
    out
}

/// Finds sensitive spans, sorted by start and never overlapping. Overlapping
/// candidates merge into one span covering all of them, labeled by the longest,
/// so no part of any match is left in the text.
pub fn find(text: &str, custom: &CustomRules) -> Vec<Match> {
    let mut candidates = Vec::new();
    for rule in &custom.rules {
        for m in rule.find_iter(text).filter(|m| !m.is_empty()) {
            candidates.push(Match { start: m.start(), end: m.end(), kind: Kind::Custom });
        }
    }
    find_private_keys(text, &mut candidates);
    find_emails(text, &mut candidates);
    find_cards(text, &mut candidates);
    find_ssns(text, &mut candidates);
    find_phones(text, &mut candidates);
    find_ipv4(text, &mut candidates);
    find_ipv6(text, &mut candidates);
    find_tokens(text, &mut candidates);
    resolve_overlaps(candidates)
}

fn resolve_overlaps(mut candidates: Vec<Match>) -> Vec<Match> {
    // Stable, so on a tie the earlier detector above wins the label.
    candidates.sort_by_key(|m| (m.start, std::cmp::Reverse(m.end - m.start)));
    let mut merged: Vec<(Match, usize)> = Vec::new();
    for c in candidates {
        let c_len = c.end - c.start;
        match merged.last_mut() {
            Some((last, label_len)) if c.start < last.end => {
                last.end = last.end.max(c.end);
                if c_len > *label_len {
                    last.kind = c.kind;
                    *label_len = c_len;
                }
            }
            _ => merged.push((c, c_len)),
        }
    }
    merged.into_iter().map(|(m, _)| m).collect()
}

macro_rules! static_regex {
    ($pattern:expr) => {{
        static RE: OnceLock<Regex> = OnceLock::new();
        RE.get_or_init(|| Regex::new($pattern).expect("built-in pattern compiles"))
    }};
}

/// Runs `re`, keeping accepted matches. A rejected match retries one character
/// later, because the regex engine would otherwise skip a valid match inside it.
fn scan(re: &Regex, text: &str, kind: Kind, accept: impl Fn(&str) -> bool, out: &mut Vec<Match>) {
    let mut pos = 0;
    while let Some(m) = re.find_at(text, pos) {
        if accept(m.as_str()) && number_bounded(text, m.start(), m.end(), ".-") {
            out.push(Match { start: m.start(), end: m.end(), kind });
            pos = m.end();
        } else {
            let step = text[m.start()..].chars().next().map_or(1, char::len_utf8);
            pos = m.start() + step;
        }
        if pos >= text.len() {
            break;
        }
    }
}

/// True if the span stands alone rather than being part of a longer number
/// such as `1.2.3.4.5`. `joiners` are the characters that continue a number
/// when a digit follows them.
fn number_bounded(text: &str, start: usize, end: usize, joiners: &str) -> bool {
    let mut before = text[..start].chars().rev();
    let prev = before.next();
    if prev.is_some_and(|c| c.is_alphanumeric() || c == '_') {
        return false;
    }
    if prev.is_some_and(|c| joiners.contains(c)) && before.next().is_some_and(|c| c.is_ascii_digit()) {
        return false;
    }
    let mut after = text[end..].chars();
    let next = after.next();
    if next.is_some_and(|c| c.is_alphanumeric() || c == '_') {
        return false;
    }
    if next.is_some_and(|c| joiners.contains(c)) && after.next().is_some_and(|c| c.is_ascii_digit()) {
        return false;
    }
    true
}

fn find_private_keys(text: &str, out: &mut Vec<Match>) {
    // A block with no END line runs to the end of the text, so a truncated
    // paste is still covered.
    let re = static_regex!(
        r"(?s)-----BEGIN [A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-----.*?(?:-----END [A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-----|\z)"
    );
    for m in re.find_iter(text) {
        out.push(Match { start: m.start(), end: m.end(), kind: Kind::PrivateKey });
    }
}

fn find_emails(text: &str, out: &mut Vec<Match>) {
    let re = static_regex!(r"[\p{L}\p{N}._%+\-]+@[\p{L}\p{N}\-]+(?:\.[\p{L}\p{N}\-]+)*\.\p{L}{2,}");
    for m in re.find_iter(text) {
        out.push(Match { start: m.start(), end: m.end(), kind: Kind::Email });
    }
}

fn find_cards(text: &str, out: &mut Vec<Match>) {
    let re = static_regex!(r"[0-9](?:[ \-]?[0-9])*");
    for run in re.find_iter(text) {
        if !number_bounded(text, run.start(), run.end(), ".-") {
            continue;
        }
        // Whole digit groups only, so a card after an unrelated number
        // ("12 4111 1111 1111 1111") is still found.
        let mut groups: Vec<(usize, usize)> = Vec::new();
        let mut group_start = None;
        for (i, c) in run.as_str().char_indices() {
            match (c.is_ascii_digit(), group_start) {
                (true, None) => group_start = Some(i),
                (false, Some(s)) => {
                    groups.push((s, i));
                    group_start = None;
                }
                _ => {}
            }
        }
        if let Some(s) = group_start {
            groups.push((s, run.as_str().len()));
        }
        let mut first = 0;
        while first < groups.len() {
            let found = (first..groups.len()).rev().find(|&last| {
                let digits: Vec<u8> = run.as_str()[groups[first].0..groups[last].1]
                    .bytes()
                    .filter(u8::is_ascii_digit)
                    .map(|b| b - b'0')
                    .collect();
                is_card_number(&digits)
            });
            match found {
                Some(last) => {
                    out.push(Match {
                        start: run.start() + groups[first].0,
                        end: run.start() + groups[last].1,
                        kind: Kind::CardNumber,
                    });
                    first = last + 1;
                }
                None => first += 1,
            }
        }
    }
}

fn is_card_number(digits: &[u8]) -> bool {
    (13..=19).contains(&digits.len()) && digits.iter().any(|d| *d != digits[0]) && luhn_valid(digits)
}

fn luhn_valid(digits: &[u8]) -> bool {
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &d)| {
            let d = u32::from(d);
            match (i % 2 == 1, d * 2) {
                (false, _) => d,
                (true, doubled) if doubled > 9 => doubled - 9,
                (true, doubled) => doubled,
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

fn find_ssns(text: &str, out: &mut Vec<Match>) {
    let re = static_regex!(r"[0-9]{3}[ \-][0-9]{2}[ \-][0-9]{4}");
    scan(re, text, Kind::Ssn, is_ssn, out);
}

/// `s` is `ddd?dd?dddd`. Rejects mixed separators and numbers the US never issues.
fn is_ssn(s: &str) -> bool {
    let b = s.as_bytes();
    b[3] == b[6] && !s.starts_with("000") && !s.starts_with("666") && b[0] != b'9' && &s[4..6] != "00" && &s[7..] != "0000"
}

fn find_phones(text: &str, out: &mut Vec<Match>) {
    // North American, with separators.
    let separated = static_regex!(
        r"(?:\+?1[ .\-]?)?(?:\([0-9]{3}\)[ .\-]?|[0-9]{3}[ .\-])[0-9]{3}[ .\-][0-9]{4}"
    );
    scan(separated, text, Kind::Phone, |_| true, out);
    // North American, bare. The rules for area code and exchange exclude most
    // other 10-digit numbers, including current Unix timestamps.
    let bare = static_regex!(r"(?:\+?1)?[2-9][0-9]{2}[2-9][0-9]{6}");
    scan(bare, text, Kind::Phone, |_| true, out);
    // International: needs the leading +.
    let international = static_regex!(r"\+[0-9]{1,3}(?:[ .\-]?\(?[0-9]{1,4}\)?){2,6}");
    let digit_count = |s: &str| s.bytes().filter(u8::is_ascii_digit).count();
    scan(international, text, Kind::Phone, |s| (8..=15).contains(&digit_count(s)), out);
}

fn find_ipv4(text: &str, out: &mut Vec<Match>) {
    let re = static_regex!(r"[0-9]{1,3}(?:\.[0-9]{1,3}){3}");
    let mut pos = 0;
    while let Some(m) = re.find_at(text, pos) {
        // Only '.' continues an address: "10.0.0.1-10.0.0.5" is two addresses.
        if m.as_str().parse::<Ipv4Addr>().is_ok() && number_bounded(text, m.start(), m.end(), ".") {
            out.push(Match { start: m.start(), end: m.end(), kind: Kind::IpAddress });
            pos = m.end();
        } else {
            pos = m.start() + 1; // The match starts with an ASCII digit.
        }
        if pos >= text.len() {
            break;
        }
    }
}

fn find_ipv6(text: &str, out: &mut Vec<Match>) {
    let re = static_regex!(r"[0-9A-Fa-f.:]*:[0-9A-Fa-f.]*:[0-9A-Fa-f.:]*");
    for m in re.find_iter(text) {
        let raw = m.as_str();
        // Punctuation next to the address is not part of it ("at fe80::1.").
        let mut body = raw.trim_start_matches('.').trim_end_matches('.');
        if body.ends_with(':') && !body.ends_with("::") {
            body = &body[..body.len() - 1];
        }
        if body.starts_with(':') && !body.starts_with("::") {
            body = &body[1..];
        }
        let start = m.start() + raw.find(body).unwrap_or(0);
        let end = start + body.len();
        // Requiring a digit keeps out "cafe::bad", and with it bare "::".
        if body.bytes().any(|b| b.is_ascii_digit())
            && body.parse::<Ipv6Addr>().is_ok()
            && number_bounded(text, start, end, "")
        {
            out.push(Match { start, end, kind: Kind::IpAddress });
        }
    }
}

fn find_tokens(text: &str, out: &mut Vec<Match>) {
    let known = static_regex!(
        r"\b(?:gh[pousr]_[A-Za-z0-9]{16,}|github_pat_[A-Za-z0-9_]{20,}|(?:AKIA|ASIA)[0-9A-Z]{16}|xox[a-zA-Z0-9]-[A-Za-z0-9\-]{8,}|glpat-[A-Za-z0-9_\-]{16,}|AIza[0-9A-Za-z_\-]{30,}|[sr]k_(?:live|test)_[A-Za-z0-9]{16,}|npm_[A-Za-z0-9]{30,}|eyJ[A-Za-z0-9_\-]{5,}\.[A-Za-z0-9_\-]{5,}\.[A-Za-z0-9_\-]*)"
    );
    for m in known.find_iter(text) {
        out.push(Match { start: m.start(), end: m.end(), kind: Kind::Token });
    }
    // The digit requirement keeps out "sk-learn-tutorials-and-guides".
    let sk = static_regex!(r"\bsk-[A-Za-z0-9_\-]{16,}");
    for m in sk.find_iter(text).filter(|m| m.as_str().bytes().any(|b| b.is_ascii_digit())) {
        out.push(Match { start: m.start(), end: m.end(), kind: Kind::Token });
    }
    let long = static_regex!(r"[A-Za-z0-9+/=_\-]{32,}");
    for m in long.find_iter(text).filter(|m| looks_random(m.as_str())) {
        out.push(Match { start: m.start(), end: m.end(), kind: Kind::Token });
    }
}

/// Heuristic for a 32+ character run of token characters. Either it has a real
/// share of digits (hex, base64, UUIDs) or it is mixed-case with entropy above
/// what joined English words reach. Long words and identifiers fail both.
fn looks_random(s: &str) -> bool {
    let count = |f: fn(&u8) -> bool| s.as_bytes().iter().filter(|b| f(b)).count();
    let lower = count(u8::is_ascii_lowercase);
    let upper = count(u8::is_ascii_uppercase);
    let digits = count(u8::is_ascii_digit);
    let letters = lower + upper;
    let entropy = shannon_entropy(s);
    let digit_share_ok = letters > 0 && digits * 20 >= letters + digits;
    (digit_share_ok && entropy >= 3.0) || (lower > 0 && upper > 0 && entropy >= 4.3)
}

/// Bits per byte.
fn shannon_entropy(s: &str) -> f64 {
    let mut counts = [0u32; 256];
    for b in s.bytes() {
        counts[usize::from(b)] += 1;
    }
    let total = s.len() as f64;
    counts
        .iter()
        .filter(|c| **c > 0)
        .map(|c| {
            let p = f64::from(*c) / total;
            -p * p.log2()
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(text: &str) -> String {
        scrub(text, &CustomRules::none())
    }

    /// Each value must be replaced whole, in the middle of a sentence.
    fn assert_found(kind: Kind, values: &[&str]) {
        for value in values {
            let text = format!("before {value} after");
            let want = format!("before {} after", kind.placeholder());
            assert_eq!(plain(&text), want, "input: {text}");
        }
    }

    /// The text must come back untouched.
    fn assert_clean(values: &[&str]) {
        for value in values {
            assert_eq!(plain(value), *value);
        }
    }

    #[test]
    fn finds_emails() {
        assert_found(Kind::Email, &["a@example.com", "first.last+tag@mail.example.co.uk", "josé@example.org"]);
    }

    #[test]
    fn finds_phones() {
        assert_found(
            Kind::Phone,
            &[
                "555-123-4567",
                "(555) 123-4567",
                "555.123.4567",
                "+1 555 123 4567",
                "1-555-123-4567",
                "2025550143",
                "+44 20 7946 0958",
                "+33 1 23 45 67 89",
            ],
        );
    }

    #[test]
    fn finds_card_numbers() {
        assert_found(
            Kind::CardNumber,
            &[
                "4111111111111111",
                "4111 1111 1111 1111",
                "4111-1111-1111-1111",
                "378282246310005",
                "6011 1111 1111 1117",
                "4222222222222",
            ],
        );
    }

    #[test]
    fn finds_a_card_after_an_unrelated_number() {
        assert_eq!(plain("12 4111 1111 1111 1111"), "12 [card number]");
    }

    #[test]
    fn finds_ssns() {
        assert_found(Kind::Ssn, &["123-45-6789", "123 45 6789"]);
    }

    #[test]
    fn finds_ip_addresses() {
        assert_found(
            Kind::IpAddress,
            &["192.168.1.1", "10.0.0.1", "2001:db8::8a2e:370:7334", "fe80::1", "::1", "::ffff:192.0.2.1", "2001:0db8:0000:0000:0000:ff00:0042:8329"],
        );
        assert_eq!(plain("at fe80::1."), "at [ip address].");
        assert_eq!(plain("10.0.0.1:8080"), "[ip address]:8080");
        assert_eq!(plain("10.0.0.1-10.0.0.5"), "[ip address]-[ip address]");
        assert_eq!(plain("[::1]:8080"), "[[ip address]]:8080");
    }

    #[test]
    fn finds_tokens() {
        assert_found(
            Kind::Token,
            &[
                "sk-abc123def456ghi789jkl",
                "ghp_abcdefghijklmnopqrstuvwxyz0123456789",
                "AKIAIOSFODNN7EXAMPLE",
                "xoxb-123456789-abcdef",
                "xoxp-123456789-abcdef",
                "glpat-abcdefghij0123456789",
                "sk_live_abcdefghij0123456789",
                "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.abc123_def-456",
                // High entropy: hex, base64 and letters-only.
                "5d41402abc4b2a76b9719d911017c592",
                "dGhpcyBpcyBhIHNlY3JldCB2YWx1ZSBmb3IgdGVzdGluZzEy",
                "QwErTyUiOpAsDfGhJkLzXcVbNmQaZwSxEdCrFvTg",
            ],
        );
    }

    #[test]
    fn finds_private_keys() {
        let block = "-----BEGIN RSA PRIVATE KEY-----\nMIIEow\nabc\n-----END RSA PRIVATE KEY-----";
        assert_found(Kind::PrivateKey, &[block, "-----BEGIN PRIVATE KEY-----\nabc\n-----END PRIVATE KEY-----"]);
        assert_found(Kind::PrivateKey, &["-----BEGIN OPENSSH PRIVATE KEY-----\nx\n-----END OPENSSH PRIVATE KEY-----"]);
        // A truncated block still hides everything after the header.
        assert_eq!(plain("key: -----BEGIN PRIVATE KEY-----\nMIIEow secret"), "key: [private key]");
    }

    #[test]
    fn leaves_version_numbers_alone() {
        assert_clean(&["1.2.3.4.5", "version 1.2.3.4.5 released", "v1.2.3.4", "1.2.3", "10.20.30.40.50.60"]);
    }

    #[test]
    fn leaves_invalid_card_numbers_alone() {
        assert_clean(&["4111111111111112", "4111 1111 1111 1112", "0000000000000000", "411111111111111111111"]);
    }

    #[test]
    fn leaves_ordinary_long_words_alone() {
        assert_clean(&[
            "antidisestablishmentarianism",
            "pneumonoultramicroscopicsilicovolcanoconiosis",
            "getUserAccountInformationFromDatabaseServer",
            "this-is-a-very-long-kebab-case-identifier-name",
            "my_long_snake_case_variable_name_for_testing_purposes",
            "task-management-system-overview",
            "sk-learn-tutorials-and-guides",
        ]);
    }

    #[test]
    fn leaves_low_entropy_runs_alone() {
        assert_clean(&["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "abc123abc123abc123abc123abc123abc123", "00000000-0000-0000-0000-000000000000"]);
    }

    #[test]
    fn leaves_dates_and_times_alone() {
        assert_clean(&["2026-11-02", "11/02/2026", "02.11.2026", "2026-11-02T10:30:45", "10:30:45", "12:30"]);
    }

    #[test]
    fn leaves_other_lookalikes_alone() {
        assert_clean(&[
            "std::fmt::Debug",
            "cafe::bad",
            "::",
            "aa:bb:cc:dd:ee:ff",
            "999.1.1.1",
            "000-12-3456",
            "666-12-3456",
            "900-12-3456",
            "123-00-6789",
            "123-45-0000",
            "123-45 6789",
            "user@localhost",
            "call me at 5pm",
            "I have 1234 items",
        ]);
    }

    #[test]
    fn debug_of_custom_rules_and_errors_shows_no_patterns() {
        let rules = CustomRules::new(["zebracorp"], ["quokka[0-9]+"]).unwrap();
        let shown = format!("{rules:?}");
        assert!(!shown.contains("zebracorp") && !shown.contains("quokka"), "{shown}");
        let err = CustomRules::new([] as [&str; 0], ["quokka("]).unwrap_err();
        assert!(matches!(err, CustomRuleError::InvalidRegex { .. }));
        let shown = format!("{err:?}");
        assert!(!shown.contains("quokka"), "{shown}");
        // Display may reach a log, so it gives the position only.
        let shown = err.to_string();
        assert!(!shown.contains("quokka"), "{shown}");
        assert_eq!(shown, "custom pattern 1 is not valid");
    }

    #[test]
    fn custom_words_match_whole_words_ignoring_case() {
        let rules = CustomRules::new(["acme", "corp.local"], [] as [&str; 0]).unwrap();
        assert_eq!(scrub("ACME and Acme", &rules), "[custom] and [custom]");
        assert_eq!(scrub("acmeon", &rules), "acmeon");
        assert_eq!(scrub("db.corp.local:1", &rules), "db.[custom]:1");
        assert_eq!(scrub("corpxlocal", &rules), "corpxlocal");
    }

    #[test]
    fn custom_regexes_match() {
        let rules = CustomRules::new([] as [&str; 0], [r"PROJ-[0-9]+"]).unwrap();
        assert_eq!(scrub("see PROJ-42 now", &rules), "see [custom] now");
    }

    #[test]
    fn invalid_custom_regex_is_an_error() {
        let result = CustomRules::new([] as [&str; 0], ["ok", "(unclosed"]);
        match result {
            Err(CustomRuleError::InvalidRegex { pattern, .. }) => assert_eq!(pattern, "(unclosed"),
            other => panic!("expected an error, got {other:?}"),
        }
    }

    #[test]
    fn oversize_custom_regex_is_an_error() {
        let result = CustomRules::new([] as [&str; 0], [r"(\w{200}){200}"]);
        assert!(matches!(result, Err(CustomRuleError::InvalidRegex { .. })));
        // Everyday patterns, including Unicode classes, stay well under the cap.
        assert!(CustomRules::new([] as [&str; 0], [r"\w+@\w+\.\w+", r"(?i)project-\d{3,6}", r"\b[A-Z]{2}\d{6}\b"]).is_ok());
    }

    #[test]
    fn blank_custom_word_is_an_error() {
        let result = CustomRules::new(["  "], [] as [&str; 0]);
        assert!(matches!(result, Err(CustomRuleError::EmptyWord)));
    }

    #[test]
    fn empty_matching_regex_replaces_nothing() {
        let rules = CustomRules::new([] as [&str; 0], ["x*"]).unwrap();
        assert_eq!(scrub("abc", &rules), "abc");
    }

    #[test]
    fn debug_hides_custom_words() {
        let rules = CustomRules::new(["acme"], ["zeta[0-9]"]).unwrap();
        let shown = format!("{rules:?}");
        assert!(!shown.contains("acme") && !shown.contains("zeta"), "{shown}");
    }

    #[test]
    fn overlaps_resolve_to_the_longest_match() {
        // The word "example" alone is shorter than the email around it.
        let rules = CustomRules::new(["example"], [] as [&str; 0]).unwrap();
        assert_eq!(scrub("a@example.com", &rules), "[email]");
        // The IPv4 inside a mapped IPv6 address is shorter than the whole.
        assert_eq!(
            find("::ffff:192.0.2.1", &CustomRules::none()),
            vec![Match { start: 0, end: 16, kind: Kind::IpAddress }]
        );
    }

    #[test]
    fn partly_overlapping_matches_leave_nothing_behind() {
        let rules = CustomRules::new([] as [&str; 0], ["abcdef", "defghij"]).unwrap();
        assert_eq!(scrub("xx abcdefghij yy", &rules), "xx [custom] yy");
    }

    #[test]
    fn find_reports_sorted_byte_offsets() {
        let text = "é a@b.co then 10.0.0.1";
        let found = find(text, &CustomRules::none());
        assert_eq!(found.len(), 2);
        assert_eq!(&text[found[0].start..found[0].end], "a@b.co");
        assert_eq!(found[0].kind, Kind::Email);
        assert_eq!(&text[found[1].start..found[1].end], "10.0.0.1");
        assert_eq!(found[1].kind, Kind::IpAddress);
    }

    #[test]
    fn handles_empty_and_non_ascii_text() {
        assert_eq!(plain(""), "");
        assert_eq!(plain("日本語のテキスト 123"), "日本語のテキスト 123");
    }
}
