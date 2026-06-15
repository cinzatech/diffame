//! `diffame` command-line tool.
//!
//! Usage:
//!     diffame <old> <new> [-f JSON|TEXT|SIDE] [-l EXT]
//!
//! When invoked by git as `diff.external`, diffame also accepts the 7-argument
//! form that git passes:
//!     diffame path old-file old-hex old-mode new-file new-hex new-mode
//!
//! The language is auto-detected from the file extension unless `-l` is given.

use std::env;
use std::fs;
use std::io::IsTerminal;
use std::path::Path;
use std::process::ExitCode;

use diffame::output::json::JsonFormatter;
use diffame::output::terminal::TerminalFormatter;
use diffame::output::text::TextFormatter;
use diffame::output::{strip_ansi, DiffFormatter, FormatInput};
use diffame::{diff_lines, diff_sources, languages, DiffOptions};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();

    let mut positional: Vec<&str> = Vec::new();
    let mut format = "SIDE".to_string();
    let mut lang_override: Option<String> = None;
    let mut max_file_size: Option<u64> = None;
    let mut parse_timeout: Option<u64> = None;
    let mut arg_index = 1;
    while arg_index < args.len() {
        match args[arg_index].as_str() {
            "-h" | "--help" => {
                print_usage(args.first().map_or("diffame", String::as_str));
                return ExitCode::SUCCESS;
            }
            "-f" if arg_index + 1 < args.len() => {
                format = args[arg_index + 1].clone();
                arg_index += 2;
            }
            "-l" if arg_index + 1 < args.len() => {
                lang_override = Some(args[arg_index + 1].clone());
                arg_index += 2;
            }
            "--max-file-size" if arg_index + 1 < args.len() => {
                let Ok(value) = args[arg_index + 1].parse::<u64>() else {
                    eprintln!("invalid value for --max-file-size: {}", args[arg_index + 1]);
                    return ExitCode::from(2);
                };
                max_file_size = Some(value);
                arg_index += 2;
            }
            "--parse-timeout" if arg_index + 1 < args.len() => {
                let Ok(value) = args[arg_index + 1].parse::<u64>() else {
                    eprintln!("invalid value for --parse-timeout: {}", args[arg_index + 1]);
                    return ExitCode::from(2);
                };
                parse_timeout = Some(value);
                arg_index += 2;
            }
            unknown if unknown.starts_with('-') => {
                eprintln!("unknown option: {unknown}");
                print_usage(&args[0]);
                return ExitCode::from(2);
            }
            _ => {
                positional.push(&args[arg_index]);
                arg_index += 1;
            }
        }
    }

    // Resolve old_path, new_path, the path used for language detection, and
    // the per-side display names shown in the header.
    //
    // Two accepted forms:
    //   diffame <old> <new>                                       (direct)
    //   diffame <path> <old-file> <old-hex> <old-mode>            (git diff.external)
    //           <new-file> <new-hex> <new-mode>
    //
    // In the git form both sides are versions of the same logical path, and
    // the actual files are temporary blobs, so the logical path is displayed
    // on both sides.
    let (old_path, new_path, lang_path, old_display, new_display): (&str, &str, &str, &str, &str) =
        match positional.len() {
            2 => (
                positional[0],
                positional[1],
                positional[0],
                positional[0],
                positional[1],
            ),
            7 => (
                positional[1],
                positional[4],
                positional[0],
                positional[0],
                positional[0],
            ),
            _ => {
                print_usage(args.first().map_or("diffame", String::as_str));
                return ExitCode::from(2);
            }
        };

    // Determine the language extension to use.
    let ext = lang_override.unwrap_or_else(|| {
        Path::new(lang_path)
            .extension()
            .and_then(|ext_os_str| ext_os_str.to_str())
            .unwrap_or("")
            .to_string()
    });

    // Try extension first, then fall back to filename (for Dockerfile, Makefile, etc.).
    let profile = languages::profile_for_ext(&ext).or_else(|| {
        let filename = Path::new(lang_path)
            .file_name()
            .and_then(|name_os_str| name_os_str.to_str())
            .unwrap_or("");
        languages::profile_for_filename(filename)
    });

    let old_src = match fs::read(old_path) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("failed to read {old_path}: {error}");
            return ExitCode::from(1);
        }
    };
    let new_src = match fs::read(new_path) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("failed to read {new_path}: {error}");
            return ExitCode::from(1);
        }
    };

    let mut diff_options = DiffOptions::default();
    if let Some(size) = max_file_size {
        diff_options.max_file_size = size;
    }
    if let Some(timeout_secs) = parse_timeout {
        diff_options.parse_timeout_us = timeout_secs.saturating_mul(1_000_000);
    }

    let diff_result = match profile {
        Some(lang_profile) => diff_sources(&old_src, &new_src, lang_profile, &diff_options),
        None => diff_lines(&old_src, &new_src, &diff_options),
    };
    let result = match diff_result {
        Ok(value) => value,
        Err(error) => {
            eprintln!("diff failed: {error}");
            return ExitCode::from(1);
        }
    };

    let lang_name = languages::language_name_for_ext(&ext);
    let (first_file, last_file) = file_sequence_position();

    let input = FormatInput {
        source_bytes: &old_src,
        destination_bytes: &new_src,
        result: &result,
        source_filename: Some(old_display),
        destination_filename: Some(new_display),
        language_name: lang_name,
        terminal_width: terminal_size::terminal_size()
            .map(|(terminal_size::Width(width), _)| width as usize),
        first_file,
        last_file,
    };

    if format.eq_ignore_ascii_case("JSON") {
        let output = JsonFormatter::format(&input);
        println!("{output}");
    } else if format.eq_ignore_ascii_case("SIDE") {
        // The formatter is a pure function that always emits ANSI styling;
        // whether the styling survives is decided once, here at the edge.
        let output = TerminalFormatter::format(&input);
        if color_enabled() {
            print!("{output}");
        } else {
            print!("{}", strip_ansi(&output));
        }
    } else {
        let output = TextFormatter::format(&input);
        print!("{output}");
    }

    ExitCode::SUCCESS
}

/// Position of this invocation within a multi-file diff sequence.
///
/// When git runs an external diff it invokes it once per changed path and
/// exposes `GIT_DIFF_PATH_COUNTER` (1-based) and `GIT_DIFF_PATH_TOTAL`, so
/// the concatenated outputs can render as one continuous table: a flat top
/// rule on the first file, connecting rules between files, and a closing
/// bottom rule on the last.  Outside git both values are absent and a single
/// invocation is both first and last.
fn file_sequence_position() -> (bool, bool) {
    let var = |name| {
        env::var(name)
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
    };
    match (var("GIT_DIFF_PATH_COUNTER"), var("GIT_DIFF_PATH_TOTAL")) {
        (Some(counter), Some(total)) => (counter <= 1, counter >= total),
        _ => (true, true),
    }
}

/// Whether colored output should reach stdout, following the common
/// conventions: `NO_COLOR` disables, `CLICOLOR_FORCE` (non-`"0"`) forces,
/// otherwise color is used only when stdout is a terminal.
fn color_enabled() -> bool {
    if env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty()) {
        return false;
    }
    if env::var_os("CLICOLOR_FORCE").is_some_and(|value| value != "0") {
        return true;
    }
    std::io::stdout().is_terminal()
}

fn print_usage(progname: &str) {
    eprintln!("usage: {progname} <old-file> <new-file> [-f JSON|TEXT|SIDE] [-l EXT]");
    eprintln!();
    eprintln!("  Also accepts git's 7-argument diff.external invocation.");
    eprintln!();
    eprintln!("  -f FORMAT          output format: SIDE (default), JSON, or TEXT");
    eprintln!("  -l EXT             override language (e.g. rs, py, js)");
    eprintln!(
        "  --max-file-size N  max input file size in bytes (default: 104857600, 0 = no limit)"
    );
    eprintln!("  --parse-timeout N  parser timeout in seconds (default: 60, 0 = no limit)");
    eprintln!("  -h                 show this help");
}
