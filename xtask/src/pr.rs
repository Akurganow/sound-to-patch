//! Checks the pull request description against the template in
//! `.github/pull_request_template.md`.
//!
//! The squash merge keeps the description as the commit body, so the
//! authorship disclosure and the verification output stay in history.

/// Markers that make GitHub skip every workflow of the push that carries
/// them, matched without case. The squash commit is the title and the
/// description, so one of them in either stops the release run on `main`.
const SKIP_MARKERS: [&str; 7] = [
    "[skip ci]",
    "[ci skip]",
    "[no ci]",
    "[skip actions]",
    "[actions skip]",
    "skip-checks:true",
    "skip-checks: true",
];

fn skip_marker(text: &str) -> Option<&'static str> {
    let lower = text.to_lowercase();
    SKIP_MARKERS
        .into_iter()
        .find(|marker| lower.contains(marker))
}

/// Fails on a workflow skip marker in the title.
pub(crate) fn check_title(title: &str) -> crate::Result<()> {
    match skip_marker(title) {
        Some(marker) => Err(format!(
            "the title contains `{marker}`: the squash commit would skip \
             the workflows on main, so reword it"
        )
        .into()),
        None => Ok(()),
    }
}

/// Runs every check of the title and the description, in the order the
/// author meets them: the description, the title, then the two together.
pub(crate) fn check(title: &str, body: &str) -> crate::Result<()> {
    check_body(body)?;
    check_title(title)?;
    check_breaking(title, body)
}

/// The first line of `body` that cog 7.0.0 reads as a breaking-change
/// footer: spaces, then `BREAKING CHANGE` or `BREAKING-CHANGE`, then `:` or
/// ` #`. A value on the same line follows a space. With no value there, cog
/// takes the next lines as the value, so the line is a footer when any
/// later line holds text. A tab before the token or another separator is
/// plain text.
fn breaking_footer(body: &str) -> Option<&str> {
    let lines: Vec<&str> = body.lines().collect();
    lines.iter().enumerate().find_map(|(at, line)| {
        let value = footer_value(line)?;
        let later = || lines[at + 1..].iter().any(|line| !line.trim().is_empty());
        (!value.trim().is_empty() || later()).then_some(*line)
    })
}

/// The text after the separator of a footer line, if `line` opens one.
fn footer_value(line: &str) -> Option<&str> {
    let line = line.trim_start_matches(' ');
    ["BREAKING CHANGE", "BREAKING-CHANGE"]
        .iter()
        .find_map(|token| {
            let rest = line.strip_prefix(token)?;
            if rest == ":" {
                Some("")
            } else {
                rest.strip_prefix(": ").or_else(|| rest.strip_prefix(" #"))
            }
        })
}

/// Fails on a breaking-change footer line in the description unless the
/// title marks the change as breaking with `!`. From 1.0 on, cog bumps the
/// major version for such a line in any commit type, so a pasted line would
/// make a major release that the title does not show.
pub(crate) fn check_breaking(title: &str, body: &str) -> crate::Result<()> {
    let marked = title
        .split_once(':')
        .is_some_and(|(prefix, _)| prefix.trim_end().ends_with('!'));
    let footer = breaking_footer(body);
    match footer {
        Some(line) if !marked => Err(format!(
            "the description has the line `{line}`, which cog reads as a breaking change, \
             but the title has no `!`: add `!` before the colon of the title, or reword the line"
        )
        .into()),
        _ => Ok(()),
    }
}

/// Fails on a workflow skip marker, on any count of checked authorship boxes
/// but one, and on a verification section that holds only comments.
pub(crate) fn check_body(body: &str) -> crate::Result<()> {
    if let Some(marker) = skip_marker(body) {
        return Err(format!(
            "the description contains `{marker}`: the squash commit would skip \
             the workflows on main, so reword it"
        )
        .into());
    }
    let authorship = section(body, "Authorship")
        .ok_or("the description has no `## Authorship` section: restore it from the template")?;
    let checked = without_comments(authorship)
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

/// The template the description fills in.
const TEMPLATE: &str = include_str!("../../.github/pull_request_template.md");

/// Whether a line is one of the `## ` headings of the template.
fn template_heading(line: &str) -> bool {
    let line = line.trim_end();
    line.starts_with("## ") && TEMPLATE.lines().any(|heading| heading.trim_end() == line)
}

/// The text under `## {title}` up to the next heading of the template. Pasted
/// output may hold any other `## ` line, such as the first line of
/// `git status -sb`, and a line inside a fenced block never ends the section.
fn section<'a>(body: &'a str, title: &str) -> Option<&'a str> {
    let heading = format!("## {title}");
    let mut lines = body.split_inclusive('\n');
    let mut start = 0;
    loop {
        let line = lines.next()?;
        start += line.len();
        if line.trim_end() == heading {
            break;
        }
    }
    let mut end = start;
    let mut fence: Option<&str> = None;
    for line in lines {
        let trimmed = line.trim_start();
        if let Some(open) = fence {
            if trimmed.trim_end().starts_with(open) {
                fence = None;
            }
        } else if trimmed.starts_with("```") {
            fence = Some("```");
        } else if trimmed.starts_with("~~~") {
            fence = Some("~~~");
        } else if template_heading(line) {
            break;
        }
        end += line.len();
    }
    Some(&body[start..end])
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
    fn a_heading_marker_inside_a_line_does_not_end_the_section() {
        let body = "## Authorship\n\n- [ ] A person, see ## below\n- [x] An agent\n\n\
            ## Verification\n\nran it\n";
        check_body(body).unwrap();
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
    fn rejects_a_skip_marker_in_any_case() {
        let err = check_body(&filled(" ", "x", "no [Skip CI] here"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("[skip ci]"), "{err}");
    }

    #[test]
    fn accepts_a_false_skip_checks_trailer() {
        check_body(&filled(" ", "x", "skip-checks: false")).unwrap();
    }

    #[test]
    fn rejects_a_skip_marker_in_the_title() {
        let err = check_title("fix: x [ci skip]").unwrap_err().to_string();
        assert!(err.contains("title"), "{err}");
        check_title("fix: skip the ci step").unwrap();
    }

    #[test]
    fn a_pasted_heading_line_does_not_end_the_verification() {
        let body = filled(" ", "x", "## main...origin/main\n M src/lib.rs");
        check_body(&body).unwrap();
    }

    #[test]
    fn a_heading_inside_a_fence_does_not_end_the_section() {
        let body = "## Authorship\n\n- [x] A person\n\n## Verification\n\n\
            ```\n## Checklist\n```\n\n## Checklist\n";
        assert_eq!(
            section(body, "Verification"),
            Some("\n```\n## Checklist\n```\n\n")
        );
    }

    #[test]
    fn a_checked_box_inside_a_comment_does_not_count() {
        let body = filled(" ", "x", "ran it").replace(
            "<!-- Check exactly one box. -->",
            "<!-- Check exactly one box, as in:\n- [x] A person wrote this change.\n-->",
        );
        check_body(&body).unwrap();
    }

    #[test]
    fn a_breaking_change_line_needs_a_marked_title() {
        let body = filled(" ", "x", "ran it\nBREAKING CHANGE: the flag is gone");
        let err = check_breaking("docs: reword", &body)
            .unwrap_err()
            .to_string();
        assert!(err.contains("BREAKING CHANGE"), "{err}");
        check_breaking("feat(notes)!: drop the flag", &body).unwrap();
        check_breaking("feat!: drop the flag", &body).unwrap();
        check_breaking("docs: reword", "no breaking change here").unwrap();
    }

    // Each case was run through `cog bump --auto --dry-run` 7.0.0 as the
    // body of a `docs:` commit on a `v1.2.3` tag, kept byte for byte with
    // `--cleanup=verbatim`: last in the body, and followed by more lines.
    #[test]
    fn a_footer_is_what_cog_reads_as_one() {
        let ends = ["", "\n", "\n   \n"];
        let follows = [
            "\nmore text",
            "\n\nmore text",
            "\n- item",
            "\n## Verification",
        ];
        let footer = |line: &str, tail: &str| {
            breaking_footer(&format!("text\n{line}{tail}")).map(ToOwned::to_owned)
        };
        // A value on the line: a footer wherever it stands.
        for line in [
            "BREAKING CHANGE: y",
            "BREAKING-CHANGE: y",
            "BREAKING CHANGE #y",
            "BREAKING-CHANGE #y",
            "  BREAKING CHANGE: y",
            "        BREAKING-CHANGE: y",
            "BREAKING CHANGE:  y",
        ] {
            for tail in ends.iter().chain(&follows) {
                assert_eq!(
                    footer(line, tail).as_deref(),
                    Some(line),
                    "{line:?} then {tail:?}"
                );
            }
        }
        // No value on the line: cog reads the next text as the value.
        for line in [
            "BREAKING CHANGE:",
            "BREAKING CHANGE: ",
            "BREAKING CHANGE:  ",
            "BREAKING CHANGE #",
            "BREAKING-CHANGE:",
            "BREAKING-CHANGE #",
            "  BREAKING CHANGE:",
        ] {
            for tail in ends {
                assert_eq!(footer(line, tail), None, "{line:?} then {tail:?}");
            }
            for tail in follows {
                assert_eq!(
                    footer(line, tail).as_deref(),
                    Some(line),
                    "{line:?} then {tail:?}"
                );
                let body = format!("text\n{line}{tail}");
                assert!(
                    check_breaking("docs: x", &body).is_err(),
                    "{line:?} then {tail:?}"
                );
            }
        }
        // Plain text wherever it stands.
        for line in [
            "BREAKING CHANGE",
            "BREAKING CHANGE no colon",
            "BREAKING CHANGES are none",
            "BREAKING CHANGES: y",
            "BREAKING CHANGES:",
            "BREAKING CHANGE:y",
            "BREAKING CHANGE : y",
            "BREAKING CHANGE :",
            "BREAKING CHANGE#y",
            "BREAKING CHANGE#",
            "\tBREAKING CHANGE: y",
            "\tBREAKING CHANGE:",
            "breaking change: y",
            "- BREAKING CHANGE: y",
            "text BREAKING CHANGE: y",
            "`BREAKING CHANGE: y`",
        ] {
            for tail in ends.iter().chain(&follows) {
                assert_eq!(footer(line, tail), None, "{line:?} then {tail:?}");
                check_breaking("docs: x", &format!("text\n{line}{tail}")).unwrap();
            }
        }
        check_breaking("docs: x", TEMPLATE).unwrap();
    }

    #[test]
    fn rejects_a_missing_section() {
        let err = check_body("## Summary\n\nText.\n").unwrap_err().to_string();
        assert!(err.contains("Authorship"), "{err}");
    }
}
