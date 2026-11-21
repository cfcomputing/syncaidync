//! Pull a JSON object out of model text.

use serde_json::Value;

/// Try the whole text; else the first fenced block; else `{` … matching `}`.
pub fn extract_json(text: &str) -> Option<Value> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        if v.is_object() {
            return Some(v);
        }
    }
    if let Some(inner) = first_fence(trimmed) {
        if let Ok(v) = serde_json::from_str::<Value>(inner.trim()) {
            if v.is_object() {
                return Some(v);
            }
        }
    }
    if let Some(span) = brace_span(trimmed) {
        if let Ok(v) = serde_json::from_str::<Value>(span) {
            if v.is_object() {
                return Some(v);
            }
        }
    }
    None
}

fn first_fence(text: &str) -> Option<&str> {
    let start = text.find("```")?;
    let after = &text[start + 3..];
    // optional language tag
    let body = if let Some(nl) = after.find('\n') {
        &after[nl + 1..]
    } else {
        after
    };
    let end = body.find("```")?;
    Some(&body[..end])
}

fn brace_span(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    let start = bytes.iter().position(|&b| b == b'{')?;
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escape = false;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_str {
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[start..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_fenced_prose_nested_garbage() {
        let bare = r#"{"summary":"ok","checkpoints":[]}"#;
        assert_eq!(
            extract_json(bare).unwrap()["summary"],
            serde_json::json!("ok")
        );

        let fenced = "Here:\n```json\n{\"summary\":\"fenced\"}\n```\n";
        assert_eq!(
            extract_json(fenced).unwrap()["summary"],
            serde_json::json!("fenced")
        );

        let prose = "Sure.\n{\"summary\":\"mid\", \"n\": {\"a\": 1}}\nThanks.";
        assert_eq!(
            extract_json(prose).unwrap()["summary"],
            serde_json::json!("mid")
        );

        let nested = r#"prefix {"summary":"s","x":"say \"hi\" {ok}"} tail"#;
        assert_eq!(
            extract_json(nested).unwrap()["summary"],
            serde_json::json!("s")
        );

        assert!(extract_json("nope").is_none());
        assert!(extract_json("{not json").is_none());
    }
}
