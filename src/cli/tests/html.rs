//! Tests for `--format html`: one page on stdout, the records of
//! `records`, or the table of `sessions` or `stats`.

use super::{History, WEEK};
use crate::testing::{TestResult, check_eq, snapshot, without_style};

/// The policy every page carries: no script, no fetch, inline styles only.
const POLICY: &str = "<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'\">";

#[test]
fn records_write_one_page_that_loads_and_runs_nothing() -> TestResult {
    let history = History::new("root")?;
    let mut args = vec!["records", "--format", "html"];
    args.extend(WEEK);
    let ran = history.run(&args);
    check_eq(&ran.code, &0, &ran.err)?;
    check_eq(&ran.out.lines().next(), &Some("<!doctype html>"), &ran.out)?;
    check_eq(&ran.out.contains(POLICY), &true, &ran.out)?;
    check_eq(&ran.out.ends_with("</html>\n"), &true, &ran.out)?;
    Ok(())
}

#[test]
fn sessions_and_stats_write_one_page_with_one_table() -> TestResult {
    let history = History::new("root")?;
    // (the command, its title in Japanese: セッション and 集計)
    for (command, japanese) in [
        (
            "sessions",
            "<h1>\u{30bb}\u{30c3}\u{30b7}\u{30e7}\u{30f3}</h1>",
        ),
        ("stats", "<h1>\u{96c6}\u{8a08}</h1>"),
    ] {
        let mut args = vec![command, "--format", "html"];
        args.extend(WEEK);
        let ran = history.run(&args);
        check_eq(&ran.code, &0, &ran.err)?;
        check_eq(&ran.out.lines().next(), &Some("<!doctype html>"), command)?;
        check_eq(&ran.out.contains(POLICY), &true, command)?;
        check_eq(
            &ran.out.matches("<div class=\"table\">\n<table>").count(),
            &1,
            &ran.out,
        )?;
        check_eq(
            &ran.out.ends_with("</table>\n</div>\n</body>\n</html>\n"),
            &true,
            &ran.out,
        )?;
        args.extend(["--lang", "ja"]);
        let worded = history.run(&args);
        check_eq(&worded.code, &0, &worded.err)?;
        check_eq(&worded.out.contains(japanese), &true, &worded.out)?;
    }
    Ok(())
}

#[test]
fn the_top_of_the_page_counts_the_masks_as_the_report_does() -> TestResult {
    let history = History::new("root")?;
    // (name, the flags, the masking line, the report)
    for (name, flags, line, report) in [
        (
            "masked",
            &["--mask-report", "text"][..],
            "<p class=\"masking\">Masked 1 by aws-credential</p>",
            "ccdig: masked 1 by aws-credential\n",
        ),
        (
            "not masked",
            &["--no-mask"][..],
            "<p class=\"masking\">Not masked (--no-mask)</p>",
            "",
        ),
    ] {
        let mut args = vec![
            "records",
            "--format",
            "html",
            "--type",
            "prompt,reply,call,result",
        ];
        args.extend(flags);
        args.extend(WEEK);
        let ran = history.run(&args);
        check_eq(&ran.code, &0, &ran.err)?;
        check_eq(
            &ran.out.contains(line),
            &true,
            &format!("{name}:\n{}", ran.out),
        )?;
        check_eq(&ran.err.as_str(), &report, name)?;
    }
    Ok(())
}

#[test]
fn lang_words_the_page() -> TestResult {
    let history = History::new("root")?;
    let mut args = vec!["records", "--format", "html", "--lang", "ja"];
    args.extend(WEEK);
    let ran = history.run(&args);
    check_eq(&ran.code, &0, &ran.err)?;
    // <h1>記録</h1>
    check_eq(&ran.out.contains("<html lang=\"ja\">"), &true, &ran.out)?;
    check_eq(
        &ran.out.contains("<h1>\u{8a18}\u{9332}</h1>"),
        &true,
        &ran.out,
    )?;
    Ok(())
}

#[test]
fn sub_agents_show_in_the_calls_that_started_them() -> TestResult {
    let history = History::new("subagents/root")?;
    let ran = history.run_week(&[
        "records",
        "--subagents",
        "--type",
        "prompt,reply,call,result",
        "--format",
        "html",
    ]);
    check_eq(&ran.code, &0, &ran.err)?;
    snapshot("cli/subagents.html", &without_style(&ran.out)?)
}
