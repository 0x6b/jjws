use std::{env::var, path::Path, process::Command};

use anyhow::{Context, Result, bail};

pub fn open_tab(workspace_path: &Path, command: Option<&str>) -> Result<Option<String>> {
    if !is_available() {
        return Ok(None);
    }

    let workspace_path = workspace_path.to_str().context("workspace path is not valid UTF-8")?;
    let cd = format!("cd {}", shell_escape_single(workspace_path));
    let full_command = match command {
        Some(cmd) => format!("{cd} && {cmd}"),
        None => cd,
    };
    let script = build_tab_script(workspace_path, &full_command);
    run_applescript_output(&script)
        .context("failed to create Ghostty tab")
        .map(Some)
}

fn is_available() -> bool {
    cfg!(target_os = "macos") && var("TERM_PROGRAM").is_ok_and(|v| v == "ghostty")
}

fn build_tab_script(workspace_path: &str, command: &str) -> String {
    format!(
        r#"tell application "Ghostty"
    set cfg to new surface configuration
    set initial working directory of cfg to "{workspace_path}"
    set newTab to new tab in front window with configuration cfg
    set newTerm to focused terminal of newTab
    focus newTerm
    delay 0.2
    input text "{command}" to newTerm
    send key "enter" to newTerm
    return id of newTerm
end tell"#,
        workspace_path = applescript_escape(workspace_path),
        command = applescript_escape(command),
    )
}

fn run_applescript_output(script: &str) -> Result<String> {
    let output = Command::new("osascript")
        .args(["-e", script])
        .output()
        .context("failed to run osascript")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("osascript failed: {}", stderr.trim());
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn applescript_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if matches!(ch, '\\' | '"') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

fn shell_escape_single(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 2);
    out.push('\'');
    for ch in input.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applescript_escape_escapes_quotes_and_backslashes() {
        assert_eq!(applescript_escape(r#"say "hi""#), r#"say \"hi\""#);
        assert_eq!(applescript_escape(r"path\to\file"), r"path\\to\\file");
    }

    #[test]
    fn shell_escape_single_wraps_string() {
        assert_eq!(shell_escape_single("hello"), "'hello'");
        assert_eq!(shell_escape_single("it's"), "'it'\\''s'");
    }

    #[test]
    fn build_tab_script_contains_working_directory_and_command() {
        let script = build_tab_script("/tmp/workspace", "cd '/tmp/workspace'");
        assert!(script.contains("new tab in front window"));
        assert!(script.contains("/tmp/workspace"));
        assert!(script.contains("cd '/tmp/workspace'"));
    }
}
