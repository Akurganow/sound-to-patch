//! Checks the pull request description against the template in
//! `.github/pull_request_template.md`.
//!
//! The squash merge keeps the description as the commit body, so the
//! authorship disclosure and the verification output stay in history.

/// Fails unless exactly one authorship box is checked and the verification
/// section holds something besides comments.
pub(crate) fn check_body(body: &str) -> crate::Result<()> {
    let authorship = section(body, "Authorship")
        .ok_or("the description has no `## Authorship` section: restore it from the template")?;
    let checked = authorship
        .lines()
        .filter(|line| {
            let line = line.trim_start();
            line.starts_with("- [x]") || line.starts_with("- [X]")
        })
        .count();
    if checked != 1 {
        return Err(format!(
            "`## Authorship` has {checked} checked boxes: check exactly one, \
             the person box or the agent box"
        )
        .into());
    }
    let verification = section(body, "Verification")
        .ok_or("the description has no `## Verification` section: restore it from the template")?;
    if without_comments(verification).trim().is_empty() {
        return Err(
            "`## Verification` is empty: paste the commands you ran and their output".into(),
        );
    }
    Ok(())
}

/// The text under `## {title}` up to the next `## ` heading.
fn section<'a>(body: &'a str, title: &str) -> Option<&'a str> {
    let heading = format!("## {title}");
    let mut offset = 0;
    for line in body.split_inclusive('\n') {
        offset += line.len();
        if line.trim_end() == heading {
            let rest = &body[offset..];
            let end = rest
                .match_indices("## ")
                .find(|&(at, _)| at == 0 || rest.as_bytes()[at - 1] == b'\n')
                .map_or(rest.len(), |(at, _)| at);
            return Some(&rest[..end]);
        }
    }
    None
}

fn without_comments(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        rest = rest[start..]
            .find("-->")
            .map_or("", |end| &rest[start + end + 3..]);
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEMPLATE: &str = include_str!("../../.github/pull_request_template.md");

    fn filled(person: &str, agent: &str, verification: &str) -> String {
        TEMPLATE
            .replace("- [ ] A person", &format!("- [{person}] A person"))
            .replace("- [ ] An agent", &format!("- [{agent}] An agent"))
            .replace(
                "<!-- verification -->",
                &format!("<!-- verification -->\n{verification}"),
            )
    }

    #[test]
    fn accepts_a_filled_template() {
        check_body(&filled(" ", "x", "```\n$ cargo nextest run\nPASS\n```")).unwrap();
    }

    #[test]
    fn accepts_crlf_line_endings() {
        let body = filled("x", " ", "ran it").replace('\n', "\r\n");
        check_body(&body).unwrap();
    }

    #[test]
    fn rejects_the_unfilled_template() {
        let err = check_body(TEMPLATE).unwrap_err().to_string();
        assert!(err.contains("0 checked boxes"), "{err}");
    }

    #[test]
    fn rejects_both_boxes_checked() {
        let err = check_body(&filled("x", "x", "ran it"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("2 checked boxes"), "{err}");
    }

    #[test]
    fn rejects_an_empty_verification() {
        let err = check_body(&filled("x", " ", "")).unwrap_err().to_string();
        assert!(err.contains("Verification"), "{err}");
    }

    #[test]
    fn rejects_a_missing_section() {
        let err = check_body("## Summary\n\nText.\n").unwrap_err().to_string();
        assert!(err.contains("Authorship"), "{err}");
    }
}
