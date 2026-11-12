//! tmux-sessionizer — fuzzy-select a project directory and open it in tmux.
//!
//! Port of the former `tmux-sessionizer.py`. Behaviour is deliberately
//! identical: same MRU cache file, same `fzf` invocation, same `tmux`
//! commands, same fallbacks (`direnv`, plain `nvim`).
//!
//! Layout of a session:
//!   bash — plain shell
//!   nvim — the editor window
//!   ai   — the AI CLI window
//!
//! Zero dependencies on purpose: the whole program is `std` only.

use std::collections::HashSet;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{self, Command, Output, Stdio};

const DEFAULT_DIR: &str = "~/projects";
/// Only the most recent entries are kept in the MRU cache.
const MRU_LIMIT: usize = 50;

fn main() {
    if let Err(err) = run() {
        eprintln!("tmux-sessionizer: {err}");
        process::exit(1);
    }
}

fn run() -> io::Result<()> {
    let arg = env::args().nth(1).unwrap_or_else(|| DEFAULT_DIR.to_string());
    let expanded = expand_user(&arg);
    let search_dir = expanded
        .to_string_lossy()
        .trim_end_matches('/')
        .to_string();

    let entries = collect_entries(&search_dir);
    let mru_file = mru_path(&search_dir);
    let mru = read_mru(&mru_file);
    let display = merge_mru(&mru, &entries);

    // `fzf --print-query --expect=ctrl-n` writes three lines to stdout:
    //   line 0: the query
    //   line 1: the key pressed (empty unless it was ctrl-n)
    //   line 2: the selected entry
    let output = fzf(&display.join("\n"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = stdout.lines();
    let query = lines.next().unwrap_or("");
    let key = lines.next().unwrap_or("");
    let selected = lines.next().unwrap_or("");

    // Cancelled with an empty query: nothing to do.
    if selected.is_empty() && query.is_empty() {
        return Ok(());
    }

    // ctrl-n asks for "this query is a new directory name"; so does
    // accepting a query that matched nothing.
    let mut selected = selected.to_string();
    if key == "ctrl-n" || (selected.is_empty() && !query.is_empty()) {
        selected = format!("{search_dir}/{query}");
        fs::create_dir_all(&selected)?;
    }

    write_mru(&mru_file, &selected, &mru)?;

    // tmux session names cannot contain dots (it treats them as a
    // session:window separator), so the directory name is sanitised.
    let session_name = Path::new(&selected)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| selected.clone())
        .replace('.', "_");

    let tmux_running = capture("pgrep", &["tmux"]).is_ok_and(|out| out.status.success());
    let in_tmux = env::var("TMUX").is_ok_and(|value| !value.is_empty());

    // No server and no client: start one attached session and let tmux
    // take over the terminal.
    if !in_tmux && !tmux_running {
        tmux(&["new-session", "-s", &session_name, "-c", &selected]);
        return Ok(());
    }

    if !has_session(&session_name) {
        tmux(&[
            "new-session", "-ds", &session_name, "-n", "bash", "-c", &selected,
        ]);
        tmux(&["new-window", "-t", &session_name, "-n", "nvim", "-c", &selected]);

        let project = Path::new(&selected);
        let nvim_cmd = if project.join(".envrc").is_file() {
            "direnv exec . nvim . || nvim .".to_string()
        } else {
            "nvim .".to_string()
        };
        tmux(&[
            "send-keys",
            "-t",
            &format!("{session_name}:nvim"),
            &nvim_cmd,
            "C-m",
        ]);

        tmux(&["new-window", "-t", &session_name, "-n", "ai", "-c", &selected]);
        // "/share," in $TMUX marks the work server (its socket path lives
        // under .../share,...), which gets a different AI CLI.
        let ai_cmd = if env::var("TMUX").unwrap_or_default().contains("/share,") {
            env::var("SESSIONIZER_AI_CMD_WORK").unwrap_or_else(|_| "work-opencode".to_string())
        } else {
            env::var("SESSIONIZER_AI_CMD").unwrap_or_else(|_| "opencode".to_string())
        };
        tmux(&[
            "send-keys",
            "-t",
            &format!("{session_name}:ai"),
            &ai_cmd,
            "C-m",
        ]);

        tmux(&["select-window", "-t", &format!("{session_name}:bash")]);
        // PROJECT_ROOT is consumed by a project's .tmux file when the user
        // sources it manually (prefix T). Never source it automatically: a
        // foreign repo's .tmux would run unsandboxed code server-side.
        tmux(&[
            "set-environment",
            "-t",
            &session_name,
            "PROJECT_ROOT",
            &selected,
        ]);
    }

    tmux(&["switch-client", "-t", &session_name]);
    Ok(())
}

/// Top-level directories followed by their `.gitmodules` submodules.
fn collect_entries(search_dir: &str) -> Vec<String> {
    let mut dirs: Vec<PathBuf> = match fs::read_dir(search_dir) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect(),
        // A missing search directory is not an error: fzf still lets the
        // user type a name, and ctrl-n creates it.
        Err(_) => Vec::new(),
    };
    // Python sorted Path objects, i.e. by string; OsStr ordering matches.
    dirs.sort_by(|a, b| a.as_os_str().cmp(b.as_os_str()));

    let mut entries = Vec::new();
    for dir in &dirs {
        entries.push(dir.to_string_lossy().into_owned());

        let gitmodules = dir.join(".gitmodules");
        let Ok(text) = fs::read_to_string(&gitmodules) else {
            continue;
        };
        for line in text.lines() {
            if let Some(rel) = gitmodules_path(line) {
                let submodule = dir.join(rel);
                if submodule.is_dir() {
                    entries.push(submodule.to_string_lossy().into_owned());
                }
            }
        }
    }
    entries
}

/// Hand-rolled equivalent of `re.search(r"path\s*=\s*(\S+)", line)`.
///
/// `path` may appear anywhere in the line, whitespace around `=` is
/// optional, and the value runs until the next whitespace character.
fn gitmodules_path(line: &str) -> Option<&str> {
    let after_key = &line[line.find("path")? + "path".len()..];
    let after_eq = after_key.trim_start().strip_prefix('=')?;
    let value = after_eq.trim_start();
    let end = value.find(char::is_whitespace).unwrap_or(value.len());
    let value = &value[..end];
    (!value.is_empty()).then_some(value)
}

fn mru_path(search_dir: &str) -> PathBuf {
    // `re.sub(r"[ /]", "__", search_dir)` — one separator per character.
    let cache = env::var_os("XDG_CACHE_HOME")
        .map(|value| expand_user(&value.to_string_lossy()))
        .unwrap_or_else(|| home_dir().join(".cache"));
    cache.join(format!(
        "tmux-sessionizer-mru-{}",
        search_dir.replace([' ', '/'], "__")
    ))
}

fn read_mru(path: &Path) -> Vec<String> {
    fs::read_to_string(path)
        .map(|text| {
            text.lines()
                .filter(|line| !line.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Most-recently-used entries that still exist come first, then everything
/// that was never used, in directory order.
fn merge_mru(mru: &[String], entries: &[String]) -> Vec<String> {
    let seen: HashSet<&str> = mru.iter().map(String::as_str).collect();
    let mut display: Vec<String> = mru
        .iter()
        .filter(|entry| Path::new(entry).is_dir())
        .cloned()
        .collect();
    display.extend(
        entries
            .iter()
            .filter(|entry| !seen.contains(entry.as_str()))
            .cloned(),
    );
    display
}

fn write_mru(path: &Path, selected: &str, mru: &[String]) -> io::Result<()> {
    let mut next = Vec::with_capacity(mru.len() + 1);
    next.push(selected.to_string());
    next.extend(
        mru.iter()
            .filter(|entry| entry.as_str() != selected)
            .cloned(),
    );
    next.truncate(MRU_LIMIT);

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, format!("{}\n", next.join("\n")))
}

/// Run `fzf` with the candidate list on stdin.
fn fzf(input: &str) -> io::Result<Output> {
    let mut child = Command::new("fzf")
        .args(["--no-sort", "--print-query", "--expect=ctrl-n"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    // Dropping the handle closes the pipe; fzf reads until EOF.
    {
        let mut stdin = child.stdin.take().expect("stdin is piped");
        stdin.write_all(input.as_bytes())?;
    }
    child.wait_with_output()
}

fn has_session(name: &str) -> bool {
    capture("tmux", &["has-session", &format!("-t={name}")]).is_ok_and(|out| out.status.success())
}

/// tmux failures are ignored on purpose: a missing server or a window that
/// already exists must not abort the rest of the sequence.
fn tmux(args: &[&str]) {
    let _ = capture("tmux", args);
}

fn capture(program: &str, args: &[&str]) -> io::Result<Output> {
    Command::new(program).args(args).output()
}

/// `~` and `~/...` only — `~user` is not needed here.
fn expand_user(path: &str) -> PathBuf {
    if path == "~" {
        return home_dir();
    }
    match path.strip_prefix("~/") {
        Some(rest) => home_dir().join(rest),
        None => PathBuf::from(path),
    }
}

fn home_dir() -> PathBuf {
    env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}
