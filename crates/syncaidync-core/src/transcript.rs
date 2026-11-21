//! Condense agent transcripts for takeaway prompts.

use crate::startup::strip_ansi;

pub const DEFAULT_BUDGET: usize = 60_000;
const HEAD_KEEP: usize = 10_000;
const TAIL_KEEP: usize = 50_000;

/// Condense a Claude Code JSONL transcript.
pub fn condense_claude(jsonl_text: &str, budget: usize) -> String {
    let mut lines = Vec::new();
    for raw in jsonl_text.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) else {
            continue;
        };
        let ty = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
        match ty {
            "user" => {
                if let Some(text) = user_text(&v) {
                    if !text.is_empty() && !text.starts_with('<') {
                        lines.push(format!("You: {text}"));
                    }
                }
            }
            "assistant" => {
                for part in assistant_parts(&v) {
                    lines.push(part);
                }
            }
            _ => {}
        }
    }
    apply_budget(&lines.join("\n"), budget)
}

/// Condense a raw terminal log (ANSI stripped, empties and repeats dropped).
pub fn condense_terminal(log_text: &str, budget: usize) -> String {
    let stripped = strip_ansi(log_text);
    let mut out = Vec::new();
    let mut prev = String::new();
    for line in stripped.lines() {
        let t = line.trim_end();
        if t.is_empty() {
            continue;
        }
        if t == prev {
            continue;
        }
        out.push(t.to_string());
        prev = t.to_string();
    }
    apply_budget(&out.join("\n"), budget)
}

fn user_text(v: &serde_json::Value) -> Option<String> {
    let content = v.pointer("/message/content").or_else(|| v.get("content"))?;
    extract_text(content)
}

fn assistant_parts(v: &serde_json::Value) -> Vec<String> {
    let mut parts = Vec::new();
    let content = v.pointer("/message/content").or_else(|| v.get("content"));
    let Some(content) = content else {
        return parts;
    };
    if let Some(s) = content.as_str() {
        let t = s.trim();
        if !t.is_empty() {
            parts.push(format!("Agent: {t}"));
        }
        return parts;
    }
    let Some(arr) = content.as_array() else {
        return parts;
    };
    for block in arr {
        let bty = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
        match bty {
            "text" => {
                if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                    let t = t.trim();
                    if !t.is_empty() {
                        parts.push(format!("Agent: {t}"));
                    }
                }
            }
            "tool_use" => {
                let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("tool");
                let detail = tool_detail(block);
                if detail.is_empty() {
                    parts.push(format!("Tool: {name}"));
                } else {
                    parts.push(format!("Tool: {name}: {detail}"));
                }
            }
            _ => {}
        }
    }
    parts
}

fn tool_detail(block: &serde_json::Value) -> String {
    let input = block.get("input");
    if let Some(cmd) = input
        .and_then(|i| i.get("command"))
        .and_then(|c| c.as_str())
    {
        return truncate(cmd.trim(), 120);
    }
    if let Some(path) = input
        .and_then(|i| i.get("file_path"))
        .and_then(|c| c.as_str())
    {
        return truncate(path.trim(), 120);
    }
    if let Some(path) = input.and_then(|i| i.get("path")).and_then(|c| c.as_str()) {
        return truncate(path.trim(), 120);
    }
    if let Some(q) = input
        .and_then(|i| i.get("pattern"))
        .and_then(|c| c.as_str())
    {
        return truncate(q.trim(), 120);
    }
    String::new()
}

fn extract_text(content: &serde_json::Value) -> Option<String> {
    if let Some(s) = content.as_str() {
        return Some(s.trim().to_string());
    }
    let arr = content.as_array()?;
    for block in arr {
        let bty = block.get("type").and_then(|t| t.as_str()).unwrap_or("text");
        if bty == "text" || block.get("text").is_some() {
            if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                return Some(t.trim().to_string());
            }
        }
        // Skip tool_result blocks
        if bty == "tool_result" {
            continue;
        }
    }
    None
}

fn truncate(s: &str, max: usize) -> String {
    let mut out: String = s.chars().take(max).collect();
    if s.chars().count() > max {
        out.push('…');
    }
    out
}

fn apply_budget(text: &str, budget: usize) -> String {
    if text.chars().count() <= budget {
        return ensure_newline(text);
    }
    // Default split: first 10k + last 50k. Scale down when the budget is smaller.
    let marker_room = 64usize;
    let head_cap = HEAD_KEEP.min(budget / 6).max(1);
    let tail_cap = TAIL_KEEP
        .min(budget.saturating_sub(head_cap + marker_room))
        .max(1);
    let head = take_prefix_lines(text, head_cap);
    let tail = take_suffix_lines(text, tail_cap);
    let omitted = text
        .chars()
        .count()
        .saturating_sub(head.chars().count() + tail.chars().count());
    let mut out = String::new();
    out.push_str(&head);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&format!("… {omitted} characters left out …\n"));
    out.push_str(&tail);
    // Final hard trim on line boundaries if still over.
    if out.chars().count() > budget {
        out = take_prefix_lines(&out, budget);
    }
    ensure_newline(&out)
}

fn take_prefix_lines(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for line in text.lines() {
        let next_len = out.chars().count() + line.chars().count() + 1;
        if next_len > max_chars && !out.is_empty() {
            break;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(line);
        if out.chars().count() >= max_chars {
            break;
        }
    }
    out
}

fn take_suffix_lines(text: &str, max_chars: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut chosen = Vec::new();
    let mut len = 0usize;
    for line in lines.iter().rev() {
        let add = line.chars().count() + 1;
        if len + add > max_chars && !chosen.is_empty() {
            break;
        }
        chosen.push(*line);
        len += add;
        if len >= max_chars {
            break;
        }
    }
    chosen.reverse();
    chosen.join("\n")
}

fn ensure_newline(s: &str) -> String {
    let mut t = s.to_string();
    if !t.is_empty() && !t.ends_with('\n') {
        t.push('\n');
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn condense_claude_keeps_user_agent_tools() {
        let jsonl = r#"{"type":"system","content":"ignore"}
{"type":"user","message":{"content":"run the tests"}}
{"type":"assistant","message":{"content":[{"type":"text","text":"Sure."},{"type":"tool_use","name":"Bash","input":{"command":"cargo test"}}]}}
{"type":"user","message":{"content":[{"type":"tool_result","content":"ok"}]}}
"#;
        let out = condense_claude(jsonl, DEFAULT_BUDGET);
        assert!(out.contains("You: run the tests"));
        assert!(out.contains("Agent: Sure."));
        assert!(out.contains("Tool: Bash: cargo test"));
        assert!(!out.contains("tool_result"));
    }

    #[test]
    fn budget_cuts_middle() {
        let mut lines = Vec::new();
        for i in 0..2000 {
            lines.push(format!("line-{i:04} {}", "x".repeat(40)));
        }
        let text = lines.join("\n");
        let out = condense_terminal(&text, 5_000);
        assert!(out.contains("characters left out"));
        assert!(out.chars().count() <= 5_000 + 80); // marker line slack
        assert!(out.contains("line-0000"));
    }
}
