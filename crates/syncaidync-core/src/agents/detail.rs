//! Extract a one-line tool detail from Claude-style tool_input JSON.

use serde_json::Value;

/// One line, at most 160 characters.
pub fn tool_detail(tool_name: &str, input: &Value, cwd: Option<&str>) -> String {
    let raw = match tool_name {
        "Bash" => str_field(input, "command"),
        "Read" | "Edit" | "Write" => {
            let path = str_field(input, "file_path");
            relativize(&path, cwd)
        }
        "Grep" | "Glob" => str_field(input, "pattern"),
        "WebFetch" => str_field(input, "url"),
        "WebSearch" => str_field(input, "query"),
        "Task" => str_field(input, "description"),
        _ => first_string_value(input).unwrap_or_default(),
    };
    cut_one_line(&raw, 160)
}

fn str_field(input: &Value, key: &str) -> String {
    input
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn first_string_value(input: &Value) -> Option<String> {
    match input {
        Value::Object(map) => {
            for v in map.values() {
                if let Some(s) = v.as_str() {
                    return Some(s.to_string());
                }
            }
            None
        }
        Value::String(s) => Some(s.clone()),
        _ => None,
    }
}

fn relativize(path: &str, cwd: Option<&str>) -> String {
    let Some(cwd) = cwd.filter(|c| !c.is_empty()) else {
        return path.to_string();
    };
    let cwd = cwd.trim_end_matches('/');
    if let Some(rest) = path.strip_prefix(cwd) {
        let rest = rest.trim_start_matches('/');
        if rest.is_empty() {
            path.to_string()
        } else {
            rest.to_string()
        }
    } else {
        path.to_string()
    }
}

fn cut_one_line(s: &str, max: usize) -> String {
    let one = s.lines().next().unwrap_or(s).trim();
    if one.chars().count() <= max {
        return one.to_string();
    }
    let truncated: String = one.chars().take(max.saturating_sub(1)).collect();
    format!("{truncated}…")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_known_tools_and_cuts() {
        assert_eq!(
            tool_detail("Bash", &json!({"command": "ls -la"}), None),
            "ls -la"
        );
        assert_eq!(
            tool_detail(
                "Read",
                &json!({"file_path": "/tmp/ws/src/main.rs"}),
                Some("/tmp/ws")
            ),
            "src/main.rs"
        );
        assert_eq!(tool_detail("Grep", &json!({"pattern": "foo"}), None), "foo");
        let long = "x".repeat(200);
        let d = tool_detail("Bash", &json!({"command": long}), None);
        assert!(d.chars().count() <= 160);
        assert!(d.ends_with('…'));
    }
}
