//! HTML exporter: one self-contained, print-ready document (no script, no
//! remote loads). It is also what the webview prints to PDF (US Letter).
//!
//! Every field is HTML-escaped first; only then is the restricted inline
//! Markdown (`**`, `*`, `<u>`, inline code, line breaks) converted. Because
//! escaping comes first, user text can never introduce a tag: the only tags in
//! the output are ones this module writes. The user's `<u>` / `</u>` appear in
//! escaped form (`&lt;u&gt;`) and are turned back into real tags only when exact.

use crate::export::validate_image_path;
use crate::markdown::{encode_path, WITHHELD_SENTENCE};
use crate::model::{Block, CalloutKind, Recording};

/// Shown in place of an image whose path is not a plain `images/...` path.
pub const IMAGE_UNAVAILABLE: &str = "Image unavailable.";

pub const CSP: &str = "default-src 'none'; img-src 'self' file: data:; style-src 'unsafe-inline'";

const CSS: &str = "\
@page { size: letter; margin: 0.75in; }
html { background: #fff; }
body { color: #000; background: #fff; font-family: -apple-system, 'Segoe UI', Roboto, Helvetica, Arial, sans-serif; font-size: 11pt; line-height: 1.45; max-width: 7in; margin: 0 auto; }
h1, h2, h3, h4, h5, h6 { page-break-after: avoid; break-after: avoid; line-height: 1.25; }
h1 { font-size: 22pt; margin: 0 0 0.6em; }
h2 { font-size: 15pt; margin: 1.4em 0 0.4em; }
h3 { font-size: 13pt; } h4, h5, h6 { font-size: 11.5pt; }
p { margin: 0.5em 0; orphans: 3; widows: 3; }
code { font-family: Consolas, 'Courier New', monospace; font-size: 0.95em; border: 1px solid #999; padding: 0 0.2em; }
ul, ol { margin: 0.5em 0; padding-left: 1.6em; }
li { margin: 0.15em 0; }
.step { margin-top: 1.4em; }
.step-head { page-break-inside: avoid; break-inside: avoid; page-break-after: avoid; break-after: avoid; }
.step-head h2 { margin-top: 0; }
figure { margin: 0.6em 0; page-break-inside: avoid; break-inside: avoid; }
figure img { max-width: 100%; height: auto; border: 1px solid #000; }
figcaption { font-size: 10pt; margin-top: 0.3em; }
.step-image { page-break-before: avoid; break-before: avoid; }
.withheld { font-style: italic; border: 1px dashed #000; padding: 0.4em 0.6em; }
.callout { margin: 0.8em 0; padding: 0.5em 0.8em; border: 1px solid #000; border-left-width: 8px; page-break-inside: avoid; break-inside: avoid; }
.callout .label { font-weight: bold; text-transform: uppercase; letter-spacing: 0.04em; }
.callout-note { border-left-style: solid; background: #f2f2f2; }
.callout-tip { border-left-style: double; background: #fff; }
.callout-warning { border-style: solid; border-width: 2px 2px 2px 10px; background: #e0e0e0; }
";

/// Escape text for HTML element content and quoted attribute values.
pub fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Convert already-escaped text. Unmatched markers stay literal.
fn inline_escaped(s: &str) -> String {
    const U_OPEN: &str = "&lt;u&gt;";
    const U_CLOSE: &str = "&lt;/u&gt;";
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    // A closing marker missing from the rest of the text is missing from every
    // later rest too, so it is searched for only until the first miss.
    let (mut no_tick, mut no_bold, mut no_em, mut no_u) = (false, false, false, false);
    while i < s.len() {
        let rest = &s[i..];
        if let Some(after) = rest.strip_prefix('`') {
            let found = if no_tick { None } else { after.find('`') };
            no_tick |= found.is_none();
            if let Some(end) = found {
                out.push_str("<code>");
                out.push_str(&after[..end]);
                out.push_str("</code>");
                i += 1 + end + 1;
                continue;
            }
        } else if let Some(after) = rest.strip_prefix("**") {
            let found = if no_bold { None } else { after.find("**") };
            no_bold |= found.is_none();
            if let Some(end) = found.filter(|e| *e > 0) {
                out.push_str("<strong>");
                out.push_str(&inline_escaped(&after[..end]));
                out.push_str("</strong>");
                i += 2 + end + 2;
                continue;
            }
        } else if let Some(after) = rest.strip_prefix('*') {
            let found = if no_em { None } else { after.find('*') };
            no_em |= found.is_none();
            if let Some(end) = found.filter(|e| *e > 0) {
                out.push_str("<em>");
                out.push_str(&inline_escaped(&after[..end]));
                out.push_str("</em>");
                i += 1 + end + 1;
                continue;
            }
        } else if let Some(after) = rest.strip_prefix(U_OPEN) {
            let found = if no_u { None } else { after.find(U_CLOSE) };
            no_u |= found.is_none();
            if let Some(end) = found {
                out.push_str("<u>");
                out.push_str(&inline_escaped(&after[..end]));
                out.push_str("</u>");
                i += U_OPEN.len() + end + U_CLOSE.len();
                continue;
            }
        }
        let ch = rest.chars().next().unwrap();
        match ch {
            '\r' => {}
            '\n' => out.push_str("<br>\n"),
            _ => out.push(ch),
        }
        i += ch.len_utf8();
    }
    out
}

/// Restricted inline Markdown to HTML. Surrounding blank lines are trimmed.
fn rich(s: &str) -> String {
    let lines: Vec<&str> = s.split(['\n', '\r']).map(str::trim).filter(|l| !l.is_empty()).collect();
    inline_escaped(&escape_html(&lines.join("\n")))
}

fn src_attr(path: &str) -> String {
    escape_html(&encode_path(path))
}

pub fn to_html(rec: &Recording) -> String {
    let title = escape_html(rec.title.trim());
    let mut out = String::new();
    out.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    out.push_str(&format!("<meta http-equiv=\"Content-Security-Policy\" content=\"{CSP}\">\n"));
    out.push_str(&format!("<title>{title}</title>\n<style>\n{CSS}</style>\n</head>\n<body>\n"));
    out.push_str(&format!("<h1>{title}</h1>\n"));

    let mut step_no = 0usize;
    for block in &rec.blocks {
        match block {
            Block::Heading { level, text, .. } => {
                let l = (*level).clamp(1, 6);
                out.push_str(&format!("<h{l}>{}</h{l}>\n", escape_html(text.trim())));
            }
            Block::Text { text, .. } => {
                let body = rich(text);
                if !body.is_empty() {
                    out.push_str(&format!("<p>{body}</p>\n"));
                }
            }
            Block::Step(step) => {
                step_no += 1;
                out.push_str("<section class=\"step\">\n<div class=\"step-head\">\n");
                out.push_str(&format!("<h2>Step {step_no}. {}</h2>\n", escape_html(step.title.trim())));
                out.push_str("</div>\n");
                let desc = rich(&step.description);
                if !desc.is_empty() {
                    out.push_str(&format!("<p>{desc}</p>\n"));
                }
                if step.screenshot_withheld {
                    // Privacy: never emit the image of a withheld step.
                    out.push_str(&format!("<p class=\"withheld\">{WITHHELD_SENTENCE}</p>\n"));
                } else if let Some(img) = &step.screenshot {
                    if validate_image_path(img) {
                        out.push_str(&format!(
                            "<figure class=\"step-image\"><img src=\"{}\" alt=\"Step {step_no}\"></figure>\n",
                            src_attr(img)
                        ));
                    } else {
                        out.push_str(&format!("<p class=\"withheld\">{IMAGE_UNAVAILABLE}</p>\n"));
                    }
                }
                out.push_str("</section>\n");
            }
            Block::List { ordered, items, .. } => {
                if !items.is_empty() {
                    let tag = if *ordered { "ol" } else { "ul" };
                    out.push_str(&format!("<{tag}>\n"));
                    for it in items {
                        out.push_str(&format!("<li>{}</li>\n", rich(it)));
                    }
                    out.push_str(&format!("</{tag}>\n"));
                }
            }
            Block::Image { image, caption, .. } => {
                if validate_image_path(image) {
                    out.push_str(&format!("<figure><img src=\"{}\" alt=\"Image\">", src_attr(image)));
                } else {
                    out.push_str(&format!("<figure><p class=\"withheld\">{IMAGE_UNAVAILABLE}</p>"));
                }
                let cap = rich(caption);
                if !cap.is_empty() {
                    out.push_str(&format!("<figcaption>{cap}</figcaption>"));
                }
                out.push_str("</figure>\n");
            }
            Block::Callout { kind, text, .. } => {
                let class = match kind {
                    CalloutKind::Note => "note",
                    CalloutKind::Tip => "tip",
                    CalloutKind::Warning => "warning",
                };
                out.push_str(&format!(
                    "<div class=\"callout callout-{class}\"><span class=\"label\">{}:</span> {}</div>\n",
                    kind.label(),
                    rich(text)
                ));
            }
        }
    }
    out.push_str("</body>\n</html>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::tests::full_fixture;
    use crate::model::*;

    #[test]
    fn golden_every_block_kind() {
        let html = to_html(&full_fixture());
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/full.html");
        if std::env::var_os("BLESS_GOLDEN").is_some() {
            std::fs::write(path, &html).unwrap();
        }
        let expected = std::fs::read_to_string(path).expect("golden file missing; run with BLESS_GOLDEN=1");
        assert_eq!(html.replace("\r\n", "\n"), expected.replace("\r\n", "\n"));
    }

    fn evil_recording() -> Recording {
        let evil = "<script>alert(1)</script><img src=x onerror=alert(2)><u onclick=alert(3)>x</u>";
        let mut rec = Recording::new("r", evil, 0);
        rec.blocks.push(Block::Heading { id: "h".into(), level: 2, text: evil.into() });
        rec.blocks.push(Block::Text { id: "t".into(), text: evil.into() });
        rec.blocks.push(Block::List { id: "l".into(), ordered: false, items: vec![evil.into()] });
        rec.blocks.push(Block::Image { id: "i".into(), image: "images/\"onerror=alert(4) x.png".into(), caption: evil.into() });
        rec.blocks.push(Block::Callout { id: "c".into(), kind: CalloutKind::Tip, text: evil.into() });
        rec.blocks.push(Block::Step(StepBlock {
            id: "s".into(),
            action: Action::Manual,
            title: evil.into(),
            description: evil.into(),
            screenshot: Some("images/\"><script>alert(5)</script>.png".into()),
            target: None,
            redactions: vec![],
            captured_unix_ms: 0,
            screenshot_withheld: false,
        }));
        rec
    }

    #[test]
    fn hostile_content_is_inert() {
        let html = to_html(&evil_recording());
        assert!(!html.contains("<script"), "script tag leaked");
        assert!(!html.contains("<img src=x"), "injected img leaked");
        assert!(!html.contains("<u onclick"), "u with attributes leaked");
        // Both image paths are invalid, so no `<img` at all, only the note.
        assert_eq!(html.matches("<img ").count(), 0);
        assert_eq!(html.matches(IMAGE_UNAVAILABLE).count(), 2);
        // No attribute could be broken out of: the raw quote never follows src=.
        assert!(!html.contains("\"onerror"));
        assert!(!html.contains("\"><script"));
        // Only the tags we emit exist.
        let mut rest = html.as_str();
        let allowed = ["html", "head", "meta", "title", "style", "body", "h1", "h2", "h3", "p", "div", "section", "span", "figure", "figcaption", "img", "ul", "li", "br", "u", "code", "strong", "em"];
        while let Some(p) = rest.find('<') {
            rest = &rest[p + 1..];
            let name: String = rest.trim_start_matches('/').chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
            if name.is_empty() || name == "DOCTYPE" || rest.starts_with('!') {
                continue;
            }
            assert!(allowed.contains(&name.as_str()), "unexpected tag <{name}");
        }
    }

    #[test]
    fn unsafe_image_paths_never_reach_the_page() {
        let bad = [
            "file://host/x.png", "file:///C:/x.png", r"\\host\x", "http://example.com/x.png",
            "https://example.com/x.png", "../x.png", "images/../x.png", "/x.png", "C:/x.png", "x.png",
        ];
        for path in bad {
            let mut rec = Recording::new("r", "t", 0);
            rec.blocks.push(Block::Image { id: "i".into(), image: path.into(), caption: String::new() });
            rec.blocks.push(Block::Step(StepBlock {
                id: "s".into(),
                action: Action::Manual,
                title: "x".into(),
                description: String::new(),
                screenshot: Some(path.into()),
                target: None,
                redactions: vec![],
                captured_unix_ms: 0,
                screenshot_withheld: false,
            }));
            let html = to_html(&rec);
            assert!(!html.contains("<img"), "{path}");
            assert!(!html.contains("src="), "{path}");
            assert!(!html.contains("host"), "{path}");
            assert_eq!(html.matches(IMAGE_UNAVAILABLE).count(), 2, "{path}");
        }
        // The control: a good path does produce an image.
        let mut rec = Recording::new("r", "t", 0);
        rec.blocks.push(Block::Image { id: "i".into(), image: "images/a.png".into(), caption: String::new() });
        let html = to_html(&rec);
        assert!(html.contains("<img src=\"images/a.png\""));
        assert!(!html.contains(IMAGE_UNAVAILABLE));
    }

    #[test]
    fn unmatched_markers_stay_literal_and_matched_ones_still_convert() {
        assert_eq!(rich("a ` b ` c ` d"), "a <code> b </code> c ` d");
        assert_eq!(rich("*a* *b"), "<em>a</em> *b");
        assert_eq!(rich("**a** **b"), "<strong>a</strong> **b");
        assert_eq!(rich("**** x"), "**** x");
        assert_eq!(rich("<u>a</u> <u>b"), "<u>a</u> &lt;u&gt;b");
    }

    #[test]
    fn only_exact_u_tags_survive() {
        assert_eq!(rich("a <u>b</u> c"), "a <u>b</u> c");
        assert_eq!(rich("<U>b</U>"), "&lt;U&gt;b&lt;/U&gt;");
        assert_eq!(rich("<u >b</u>"), "&lt;u &gt;b&lt;/u&gt;");
        assert_eq!(rich("<u onclick=x>b</u>"), "&lt;u onclick=x&gt;b</u>".replace("</u>", "&lt;/u&gt;"));
        assert_eq!(rich("<u>unclosed"), "&lt;u&gt;unclosed");
    }

    #[test]
    fn inline_formatting_converts() {
        assert_eq!(rich("**b** and *i* and `c*d*`"), "<strong>b</strong> and <em>i</em> and <code>c*d*</code>");
        assert_eq!(rich("one\ntwo"), "one<br>\ntwo");
        assert_eq!(rich("2 * 3"), "2 * 3");
        assert_eq!(rich("a & b"), "a &amp; b");
    }

    #[test]
    fn head_is_locked_down_and_title_only() {
        let mut rec = full_fixture();
        rec.title = "My guide".into();
        let html = to_html(&rec);
        assert!(html.contains(&format!("content=\"{CSP}\"")));
        
        assert!(html.contains("<title>My guide</title>"));
        assert!(!html.to_lowercase().contains("generator"));
        assert!(!html.contains("<script"));
        assert!(html.contains("@page { size: letter; margin: 0.75in; }"));
        assert!(!html.contains("http://") && !html.contains("https://"));
    }

    #[test]
    fn numbering_matches_markdown_and_withheld_has_no_image() {
        let mut rec = full_fixture();
        if let Some(Block::Step(s)) = rec.blocks.iter_mut().rev().find(|b| matches!(b, Block::Step(_))) {
            s.screenshot = Some("images/secret.png".into());
        }
        let html = to_html(&rec);
        let md = crate::markdown::to_markdown(&rec);
        for n in 1..=3 {
            assert!(html.contains(&format!("Step {n}. ")));
            assert!(md.contains(&format!("## Step {n}. ")));
        }
        assert!(html.contains(WITHHELD_SENTENCE));
        assert!(!html.contains("secret.png"));
    }
}
