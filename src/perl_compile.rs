use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range};

const COMPILE_TIMEOUT: Duration = Duration::from_secs(3);

/// Return the conventional project-local include directories that exist.
///
/// Perl's own default `@INC` remains implicit; adding it again via `-I` would
/// duplicate every standard entry.
pub fn workspace_include_paths(workspace_root: Option<&Path>) -> Vec<PathBuf> {
    let Some(root) = workspace_root else {
        return Vec::new();
    };

    ["lib", "local/lib/perl5"]
        .into_iter()
        .map(|path| root.join(path))
        .filter(|path| path.is_dir())
        .collect()
}

pub async fn check(
    path: &Path,
    text: &str,
    include_paths: &[PathBuf],
) -> Result<Vec<Diagnostic>, String> {
    if !path.is_file() {
        return Ok(Vec::new());
    }

    let Some(parent) = path.parent() else {
        return Err(format!("{} has no parent directory", path.display()));
    };
    let Some(file_name) = path.file_name() else {
        return Err(format!("{} has no file name", path.display()));
    };

    let args = command_args(include_paths, file_name);
    log::debug!(
        "perl -c command: cwd={:?} command=perl args={:?}",
        parent,
        args
    );

    let mut command = Command::new("perl");
    command
        .args(&args)
        .current_dir(parent)
        .stdin(Stdio::null())
        .kill_on_drop(true);

    let output = tokio::time::timeout(COMPILE_TIMEOUT, command.output())
        .await
        .map_err(|_| format!("perl -c timed out after {}s", COMPILE_TIMEOUT.as_secs()))?
        .map_err(|error| format!("failed to run perl -c: {error}"))?;

    let mut combined = String::from_utf8_lossy(&output.stderr).into_owned();
    combined.push_str(&String::from_utf8_lossy(&output.stdout));
    Ok(parse_diagnostics(text, path, &combined))
}

fn command_args(include_paths: &[PathBuf], file_name: &std::ffi::OsStr) -> Vec<OsString> {
    let mut args = Vec::with_capacity(include_paths.len() * 2 + 2);
    for path in include_paths {
        args.push(OsString::from("-I"));
        args.push(path.as_os_str().to_owned());
    }
    args.push(OsString::from("-c"));
    args.push(file_name.to_owned());
    args
}

fn parse_diagnostics(text: &str, path: &Path, output: &str) -> Vec<Diagnostic> {
    let Some(base_name) = path.file_name() else {
        return Vec::new();
    };
    let mut diagnostics = Vec::new();
    let mut seen = HashSet::new();

    for line in output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let Some((message, file_name, line_number)) = parse_location(line) else {
            continue;
        };
        let reported_path = Path::new(file_name);
        if reported_path != path && reported_path.as_os_str() != base_name {
            continue;
        }
        if !seen.insert((line_number, message.to_owned())) {
            continue;
        }

        diagnostics.push(Diagnostic {
            range: line_range(text, line_number),
            severity: Some(DiagnosticSeverity::ERROR),
            source: Some("perl -c".to_owned()),
            message: message.to_owned(),
            ..Default::default()
        });
    }

    diagnostics
}

fn parse_location(line: &str) -> Option<(&str, &str, usize)> {
    let line_marker = line.rfind(" line ")?;
    let line_number = line[line_marker + " line ".len()..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse::<usize>()
        .ok()?;
    if line_number == 0 {
        return None;
    }

    let before_line = &line[..line_marker];
    let at_marker = before_line.rfind(" at ")?;
    let message = before_line[..at_marker].trim();
    let file_name = before_line[at_marker + " at ".len()..].trim();
    if message.is_empty() || file_name.is_empty() {
        return None;
    }

    Some((message, file_name, line_number))
}

fn line_range(text: &str, line_number: usize) -> Range {
    let lines: Vec<&str> = text.split('\n').collect();
    let line = line_number
        .saturating_sub(1)
        .min(lines.len().saturating_sub(1));
    let character = lines[line].encode_utf16().count() as u32;

    Range {
        start: Position {
            line: line as u32,
            character: 0,
        },
        end: Position {
            line: line as u32,
            character,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(label: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("perl-lsp-{label}-{}-{nonce}", std::process::id()))
    }

    #[test]
    fn workspace_paths_include_only_existing_conventional_directories() {
        let root = temp_root("compile-paths");
        let lib = root.join("lib");
        std::fs::create_dir_all(&lib).unwrap();

        assert_eq!(workspace_include_paths(Some(&root)), vec![lib]);

        let local_lib = root.join("local/lib/perl5");
        std::fs::create_dir_all(&local_lib).unwrap();
        assert_eq!(
            workspace_include_paths(Some(&root)),
            vec![root.join("lib"), local_lib]
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn check_passes_workspace_lib_to_perl() {
        let root = temp_root("compile-check");
        let lib = root.join("lib");
        let module_dir = lib.join("Local");
        std::fs::create_dir_all(&module_dir).unwrap();
        std::fs::write(
            module_dir.join("CompileCheck.pm"),
            "package Local::CompileCheck; 1;\n",
        )
        .unwrap();

        let source = "use Local::CompileCheck;\n1;\n";
        let script = root.join("file.pl");
        std::fs::write(&script, source).unwrap();

        let diagnostics = check(&script, source, &[lib]).await.unwrap();
        assert!(diagnostics.is_empty(), "{diagnostics:?}");

        let diagnostics_without_lib = check(&script, source, &[]).await.unwrap();
        assert!(
            diagnostics_without_lib.iter().any(|diagnostic| diagnostic
                .message
                .contains("Can't locate Local/CompileCheck.pm")),
            "{diagnostics_without_lib:?}"
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn command_uses_workspace_include_paths_in_order() {
        let paths = vec![
            PathBuf::from("/workspace/lib"),
            PathBuf::from("/workspace/local/lib/perl5"),
        ];
        assert_eq!(
            command_args(&paths, std::ffi::OsStr::new("file.pl")),
            vec![
                "-I",
                "/workspace/lib",
                "-I",
                "/workspace/local/lib/perl5",
                "-c",
                "file.pl",
            ]
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>()
        );
    }

    #[test]
    fn parses_diagnostics_for_the_checked_file_only() {
        let text = "use strict;\nuse Foo;\nmy $x = ;\n";
        let output = "\
Can't locate Foo.pm in @INC at file.pl line 2.
syntax error at file.pl line 3, near \";\"
syntax error at other.pl line 1.
file.pl had compilation errors.
";

        let diagnostics = parse_diagnostics(text, Path::new("/tmp/file.pl"), output);
        assert_eq!(diagnostics.len(), 2);
        assert_eq!(diagnostics[0].message, "Can't locate Foo.pm in @INC");
        assert_eq!(diagnostics[0].range.start.line, 1);
        assert_eq!(diagnostics[1].message, "syntax error");
        assert_eq!(diagnostics[1].range.start.line, 2);
    }

    #[test]
    fn diagnostic_ranges_use_utf16_columns() {
        let diagnostics = parse_diagnostics(
            "my $x = \"😀\";\n",
            Path::new("/tmp/file.pl"),
            "syntax error at file.pl line 1.\n",
        );
        assert_eq!(diagnostics[0].range.end.character, 13);
    }
}
