// JSONL rendering of stored raw heartbeats. No I/O, no compute — takes rows in.

pub fn render(raw_rows: &[String]) -> String {
    let mut out = String::new();
    for raw in raw_rows {
        out.push_str(raw);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_emits_one_line_per_heartbeat() {
        let rows = vec![
            r#"{"time":1.0,"entity":"a.py","project":"foo"}"#.to_string(),
            r#"{"time":2.0,"entity":"b.py","project":"foo"}"#.to_string(),
        ];
        let out = render(&rows);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], r#"{"time":1.0,"entity":"a.py","project":"foo"}"#);
        assert_eq!(lines[1], r#"{"time":2.0,"entity":"b.py","project":"foo"}"#);
    }

    #[test]
    fn render_empty_returns_empty_string() {
        assert_eq!(render(&[]), "");
    }
}