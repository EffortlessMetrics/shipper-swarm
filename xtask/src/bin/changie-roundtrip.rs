//! Local proof that Changie's retained version files reproduce `CHANGELOG.md`.
//!
//! This is deliberately a maintainer/local command, not a GitHub Actions gate.
//! It executes the pinned Changie binary, renders `changie merge --dry-run`,
//! and compares that output with the tracked changelog while allowing only a
//! final-newline difference.

use anyhow::{Context, Result, bail};
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const CHANGIE_VERSION: &str = "1.25.1";

fn main() -> Result<()> {
    let root = repo_root()?;
    let observed_version = changie_version(&root)?;
    if !changie_version_matches(&observed_version) {
        bail!(
            "expected Changie v{CHANGIE_VERSION}, received `{observed_version}`; align the local tool before validating the baseline"
        );
    }

    let merge = command_output(&root, "changie", ["merge", "--dry-run"])
        .context("failed to launch `changie merge --dry-run`")?;
    if !merge.status.success() {
        bail!("Changie merge dry-run failed: {}", output_text(&merge));
    }

    let rendered = String::from_utf8(merge.stdout)
        .context("Changie merge output was not valid UTF-8")?;
    let tracked_path = root.join("CHANGELOG.md");
    let tracked = fs::read_to_string(&tracked_path)
        .with_context(|| format!("failed to read {}", tracked_path.display()))?;

    if normalize_final_newline(&rendered) != normalize_final_newline(&tracked) {
        let mismatch = first_mismatch(&rendered, &tracked);
        bail!(
            "Changie round trip does not reproduce CHANGELOG.md. {mismatch}\nRun `changie merge --dry-run`, inspect the retained `.changes/*.md` baseline, and do not run a writing merge until the difference is understood."
        );
    }

    eprintln!(
        "changie round trip: PASS ({observed_version}; {} bytes)",
        normalize_final_newline(&tracked).len()
    );
    Ok(())
}

fn repo_root() -> Result<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("failed to launch git")?;
    if !output.status.success() {
        bail!("not inside a Git worktree: {}", output_text(&output));
    }
    let root = String::from_utf8(output.stdout).context("Git worktree path was not UTF-8")?;
    Ok(PathBuf::from(root.trim()))
}

fn changie_version(root: &Path) -> Result<String> {
    let output = command_output(root, "changie", ["--version"]).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            anyhow::anyhow!(
                "required executable `changie` was not found; install Changie v{CHANGIE_VERSION} and retry"
            )
        } else {
            anyhow::anyhow!("failed to launch Changie: {error}")
        }
    })?;
    if !output.status.success() {
        bail!("`changie --version` failed: {}", output_text(&output));
    }
    Ok(output_text(&output))
}

fn changie_version_matches(output: &str) -> bool {
    output
        .split_whitespace()
        .map(|token| token.trim_start_matches('v'))
        .any(|token| token == CHANGIE_VERSION)
}

fn normalize_final_newline(text: &str) -> &str {
    text.strip_suffix("\r\n")
        .or_else(|| text.strip_suffix('\n'))
        .unwrap_or(text)
}

fn first_mismatch(rendered: &str, tracked: &str) -> String {
    let rendered = normalize_final_newline(rendered);
    let tracked = normalize_final_newline(tracked);
    let rendered_lines: Vec<&str> = rendered.lines().collect();
    let tracked_lines: Vec<&str> = tracked.lines().collect();
    let shared = rendered_lines.len().min(tracked_lines.len());

    for index in 0..shared {
        if rendered_lines[index] != tracked_lines[index] {
            return format!(
                "First mismatch at line {}: rendered `{}`; tracked `{}`.",
                index + 1,
                rendered_lines[index],
                tracked_lines[index]
            );
        }
    }

    if rendered_lines.len() != tracked_lines.len() {
        return format!(
            "Line-count mismatch after line {shared}: rendered {} lines; tracked {} lines.",
            rendered_lines.len(),
            tracked_lines.len()
        );
    }

    "The files differ outside the permitted final-newline normalization.".to_string()
}

fn command_output<I, S>(cwd: &Path, program: &str, args: I) -> io::Result<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Command::new(program).current_dir(cwd).args(args).output()
}

fn output_text(output: &Output) -> String {
    let mut text = String::new();
    if !output.stdout.is_empty() {
        text.push_str(&String::from_utf8_lossy(&output.stdout));
    }
    if !output.stderr.is_empty() {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&String::from_utf8_lossy(&output.stderr));
    }
    text.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_match_is_exact() {
        assert!(changie_version_matches("changie version v1.25.1"));
        assert!(changie_version_matches("v1.25.1"));
        assert!(!changie_version_matches("changie version v1.25.10"));
        assert!(!changie_version_matches("changie version vdev"));
    }

    #[test]
    fn only_the_final_newline_is_normalized() {
        assert_eq!(normalize_final_newline("body\n"), "body");
        assert_eq!(normalize_final_newline("body\r\n"), "body");
        assert_eq!(normalize_final_newline("body\n\n"), "body\n");
        assert_ne!(normalize_final_newline("body "), "body");
    }

    #[test]
    fn mismatch_reports_the_first_different_line() {
        assert_eq!(
            first_mismatch("one\ntwo\n", "one\nthree"),
            "First mismatch at line 2: rendered `two`; tracked `three`."
        );
    }

    #[test]
    fn mismatch_reports_missing_history() {
        assert_eq!(
            first_mismatch("one\n", "one\ntwo\n"),
            "Line-count mismatch after line 1: rendered 1 lines; tracked 2 lines."
        );
    }
}
