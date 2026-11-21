//! Markdown → safe HTML (no raw HTML, safe link destinations only).

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag};

/// Optional context for rewriting relative image URLs to a session/pull or margin media route.
pub struct RenderContext<'a> {
    /// API owner prefix, e.g. `sessions/s1` or `pulls/p1`.
    pub owner: &'a str,
    pub base_path: &'a str,
    /// When set, `media/<name>` images rewrite to the margin media route.
    pub margin_id: Option<&'a str>,
}

fn md_options() -> Options {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options
}

/// Render markdown to HTML safe for insertion into the page.
pub fn to_safe_html(md: &str) -> String {
    to_safe_html_with_context(md, None)
}

/// Like [`to_safe_html`], optionally rewriting relative image destinations.
pub fn to_safe_html_with_context(md: &str, ctx: Option<&RenderContext<'_>>) -> String {
    render(md, ctx, false)
}

/// Like [`to_safe_html_with_context`], tagging block elements with `data-source-line`.
pub fn to_safe_html_with_source_lines(md: &str, ctx: Option<&RenderContext<'_>>) -> String {
    render(md, ctx, true)
}

fn render(md: &str, ctx: Option<&RenderContext<'_>>, source_lines: bool) -> String {
    let parser = Parser::new_ext(md, md_options()).filter_map(|ev| match ev {
        Event::Html(_) | Event::InlineHtml(_) => None,
        Event::Start(Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        }) => {
            let dest = match ctx {
                Some(c) if c.margin_id.is_none() && !c.base_path.is_empty() => {
                    sanitize_link_url(&dest_url, c)
                }
                _ => sanitize_url(&dest_url),
            };
            Some(Event::Start(Tag::Link {
                link_type,
                dest_url: dest.into(),
                title,
                id,
            }))
        }
        Event::Start(Tag::Image {
            link_type,
            dest_url,
            title,
            id,
        }) => {
            let dest = match ctx {
                Some(c) => sanitize_image_url(&dest_url, c),
                None => sanitize_url(&dest_url),
            };
            Some(Event::Start(Tag::Image {
                link_type,
                dest_url: dest.into(),
                title,
                id,
            }))
        }
        other => Some(other),
    });

    let mut out = String::new();
    pulldown_cmark::html::push_html(&mut out, parser);
    if source_lines {
        inject_source_lines(&out, &block_markers(md))
    } else {
        out
    }
}

/// Opening-tag prefixes in the same order pulldown emits them for tracked blocks.
fn block_markers(md: &str) -> Vec<(&'static str, u32)> {
    let mut out = Vec::new();
    for (ev, range) in Parser::new_ext(md, md_options()).into_offset_iter() {
        let needle = match &ev {
            Event::Start(tag) => block_open_needle(tag),
            Event::Rule => Some("<hr"),
            _ => None,
        };
        let Some(needle) = needle else { continue };
        let line = byte_line_number(md, range.start);
        out.push((needle, line));
    }
    out
}

fn block_open_needle(tag: &Tag<'_>) -> Option<&'static str> {
    match tag {
        Tag::Paragraph => Some("<p"),
        Tag::Heading { level, .. } => Some(match level {
            HeadingLevel::H1 => "<h1",
            HeadingLevel::H2 => "<h2",
            HeadingLevel::H3 => "<h3",
            HeadingLevel::H4 => "<h4",
            HeadingLevel::H5 => "<h5",
            HeadingLevel::H6 => "<h6",
        }),
        Tag::CodeBlock(_) => Some("<pre"),
        Tag::BlockQuote(_) => Some("<blockquote"),
        Tag::Table(_) => Some("<table"),
        Tag::Item => Some("<li"),
        _ => None,
    }
}

fn byte_line_number(md: &str, byte: usize) -> u32 {
    let end = byte.min(md.len());
    md.as_bytes()[..end].iter().filter(|&&b| b == b'\n').count() as u32 + 1
}

fn inject_source_lines(html: &str, blocks: &[(&str, u32)]) -> String {
    let mut out = String::with_capacity(html.len() + blocks.len() * 32);
    let mut rest = html;
    for &(needle, line) in blocks {
        let Some(i) = rest.find(needle) else {
            break;
        };
        out.push_str(&rest[..i]);
        out.push_str(needle);
        let after = &rest[i + needle.len()..];
        if !after.starts_with(" data-source-line=") {
            out.push_str(" data-source-line=\"");
            out.push_str(&line.to_string());
            out.push('"');
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

fn sanitize_url(url: &str) -> String {
    let t = url.trim();
    let lower = t.to_ascii_lowercase();
    if lower.starts_with("https://")
        || lower.starts_with("http://")
        || lower.starts_with("mailto:")
        || lower.starts_with('#')
    {
        t.to_string()
    } else {
        "#".into()
    }
}

/// Relative workspace links become `#dync-open:<path>` for the Files View to intercept.
fn sanitize_link_url(url: &str, ctx: &RenderContext<'_>) -> String {
    let t = url.trim();
    let lower = t.to_ascii_lowercase();
    if lower.starts_with("https://")
        || lower.starts_with("http://")
        || lower.starts_with("mailto:")
        || lower.starts_with('#')
    {
        return t.to_string();
    }
    if looks_like_scheme(t) {
        return "#".into();
    }
    let path_part = t.split(['?', '#']).next().unwrap_or("");
    let Some(decoded) = percent_decode(path_part) else {
        return "#".into();
    };
    match resolve_relative(ctx.base_path, &decoded) {
        Some(rel) => format!("#dync-open:{}", percent_encode_path(&rel)),
        None => "#".into(),
    }
}

fn sanitize_image_url(url: &str, ctx: &RenderContext<'_>) -> String {
    let t = url.trim();
    let lower = t.to_ascii_lowercase();
    if lower.starts_with("https://")
        || lower.starts_with("http://")
        || lower.starts_with("mailto:")
        || lower.starts_with('#')
    {
        return t.to_string();
    }
    if looks_like_scheme(t) {
        return "#".into();
    }
    // `img.png?raw=true` and `img.png#x` name the file `img.png`.
    let path_part = t.split(['?', '#']).next().unwrap_or("");
    // Authors write `my%20diagram.png`; decode so we do not encode it a second time.
    let Some(decoded) = percent_decode(path_part) else {
        return "#".into();
    };
    if let Some(mid) = ctx.margin_id {
        if let Some(name) = margin_media_name(&decoded) {
            return format!(
                "/api/v1/{}/margins/{}/media/{}",
                ctx.owner,
                mid,
                percent_encode_path(name)
            );
        }
        return "#".into();
    }
    if ctx.base_path.is_empty() {
        return "#".into();
    }
    match resolve_relative(ctx.base_path, &decoded) {
        Some(rel) => format!(
            "/api/v1/{}/media?path={}",
            ctx.owner,
            percent_encode_path(&rel)
        ),
        None => "#".into(),
    }
}

fn margin_media_name(url: &str) -> Option<&str> {
    let t = url.trim();
    let rest = t
        .strip_prefix("media/")
        .or_else(|| t.strip_prefix("./media/"))?;
    if rest.is_empty() || rest.contains('/') || rest.contains("..") {
        return None;
    }
    Some(rest)
}

fn looks_like_scheme(url: &str) -> bool {
    let bytes = url.as_bytes();
    if bytes.is_empty() || !bytes[0].is_ascii_alphabetic() {
        return false;
    }
    for (i, b) in bytes.iter().enumerate() {
        if *b == b':' {
            return i > 0;
        }
        if !(b.is_ascii_alphanumeric() || *b == b'+' || *b == b'-' || *b == b'.') {
            return false;
        }
    }
    false
}

/// Resolve `href` against the directory of `base_path`. Returns a workspace-relative path
/// with `/` separators, or `None` if the result would leave the workspace root or is invalid.
pub fn resolve_relative(base_path: &str, href: &str) -> Option<String> {
    let href = href.trim();
    if href.is_empty() || href.contains('\0') {
        return None;
    }
    if href.starts_with('/') {
        return None;
    }
    let base_dir = parent_dir(base_path);
    let mut parts: Vec<&str> = Vec::new();
    if !base_dir.is_empty() {
        for seg in base_dir.split('/') {
            if !seg.is_empty() {
                parts.push(seg);
            }
        }
    }
    for seg in href.split('/') {
        if seg.is_empty() || seg == "." {
            continue;
        }
        if seg == ".." {
            if parts.is_empty() {
                return None;
            }
            parts.pop();
            continue;
        }
        if seg == ".git" || seg.contains('\0') {
            return None;
        }
        parts.push(seg);
    }
    if parts.is_empty() {
        return None;
    }
    Some(parts.join("/"))
}

fn parent_dir(base_path: &str) -> &str {
    let base = base_path.trim_matches('/');
    match base.rfind('/') {
        Some(i) => &base[..i],
        None => "",
    }
}

/// Decode `%XX` escapes. Returns `None` for a bad escape or a result that is not UTF-8.
fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = bytes.get(i + 1..i + 3)?;
            let hi = (hex[0] as char).to_digit(16)?;
            let lo = (hex[1] as char).to_digit(16)?;
            out.push((hi * 16 + lo) as u8);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn percent_encode_path(rel: &str) -> String {
    let mut enc = String::with_capacity(rel.len());
    for &b in rel.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                enc.push(b as char);
            }
            _ => {
                const HEX: &[u8; 16] = b"0123456789ABCDEF";
                enc.push('%');
                enc.push(HEX[(b >> 4) as usize] as char);
                enc.push(HEX[(b & 0xf) as usize] as char);
            }
        }
    }
    enc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_raw_html_and_bad_links() {
        let html = to_safe_html(
            "Hello <script>alert(1)</script>\n\n[x](javascript:alert(1)) [y](data:text/html,hi) [ok](https://example.com)\n\n![a](https://img.example/a.png) ![b](javascript:x)\n\n| A | B |\n| - | - |\n| 1 | 2 |\n\n- [x] done\n- [ ] todo\n",
        );
        assert!(!html.contains("<script>"));
        assert!(html.contains("href=\"#\""));
        assert!(html.contains("href=\"https://example.com\""));
        assert!(html.contains("src=\"https://img.example/a.png\""));
        assert!(html.contains("<table>"));
        assert!(html.contains("type=\"checkbox\""));
    }

    #[test]
    fn end_tags_still_emit() {
        let html = to_safe_html("**bold**");
        assert!(html.contains("<strong>bold</strong>"));
    }

    #[test]
    fn resolve_relative_joins_and_blocks_escape() {
        assert_eq!(
            resolve_relative("docs/guide/foo.md", "./a.png").as_deref(),
            Some("docs/guide/a.png")
        );
        assert_eq!(
            resolve_relative("docs/guide/foo.md", "../img/a.png").as_deref(),
            Some("docs/img/a.png")
        );
        assert_eq!(resolve_relative("docs/foo.md", "../../etc/passwd"), None);
        assert_eq!(resolve_relative("foo.md", "/abs.png"), None);
        assert_eq!(
            resolve_relative("README.md", "pics/diagram.png").as_deref(),
            Some("pics/diagram.png")
        );
    }

    #[test]
    fn relative_images_rewrite_to_media() {
        let ctx = RenderContext {
            owner: "sessions/s1",
            base_path: "docs/readme.md",
            margin_id: None,
        };
        let html = to_safe_html_with_context("![d](./img/a.png)", Some(&ctx));
        assert!(
            html.contains("src=\"/api/v1/sessions/s1/media?path=docs/img/a.png\""),
            "{html}"
        );
        let bad = to_safe_html_with_context("![x](javascript:alert(1))", Some(&ctx));
        assert!(bad.contains("src=\"#\""), "{bad}");
        let abs = to_safe_html_with_context("![x](https://img.example/a.png)", Some(&ctx));
        assert!(abs.contains("src=\"https://img.example/a.png\""), "{abs}");
    }

    #[test]
    fn relative_links_open_in_files() {
        let ctx = RenderContext {
            owner: "sessions/s1",
            base_path: "docs/readme.md",
            margin_id: None,
        };
        let html = to_safe_html_with_context("[x](./other.md)", Some(&ctx));
        assert!(html.contains("href=\"#dync-open:docs/other.md\""), "{html}");
        let bad = to_safe_html_with_context("[x](javascript:alert(1))", Some(&ctx));
        assert!(bad.contains("href=\"#\""), "{bad}");
    }

    #[test]
    fn margin_media_images_rewrite() {
        let ctx = RenderContext {
            owner: "sessions/s1",
            base_path: "",
            margin_id: Some("m1"),
        };
        let html = to_safe_html_with_context("![d](media/a.png)", Some(&ctx));
        assert!(
            html.contains("src=\"/api/v1/sessions/s1/margins/m1/media/a.png\""),
            "{html}"
        );
        let pull = RenderContext {
            owner: "pulls/p1",
            base_path: "",
            margin_id: Some("m1"),
        };
        let pull_html = to_safe_html_with_context("![d](media/a.png)", Some(&pull));
        assert!(
            pull_html.contains("src=\"/api/v1/pulls/p1/margins/m1/media/a.png\""),
            "{pull_html}"
        );
    }

    #[test]
    fn image_query_fragment_and_escapes() {
        let ctx = RenderContext {
            owner: "sessions/s1",
            base_path: "docs/readme.md",
            margin_id: None,
        };
        let q = to_safe_html_with_context("![x](img/shot.png?raw=true)", Some(&ctx));
        assert!(q.contains("media?path=docs/img/shot.png\""), "{q}");
        let f = to_safe_html_with_context("![x](img/shot.png#dark)", Some(&ctx));
        assert!(f.contains("media?path=docs/img/shot.png\""), "{f}");
        let enc = to_safe_html_with_context("![x](my%20diagram.png)", Some(&ctx));
        assert!(enc.contains("media?path=docs/my%20diagram.png\""), "{enc}");
        // Encoded `..` is still caught after decoding.
        let esc = to_safe_html_with_context("![x](%2E%2E/%2E%2E/etc/x.png)", Some(&ctx));
        assert!(esc.contains("src=\"#\""), "{esc}");
        let bad = to_safe_html_with_context("![x](a%zz.png)", Some(&ctx));
        assert!(bad.contains("src=\"#\""), "{bad}");
    }

    #[test]
    fn percent_encode_spaces() {
        assert_eq!(percent_encode_path("a b.png"), "a%20b.png");
    }

    #[test]
    fn source_lines_tag_blocks() {
        let md = "# Title\n\nHello\n\n- one\n- two\n";
        let html = to_safe_html_with_source_lines(md, None);
        assert!(html.contains("<h1 data-source-line=\"1\">"), "{html}");
        assert!(html.contains("<p data-source-line=\"3\">"), "{html}");
        assert!(html.contains("<li data-source-line=\"5\">"), "{html}");
        assert!(html.contains("<li data-source-line=\"6\">"), "{html}");
        let plain = to_safe_html(md);
        assert!(!plain.contains("data-source-line"), "{plain}");
    }
}
