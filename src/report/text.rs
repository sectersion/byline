// text rendering of derived hours. No I/O, no compute — takes rows in.

pub fn render(rows: &[(String, String, f64)]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    let mut last_day = String::new();
    for (day, project, minutes) in rows {
        if day != &last_day {
            if !last_day.is_empty() {
                out.push('\n');
            }
            out.push_str(day);
            out.push('\n');
            last_day = day.clone();
        }
        out.push_str(&format!("  {:<20} {:>6.1} h\n", project, minutes / 60.0));
    }
    out
}
