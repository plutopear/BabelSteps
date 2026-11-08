//! HTML exporter: one self-contained, print-ready document (no script, no
//! remote loads). It is also what the webview prints to PDF (US Letter).
//!
//! Every field is HTML-escaped first; only then is the restricted inline
//! Markdown (`**`, `*`, `<u>`, inline code, line breaks) converted. Because
//! escaping comes first, user text can never introduce a tag: the only tags in
//! the output are ones this module writes. The user's `<u>` / `</u>` appear in
//! escaped form (`&lt;u&gt;`) and are turned back into real tags only when exact.

use crate::export::validate_image_path;
use crate::markdown::{encode_path, first_block_repeats_title, WITHHELD_SENTENCE};
use crate::export_options::{recorded_on_line, step_time, technical_details, ExportOptions, DETAILS_HEADING, DETAILS_STEPS_HEADING};
use crate::model::{Block, CalloutKind, Recording};
use std::collections::HashMap;

/// Shown in place of an image whose path is not a plain `images/...` path.
pub const IMAGE_UNAVAILABLE: &str = "Image unavailable.";

pub const CSP: &str = "default-src 'none'; img-src 'self' file: data:; style-src 'unsafe-inline'";

/// The single-file export loads nothing at all: its pictures are `data:` URIs.
pub const SINGLE_FILE_CSP: &str = "default-src 'none'; img-src data:; style-src 'unsafe-inline'";

/// Image path (`images/...`) to the `data:` URI that replaces it in a
/// single-file export. A path missing from the map is shown as unavailable.
pub type EmbeddedImages = HashMap<String, String>;

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
.step-time { margin: 0 0 0.3em; font-size: 10pt; }
.recorded-on { margin: 0 0 1em; }
.details { margin-top: 2em; border-top: 1px solid #000; }
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
    to_html_with(rec, &ExportOptions::default())
}

/// HTML with the export options applied (step times, technical details).
pub fn to_html_with(rec: &Recording, opts: &ExportOptions) -> String {
    render(rec, opts, None)
}

/// One self-contained page (B10): the same document, with every picture taken
/// from `images` as a `data:` URI and a policy that allows nothing else. The
/// caller prepares the images (redactions burned in); a picture that is not in
/// `images` is replaced by a note, never by a link to a file.
pub fn to_single_html_with(rec: &Recording, opts: &ExportOptions, images: &EmbeddedImages) -> String {
    render(rec, opts, Some(images))
}

/// What goes in `src`: a relative path for the folder export, a data URI for
/// the single-file export. `None` when there is nothing safe to show.
fn image_src(path: &str, images: Option<&EmbeddedImages>) -> Option<String> {
    if !validate_image_path(path) {
        return None;
    }
    match images {
        None => Some(src_attr(path)),
        Some(map) => map.get(path).map(|uri| escape_html(uri)),
    }
}

fn render(rec: &Recording, opts: &ExportOptions, images: Option<&EmbeddedImages>) -> String {
    let csp = if images.is_some() { SINGLE_FILE_CSP } else { CSP };
    let title = escape_html(rec.title.trim());
    let mut out = String::new();
    out.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    out.push_str(&format!("<meta http-equiv=\"Content-Security-Policy\" content=\"{csp}\">\n"));
    out.push_str(&format!("<title>{title}</title>\n<style>\n{CSS}</style>\n</head>\n<body>\n"));
    out.push_str(&format!("<h1>{title}</h1>\n"));
    if let Some(line) = recorded_on_line(rec, opts) {
        out.push_str(&format!("<p class=\"recorded-on\">{}</p>\n", escape_html(&line)));
    }

    let mut step_no = 0usize;
    let skip = usize::from(first_block_repeats_title(rec));
    for block in rec.blocks.iter().skip(skip) {
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
                if let Some(t) = step_time(rec, step, opts) {
                    out.push_str(&format!("<p class=\"step-time\">Time: {}</p>\n", escape_html(&t)));
                }
                out.push_str("</div>\n");
                let desc = rich(&step.description);
                if !desc.is_empty() {
                    out.push_str(&format!("<p>{desc}</p>\n"));
                }
                if step.screenshot_withheld {
                    // Privacy: never emit the image of a withheld step.
                    out.push_str(&format!("<p class=\"withheld\">{WITHHELD_SENTENCE}</p>\n"));
                } else if let Some(img) = &step.screenshot {
                    if let Some(src) = image_src(img, images) {
                        out.push_str(&format!(
                            "<figure class=\"step-image\"><img src=\"{src}\" alt=\"Step {step_no}\"></figure>\n"
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
                if let Some(src) = image_src(image, images) {
                    out.push_str(&format!("<figure><img src=\"{src}\" alt=\"Image\">"));
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
    if let Some(d) = technical_details(rec, opts) {
        out.push_str(&format!("<section class=\"details\">\n<h2>{DETAILS_HEADING}</h2>\n<ul>\n"));
        for (label, value) in &d.facts {
            out.push_str(&format!("<li>{label}: {}</li>\n", escape_html(value)));
        }
        out.push_str("</ul>\n");
        if !d.steps.is_empty() {
            out.push_str(&format!("<h3>{DETAILS_STEPS_HEADING}</h3>\n<ul>\n"));
            for (n, line) in &d.steps {
                out.push_str(&format!("<li>Step {n}: {}</li>\n", escape_html(line)));
            }
            out.push_str("</ul>\n");
        }
        out.push_str("</section>\n");
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

    /// Every tag in `html` is one the exporter emits, so no text became markup.
    fn assert_only_our_tags(html: &str) {
        let mut rest = html;
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
        assert_only_our_tags(&html);
    }

    #[test]
    fn single_file_hostile_text_and_picture_values_stay_inert() {
        // Same hostile text as the folder export, but with valid picture paths so
        // the single-file branch really embeds pictures next to it.
        let mut rec = evil_recording();
        for b in &mut rec.blocks {
            match b {
                Block::Image { image, .. } => *image = "images/a.png".into(),
                Block::Step(s) => s.screenshot = Some("images/b.png".into()),
                _ => {}
            }
        }
        let mut images = EmbeddedImages::new();
        images.insert("images/a.png".into(), "data:image/png;base64,AAAA".into());
        // A prepared value that tries to leave its attribute must be escaped.
        images.insert("images/b.png".into(), "data:image/png;base64,\" onerror=\"alert(6)".into());
        let html = to_single_html_with(&rec, &ExportOptions::default(), &images);
        assert!(!html.contains("<script"), "script tag leaked");
        assert!(!html.contains("<img src=x") && !html.contains("<u onclick"));
        assert!(!html.contains("\" onerror="), "attribute break-out in a picture value");
        assert_eq!(html.matches("<img ").count(), 2);
        assert_only_our_tags(&html);
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
    fn single_file_embeds_only_what_it_is_given() {
        let mut rec = Recording::new("r", "t", 0);
        rec.blocks.push(Block::Image { id: "i".into(), image: "images/a.png".into(), caption: String::new() });
        rec.blocks.push(Block::Image { id: "j".into(), image: "images/b.png".into(), caption: String::new() });
        rec.blocks.push(Block::Image { id: "k".into(), image: "http://example.com/x.png".into(), caption: String::new() });
        let mut images = EmbeddedImages::new();
        images.insert("images/a.png".into(), "data:image/png;base64,AAAA".into());
        // A path that is valid but was not prepared is a note, never a file link; so is an unsafe one.
        let html = to_single_html_with(&rec, &ExportOptions::default(), &images);
        assert_eq!(html.matches("<img src=\"data:image/png;base64,AAAA\"").count(), 1);
        assert_eq!(html.matches("<img ").count(), 1);
        assert_eq!(html.matches(IMAGE_UNAVAILABLE).count(), 2);
        assert!(!html.contains("images/b.png") && !html.contains("example.com"));
        assert!(html.contains(&format!("content=\"{SINGLE_FILE_CSP}\"")));
        // The folder export of the same document is unchanged by the refactor.
        assert!(to_html(&rec).contains("<img src=\"images/a.png\""));
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
    fn first_block_h1_equal_to_title_gives_one_h1() {
        let mut rec = Recording::new("r", "Guide", 0);
        rec.blocks.push(Block::Heading { id: "h".into(), level: 1, text: "Guide".into() });
        rec.blocks.push(Block::Text { id: "t".into(), text: "Intro.".into() });
        assert_eq!(to_html(&rec).matches("<h1>").count(), 1);
        // A different H1 is kept next to the title.
        rec.blocks[0] = Block::Heading { id: "h".into(), level: 1, text: "Other".into() };
        let html = to_html(&rec);
        assert_eq!(html.matches("<h1>").count(), 2);
        assert!(html.contains("<h1>Other</h1>"));
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
