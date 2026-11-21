//! Acceptance evidence of `application-startup-boundaries` (OpenSpec change
//! `specify-no-background-start`), tasks 3.1 to 3.4: source and configuration scans of the
//! repository, the native operations and commands enumerated, and the `chiral` binary run
//! against a loopback chain RPC beside legacy data, observed from outside (macOS).
//!
//! Scans read files only. The source scans that must tell code from comments and test code use
//! `rust::lex`, which blanks comments and literal contents without moving any byte, and
//! `rust::test_regions`, which finds every `#[cfg(test)]` item; each scanner is itself tested
//! below on code that has the forbidden item, so that no scan can pass because it read nothing.
//! The denylist scans (node and miner executables, start-at-login mechanisms, Core-space calls)
//! read whole files, comments and tests included, which is stricter; the only file excluded is
//! this one, which has to name what it looks for.
//!
//! The binary tests run `chiral` against a loopback chain RPC (Unix). On macOS they start it
//! suspended and watch it with kqueue from before its first instruction
//! (`support/process_observer.rs`): zero fork notifications means that the process created no
//! process at all. The Linux `/proc` observation that the tasks name stays open.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[path = "support/process_observer.rs"]
mod process_observer;

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

fn read(relative: &str) -> String {
    std::fs::read_to_string(repository().join(relative)).unwrap_or_else(|error| panic!("{relative}: {error}"))
}

/// Every regular file under `root`, recursively, except under directories named in `skip`.
fn files_under(root: &Path, skip: &[&str]) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).unwrap_or_else(|error| panic!("{directory:?}: {error}")) {
            let entry = entry.unwrap();
            let kind = entry.file_type().unwrap();
            let path = entry.path();
            if kind.is_dir() {
                if !skip.iter().any(|name| entry.file_name() == *name) {
                    pending.push(path);
                }
            } else if kind.is_file() {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

fn relative(path: &Path) -> String {
    path.strip_prefix(repository()).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

/// 1-based line of byte offset `at` in `text`.
fn line_of(text: &str, at: usize) -> usize {
    text[..at].bytes().filter(|byte| *byte == b'\n').count() + 1
}

fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Offsets of `word` in `text` where it is a whole identifier.
fn word_offsets(text: &str, word: &str) -> Vec<usize> {
    let bytes = text.as_bytes();
    text.match_indices(word)
        .map(|(at, _)| at)
        .filter(|at| *at == 0 || !is_identifier_byte(bytes[at - 1]))
        .filter(|at| bytes.get(at + word.len()).is_none_or(|byte| !is_identifier_byte(*byte)))
        .collect()
}

/// A minimal Rust lexer and the structure the scans need: comments, literals, `#[cfg(test)]`
/// items and function bodies.
mod rust {
    use std::ops::Range;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Kind {
        Code,
        Comment,
        /// The contents of a string, byte-string, raw-string or character literal; its quotes
        /// and prefixes are code.
        Literal,
    }

    /// Splits Rust source into code, comments (line, doc and nested block comments) and literal
    /// contents. A lifetime (`'a`) is code; a raw identifier (`r#type`) is code.
    pub fn lex(source: &str) -> Vec<(Kind, Range<usize>)> {
        let bytes = source.as_bytes();
        let mut segments = Vec::new();
        let mut code_start = 0;
        let mut at = 0;
        let push = |segments: &mut Vec<(Kind, Range<usize>)>, kind, range: Range<usize>| {
            if !range.is_empty() {
                segments.push((kind, range));
            }
        };
        while at < bytes.len() {
            let next = bytes.get(at + 1).copied();
            let previous_identifier = at > 0 && super::is_identifier_byte(bytes[at - 1]);
            if bytes[at] == b'/' && next == Some(b'/') {
                let end = source[at..].find('\n').map_or(bytes.len(), |offset| at + offset);
                push(&mut segments, Kind::Code, code_start..at);
                push(&mut segments, Kind::Comment, at..end);
                at = end;
                code_start = at;
            } else if bytes[at] == b'/' && next == Some(b'*') {
                let (mut end, mut depth) = (at + 2, 1);
                while end < bytes.len() && depth > 0 {
                    if bytes[end] == b'/' && bytes.get(end + 1) == Some(&b'*') {
                        depth += 1;
                        end += 2;
                    } else if bytes[end] == b'*' && bytes.get(end + 1) == Some(&b'/') {
                        depth -= 1;
                        end += 2;
                    } else {
                        end += 1;
                    }
                }
                assert_eq!(depth, 0, "unterminated block comment");
                push(&mut segments, Kind::Code, code_start..at);
                push(&mut segments, Kind::Comment, at..end);
                at = end;
                code_start = at;
            } else if !previous_identifier && (bytes[at] == b'r' || (bytes[at] == b'b' && next == Some(b'r'))) {
                let hashes_from = if bytes[at] == b'r' { at + 1 } else { at + 2 };
                let mut quote = hashes_from;
                while bytes.get(quote) == Some(&b'#') {
                    quote += 1;
                }
                if bytes.get(quote) != Some(&b'"') {
                    at += 1;
                    continue;
                }
                let closing = format!("\"{}", "#".repeat(quote - hashes_from));
                let content = quote + 1;
                let end = source[content..].find(&closing).map(|offset| content + offset).expect("unterminated raw string");
                push(&mut segments, Kind::Code, code_start..content);
                push(&mut segments, Kind::Literal, content..end);
                at = end + closing.len();
                code_start = end;
            } else if bytes[at] == b'"' {
                let content = at + 1;
                let mut end = content;
                while bytes[end] != b'"' {
                    end += if bytes[end] == b'\\' { 2 } else { 1 };
                }
                push(&mut segments, Kind::Code, code_start..content);
                push(&mut segments, Kind::Literal, content..end);
                at = end + 1;
                code_start = end;
            } else if bytes[at] == b'\'' {
                let content = at + 1;
                let end = if next == Some(b'\\') {
                    // An escape: `'\''`, `'\\'`, `'\n'`, `'\x7f'`, `'\u{..}'`.
                    let from = if bytes.get(at + 2) == Some(&b'\'') || bytes.get(at + 2) == Some(&b'\\') { at + 3 } else { at + 2 };
                    Some(from + source[from..].find('\'').expect("unterminated character literal"))
                } else {
                    source[content..].chars().next().map(|c| content + c.len_utf8()).filter(|end| bytes.get(*end) == Some(&b'\''))
                };
                match end {
                    Some(end) => {
                        push(&mut segments, Kind::Code, code_start..content);
                        push(&mut segments, Kind::Literal, content..end);
                        at = end + 1;
                        code_start = end;
                    }
                    None => at += 1,
                }
            } else {
                at += 1;
            }
        }
        push(&mut segments, Kind::Code, code_start..bytes.len());
        segments
    }

    /// `source` with every byte of the segments of the given kinds, except newlines, replaced
    /// by a space: offsets and line numbers stay those of the file.
    pub fn blank(source: &str, kinds: &[Kind]) -> String {
        let mut bytes = source.as_bytes().to_vec();
        for (kind, range) in lex(source) {
            if kinds.contains(&kind) {
                for byte in &mut bytes[range] {
                    if *byte != b'\n' {
                        *byte = b' ';
                    }
                }
            }
        }
        String::from_utf8(bytes).unwrap()
    }

    /// Comments and literal contents blanked.
    pub fn code(source: &str) -> String {
        blank(source, &[Kind::Comment, Kind::Literal])
    }

    /// Comments blanked, literals kept.
    pub fn without_comments(source: &str) -> String {
        blank(source, &[Kind::Comment])
    }

    /// The offset just after the bracket that closes the one at `open` in `code`.
    pub fn matching(code: &str, open: usize) -> usize {
        let bytes = code.as_bytes();
        let mut depth = 0i32;
        for (at, byte) in bytes.iter().enumerate().skip(open) {
            match byte {
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return at + 1;
                    }
                }
                _ => {}
            }
        }
        panic!("unbalanced brackets from offset {open}");
    }

    /// The span of every item, statement, field or expression that `#[cfg(test)]` marks in
    /// `code` (comments and literals blanked), from the attribute to the end of what it marks:
    /// the `;` or `,` that ends it, the `}` that closes a block it opened, or, for the last
    /// field of a block, the end of that block. A span ends no later than that, so that test
    /// code is never taken for more than it is.
    pub fn test_regions(code: &str) -> Vec<Range<usize>> {
        let bytes = code.as_bytes();
        let mut regions = Vec::new();
        for (start, _) in code.match_indices("#[") {
            let end = matching(code, start + 1);
            let attribute: String = code[start..end].chars().filter(|c| !c.is_whitespace()).collect();
            if attribute != "#[cfg(test)]" {
                continue;
            }
            let mut at = end;
            // Further attributes of the same item.
            loop {
                while bytes[at].is_ascii_whitespace() {
                    at += 1;
                }
                if bytes[at] == b'#' && bytes.get(at + 1) == Some(&b'[') {
                    at = matching(code, at + 1);
                } else {
                    break;
                }
            }
            let mut depth = 0i32;
            let mut block = false;
            let mut item_end = None;
            for (offset, byte) in bytes.iter().enumerate().skip(at) {
                match byte {
                    b'(' | b'[' | b'{' => {
                        if *byte == b'{' && depth == 0 {
                            block = true;
                        }
                        depth += 1;
                    }
                    b')' | b']' | b'}' => {
                        depth -= 1;
                        if depth < 0 {
                            item_end = Some(offset);
                            break;
                        }
                        if depth == 0 && *byte == b'}' && block {
                            item_end = Some(offset + 1);
                            break;
                        }
                    }
                    b';' | b',' if depth == 0 => {
                        item_end = Some(offset + 1);
                        break;
                    }
                    _ => {}
                }
            }
            regions.push(start..item_end.unwrap_or_else(|| panic!("unterminated #[cfg(test)] item at offset {start}")));
        }
        regions
    }

    /// The files that `#[cfg(test)]` modules load from another file (`#[path = "x.rs"] mod m;`
    /// or `mod m;`), relative to the directory of `source`.
    pub fn test_module_files(source: &str) -> Vec<String> {
        let code = code(source);
        let kept = without_comments(source);
        let mut files = Vec::new();
        for region in test_regions(&code) {
            let text = &code[region.clone()];
            let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
            if !compact.ends_with(';') || super::word_offsets(text, "mod").is_empty() {
                continue;
            }
            let module_at = super::word_offsets(text, "mod")[0];
            let name: String = text[module_at + 3..].trim_start().chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
            let original = &kept[region];
            match original.find("#[path") {
                Some(path_at) => {
                    let quoted = &original[path_at..];
                    let open = quoted.find('"').expect("path attribute string");
                    let close = quoted[open + 1..].find('"').expect("path attribute string") + open + 1;
                    files.push(quoted[open + 1..close].to_owned());
                }
                None => files.push(format!("{name}.rs")),
            }
        }
        files
    }

    /// Every function with a body in `code` (comments and literals blanked): its name, the
    /// offset of `fn` and the range of its body, braces included.
    pub fn functions(code: &str) -> Vec<(String, usize, Range<usize>)> {
        let bytes = code.as_bytes();
        let mut found = Vec::new();
        for at in super::word_offsets(code, "fn") {
            let name: String = code[at + 2..].trim_start().chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
            if name.is_empty() {
                continue; // `fn(` types.
            }
            let mut depth = 0i32;
            let mut body = None;
            for (offset, byte) in bytes.iter().enumerate().skip(at) {
                match byte {
                    b'(' | b'[' => depth += 1,
                    b')' | b']' => depth -= 1,
                    b'{' if depth == 0 => {
                        body = Some(offset);
                        break;
                    }
                    b';' if depth == 0 => break,
                    _ => {}
                }
            }
            if let Some(open) = body {
                found.push((name, at, open..matching(code, open)));
            }
        }
        found
    }

    /// The name of the innermost function whose body holds `at`.
    pub fn enclosing_function(functions: &[(String, usize, Range<usize>)], at: usize) -> Option<String> {
        functions.iter().filter(|(_, _, body)| body.contains(&at)).min_by_key(|(_, _, body)| body.len()).map(|(name, _, _)| name.clone())
    }
}

/// A Rust source file of the library or binaries, with its code view and its test regions.
struct Source {
    path: String,
    text: String,
    code: String,
    tests: Vec<std::ops::Range<usize>>,
    test_only: bool,
}

impl Source {
    fn new(path: String, text: String, test_only: bool) -> Self {
        let code = rust::code(&text);
        let tests = rust::test_regions(&code);
        Self { path, text, code, tests, test_only }
    }
    fn in_test(&self, at: usize) -> bool {
        self.test_only || self.tests.iter().any(|region| region.contains(&at))
    }
}

/// Every `.rs` file under `src-tauri/src` and `src-tauri/build.rs`; files that only a
/// `#[cfg(test)]` module loads are marked test-only.
fn rust_sources() -> Vec<Source> {
    let root = repository().join("src-tauri");
    let mut paths: Vec<PathBuf> = files_under(&root.join("src"), &[]).into_iter().filter(|path| path.extension().is_some_and(|e| e == "rs")).collect();
    paths.push(root.join("build.rs"));
    let mut test_only = BTreeSet::new();
    for path in &paths {
        for file in rust::test_module_files(&std::fs::read_to_string(path).unwrap()) {
            test_only.insert(path.parent().unwrap().join(file));
        }
    }
    paths
        .into_iter()
        .map(|path| {
            let text = std::fs::read_to_string(&path).unwrap();
            Source::new(relative(&path), text, test_only.contains(&path))
        })
        .collect()
}

/// The process-creating constructs of Rust code: `Command::new`, a path or import of the
/// `Command` of `std::process` or `tokio::process`, the C library's process functions and the
/// Windows process APIs. Returns `(offset, construct)` for each.
fn process_constructs(code: &str) -> Vec<(usize, String)> {
    let bytes = code.as_bytes();
    let mut found = Vec::new();
    for at in word_offsets(code, "Command") {
        let after: String = code[at + 7..].chars().filter(|c| !c.is_whitespace()).take(5).collect();
        let before: String = code[..at].chars().rev().filter(|c| !c.is_whitespace()).take(9).collect::<String>().chars().rev().collect();
        // `process::Command`, or `Command` inside a `process::{...}` import group.
        let in_process_group = code[..at].rfind("process::{").is_some_and(|group| {
            let close = rust::matching(code, group + "process::".len());
            close > at && !code[group..at].contains(';')
        });
        if after.starts_with("::new") {
            found.push((at, "Command::new".into()));
        } else if before.ends_with("process::") || in_process_group {
            found.push((at, "process::Command".into()));
        }
    }
    for glob in code.match_indices("process::*").map(|(at, _)| at) {
        found.push((glob, "process::*".into()));
    }
    // A glob import of the C library brings its process functions in under their bare names.
    for module in ["libc", "unistd"] {
        for at in word_offsets(code, module) {
            if code[at + module.len()..].chars().filter(|c| !c.is_whitespace()).take(3).collect::<String>() == "::*" {
                found.push((at, format!("{module}::*")));
            }
        }
    }
    // Inside a `libc::{...}` or `unistd::{...}` import group.
    let in_c_group = |at: usize| {
        ["libc::{", "unistd::{"].iter().any(|group| {
            code[..at].rfind(group).is_some_and(|start| {
                let close = rust::matching(code, start + group.len() - 1);
                close > at && !code[start..at].contains(';')
            })
        })
    };
    // Names that only ever mean a process function are flagged wherever they appear as a whole
    // identifier, so that an import group or alias cannot hide them; the ordinary words
    // (`system`, `daemon`, `clone`) only as a path or group member of the C library.
    let unambiguous = ["fork", "vfork", "forkpty", "posix_spawn", "posix_spawnp", "execv", "execve", "execvp", "execvpe", "execl", "execle", "execlp", "fexecve", "popen"];
    for function in unambiguous.iter().chain(&["system", "daemon", "clone"]) {
        for at in word_offsets(code, function) {
            let before: String = code[..at].chars().rev().filter(|c| !c.is_whitespace()).take(8).collect::<String>().chars().rev().collect();
            if before.ends_with("libc::") || before.ends_with("unistd::") || in_c_group(at) {
                found.push((at, format!("libc::{function}")));
            } else if unambiguous.contains(function) {
                found.push((at, (*function).to_owned()));
            }
        }
    }
    for windows in ["CreateProcessW", "CreateProcessA", "CreateProcess", "CreateProcessAsUserW", "ShellExecuteW", "ShellExecuteExW", "ShellExecuteA", "WinExec"] {
        for at in word_offsets(code, windows) {
            found.push((at, windows.into()));
        }
    }
    let _ = bytes;
    found.sort();
    found.dedup();
    found
}

#[test]
fn the_rust_scanner_tells_code_from_comments_literals_and_test_code() {
    let source = r##"
//! fork and Command::new in a doc comment
use tokio::process::{Child, Command};
fn launch() { let _ = Command::new("x"); /* libc::fork() */ }
fn text() -> &'static str { "std::process::Command::new(\"geth\")" }
fn raw() -> &'static str { r#"libc::system"# }
fn lifetime<'a>(value: &'a str) -> char { let _ = value; '{' }
#[cfg(test)]
mod tests { fn helper() { std::process::Command::new("y"); } }
#[cfg(test)]
#[path = "other_tests.rs"]
mod more;
struct Fields { kept: u8, #[cfg(test)] probe: std::sync::Mutex<Option<(u8, u8)>>, after: u8 }
fn statement() { #[cfg(test)] unsafe { libc::fork(); } let _ = std::process::Command::new("z"); }
#[cfg(not(test))]
fn production() { unsafe { libc::posix_spawn(); } }
fn grouped() { use libc::{c_int, system}; let _: c_int = unsafe { vfork() }; let _ = value.clone(); }
use libc::*;
"##;
    let code = rust::code(source);
    assert_eq!(code.len(), source.len());
    assert_eq!(code.lines().count(), source.lines().count());
    assert!(!code.contains("doc comment") && !code.contains("geth") && !code.contains("libc::system"));
    let tests = rust::test_regions(&code);
    assert_eq!(tests.len(), 4, "{tests:?}");
    let found: Vec<_> = process_constructs(&code)
        .into_iter()
        .filter(|(at, _)| !tests.iter().any(|region| region.contains(at)))
        .map(|(at, construct)| (line_of(source, at), construct))
        .collect();
    assert_eq!(found, vec![
        (3, "process::Command".to_owned()),
        (4, "Command::new".to_owned()),
        (14, "Command::new".to_owned()),
        (16, "libc::posix_spawn".to_owned()),
        // A group member and a bare call; a `.clone()` method is not a process function.
        (17, "libc::system".to_owned()),
        (17, "vfork".to_owned()),
        (18, "libc::*".to_owned()),
    ]);
    // The field after the test-only one, and the statement after the test-only block, are code.
    let after = source.find("after: u8").unwrap();
    assert!(!tests.iter().any(|region| region.contains(&after)));
    assert_eq!(rust::test_module_files(source), vec!["other_tests.rs".to_owned()]);
    let functions = rust::functions(&code);
    let at = source.find("Command::new(\"x\")").unwrap();
    assert_eq!(rust::enclosing_function(&functions, at).as_deref(), Some("launch"));
}

/// Task 3.1 (requirement 1): the only process construction outside test code in the library,
/// the binaries and the build script is `engine_runtime.rs`'s `command()`, whose `Command::new`
/// only the pinned llama-server launch and the device observers reach, the observers being the
/// verified engine's `--list-devices` and six platform tools at fixed system locations; only
/// the provider daemon starts a managed engine. Would catch a node, miner or any other process
/// started by the desktop, the CLI or a binary, and another caller of `command()`.
#[test]
fn the_only_process_construction_is_the_managed_engine_launcher() {
    let sources = rust_sources();
    assert!(sources.len() > 30, "too few sources read");
    assert!(sources.iter().any(|source| source.path.ends_with("engine_runtime_tests.rs") && source.test_only));
    assert!(sources.iter().any(|source| source.path.ends_with("chain_client_tests.rs") && source.test_only));
    let mut constructs = Vec::new();
    for source in &sources {
        for (at, construct) in process_constructs(&source.code) {
            if !source.in_test(at) {
                let functions = rust::functions(&source.code);
                constructs.push((source.path.clone(), construct, rust::enclosing_function(&functions, at)));
            }
        }
    }
    assert_eq!(constructs, vec![
        ("src-tauri/src/engine_runtime.rs".to_owned(), "process::Command".to_owned(), None),
        ("src-tauri/src/engine_runtime.rs".to_owned(), "Command::new".to_owned(), Some("command".to_owned())),
    ], "process construction outside engine_runtime::command");
    // Test code does construct processes, so the scan sees them and sets them aside.
    let in_tests: usize = sources.iter().map(|source| process_constructs(&source.code).iter().filter(|(at, _)| source.in_test(*at)).count()).sum();
    assert!(in_tests >= 5, "the scan found no process construction in test code");

    let engine = sources.iter().find(|source| source.path == "src-tauri/src/engine_runtime.rs").unwrap();
    let functions = rust::functions(&engine.code);
    let production = |at: &usize| !engine.in_test(*at);
    // Every use of the identifier `command`: its definition, a call, or a local variable that
    // the enclosing function binds earlier (so not the launcher passed as a function value).
    let mut callers = Vec::new();
    for at in word_offsets(&engine.code, "command").into_iter().filter(production) {
        let before = engine.code[..at].trim_end();
        let after = engine.code[at + "command".len()..].trim_start();
        let function = rust::enclosing_function(&functions, at);
        if before.ends_with("fn") {
            assert!(after.starts_with('(') && function.is_none());
        } else if after.starts_with('(') {
            callers.push(function.unwrap());
        } else if !before.ends_with("let mut") {
            let (_, _, body) = functions.iter().filter(|(_, _, body)| body.contains(&at)).min_by_key(|(_, _, body)| body.len())
                .unwrap_or_else(|| panic!("engine_runtime.rs:{}: `command` outside a function", line_of(&engine.text, at)));
            let bound = engine.code[body.start..at].contains("let mut command =");
            assert!(bound, "engine_runtime.rs:{}: `command` used other than as the launcher's call or a local variable", line_of(&engine.text, at));
        }
    }
    callers.sort();
    assert_eq!(callers, ["capture_within", "launch_command"], "callers of engine_runtime::command");
    let calls = |name: &str| -> Vec<String> {
        let mut callers: Vec<String> = word_offsets(&engine.code, name)
            .into_iter()
            .filter(production)
            .filter(|at| engine.code[at + name.len()..].trim_start().starts_with('(') && !engine.code[..*at].trim_end().ends_with("fn"))
            .map(|at| rust::enclosing_function(&functions, at).unwrap())
            .collect();
        callers.sort();
        callers
    };
    assert_eq!(calls("launch_command"), ["start_observed"]);
    assert_eq!(calls("capture_within"), ["capture"]);
    assert_eq!(calls("capture"), ["output", "output"]);
    // The callers above are the only ones because these functions are private to the module:
    // no other module (or crate) can call them, which the compiler enforces.
    for name in ["command", "launch_command", "capture", "capture_within", "system_tool", "start_observed"] {
        let (_, at, _) = functions.iter().find(|(found, at, _)| found == name && production(at)).unwrap_or_else(|| panic!("fn {name}"));
        let qualifiers: Vec<&str> = engine.code[..*at].rsplit(['\n', ';', '}', '{']).next().unwrap().split_whitespace().collect();
        assert!(qualifiers.iter().all(|word| ["async", "unsafe", "const"].contains(word)), "engine_runtime.rs: fn {name} is visible outside the module: {qualifiers:?}");
    }
    // No dependency of the native crate is a library that runs programs (a process builder, a
    // shell, a URL or file opener, or POSIX process bindings), so `Command::new` is the only way.
    let mut section = String::new();
    let mut direct = Vec::new();
    for line in read("src-tauri/Cargo.toml").lines().map(str::trim) {
        if line.starts_with('[') {
            section = line.trim_matches(['[', ']']).to_owned();
        } else if section.ends_with("dependencies") && !section.starts_with("dev-") && !line.is_empty() && !line.starts_with('#') {
            direct.push(line.split(['=', ' ']).next().unwrap().trim_matches('"').to_owned());
        }
    }
    assert!(direct.len() >= 20 && direct.iter().any(|name| name == "tokio"), "{direct:?}");
    let runners = ["duct", "subprocess", "xshell", "cmd_lib", "execute", "run_script", "process-wrap", "command-group", "shared_child", "async-process",
        "async-std", "smol", "open", "opener", "webbrowser", "nix", "rustix", "daemonize", "fork", "sudo", "runas", "elevated-command", "auto-launch", "tauri-plugin-shell"];
    let found: Vec<&String> = direct.iter().filter(|name| runners.contains(&name.as_str())).collect();
    assert!(found.is_empty(), "a dependency that runs programs: {found:?}");
    assert_eq!(functions.iter().filter(|(name, at, _)| name == "output" && production(at)).count(), 1);
    // The observers: the verified engine with `--list-devices`, and fixed platform tools.
    let kept = rust::without_comments(&engine.text);
    let mut tools = BTreeSet::new();
    for (at, _) in kept.match_indices("Observer::Tool(").filter(|(at, _)| production(at)) {
        let argument = &kept[at + "Observer::Tool(".len()..];
        let argument = &argument[..argument.find(')').unwrap()];
        if argument != "name" && argument != "&'static str" {
            tools.insert(argument.trim_matches('"').to_owned());
        }
    }
    assert_eq!(tools, BTreeSet::from(["ioreg", "nvidia-smi", "sw_vers", "sysctl", "vm_stat", "vulkaninfo"].map(str::to_owned)));
    // The engine observer: matched once in `output`, called once, with `--list-devices`.
    let engine_observations: Vec<String> = kept.match_indices("Observer::Engine(").filter(|(at, _)| production(at))
        .map(|(at, matched)| { let rest = &kept[at + matched.len()..]; let close = rest.find(')').unwrap(); rest[..close + 1 + rest[close + 1..].find([')', '=']).unwrap()].to_owned() }).collect();
    assert_eq!(engine_observations, ["executable) ", r#"executable), &["--list-devices"]"#]);
    let (_, _, tool_body) = functions.iter().find(|(name, at, _)| name == "system_tool" && production(at)).unwrap();
    let mut roots: Vec<String> = rust::lex(&engine.text[tool_body.clone()]).into_iter()
        .filter(|(kind, _)| *kind == rust::Kind::Literal)
        .map(|(_, range)| engine.text[tool_body.start + range.start..tool_body.start + range.end].to_owned())
        .filter(|literal| literal.contains('/') || literal.contains('\\')).collect();
    roots.sort();
    assert_eq!(roots, ["/bin", "/sbin", "/usr/bin", "/usr/sbin", r"C:\\Program Files\\NVIDIA Corporation\\NVSMI"]);

    // Only the provider daemon reaches a managed engine.
    let mut starters = Vec::new();
    for source in &sources {
        for name in ["ManagedEngine::start", "ProviderRuntime::start"] {
            for (at, _) in source.code.match_indices(name).filter(|(at, _)| !source.in_test(*at)) {
                if source.code[at + name.len()..].starts_with('(') {
                    starters.push((source.path.clone(), name));
                }
            }
        }
    }
    starters.sort();
    assert_eq!(starters, [("src-tauri/src/app.rs".to_owned(), "ManagedEngine::start"), ("src-tauri/src/bin/chiral_daemon.rs".to_owned(), "ProviderRuntime::start")]);
    for path in ["src-tauri/src/consumer.rs", "src-tauri/src/desktop.rs", "src-tauri/src/bin/chiral.rs", "src-tauri/src/main.rs"] {
        let source = sources.iter().find(|source| source.path == path).unwrap();
        for name in ["engine_runtime", "ManagedEngine", "ProviderRuntime", "app::"] {
            assert!(source.code.match_indices(name).all(|(at, _)| source.in_test(at)), "{path} reaches {name}");
        }
    }
}

/// Node and miner executables, matched in whole files: names that are distinctive anywhere,
/// `geth` and `ethash` as a word or a word start (`gethService`; not `getHash`), and the
/// lower-case `conflux` executable as a word (the network's name is written `Conflux`).
fn node_or_miner_names(text: &str) -> Vec<String> {
    let lower = text.to_ascii_lowercase();
    let mut found = Vec::new();
    for name in ["core-geth", "go-ethereum", "ethminer", "xmrig", "lolminer", "nbminer", "gminer", "teamredminer", "phoenixminer", "bzminer",
        "nanominer", "srbminer", "cgminer", "bfgminer", "cpuminer", "nheqminer", "cfxminer", "minerd", "openethereum", "nethermind", "erigon",
        "hyperledger-besu", "bitcoind", "monerod", "gpu-miner", "gpu_miner", "conflux-rust"] {
        if lower.contains(name) {
            found.push(name.to_owned());
        }
    }
    let bytes = text.as_bytes();
    for spelling in ["geth", "Geth", "GETH", "ethash", "Ethash", "ETHASH", "conflux"] {
        for (at, _) in text.match_indices(spelling) {
            let before = at.checked_sub(1).map(|at| bytes[at]);
            let after = bytes.get(at + spelling.len()).copied();
            let word_start = before.is_none_or(|byte| !byte.is_ascii_alphanumeric());
            let word_end = after.is_none_or(|byte| !byte.is_ascii_lowercase() && !byte.is_ascii_digit() && (spelling != "conflux" || !byte.is_ascii_uppercase()));
            if word_start && word_end {
                found.push(spelling.to_owned());
            }
        }
    }
    found
}

#[test]
fn the_node_and_miner_scan_finds_executables_and_spares_ordinary_words() {
    for text in ["Command::new(\"geth\")", "gethService.start()", "/usr/local/bin/geth --mine", "Core-Geth", "bin/ethminer.exe", "conflux --config node.toml", "spawn(\"conflux\")", "xmrig"] {
        assert!(!node_or_miner_names(text).is_empty(), "{text}");
    }
    for text in ["together", "getHash(); getHTTP()", "Conflux eSpace", "https://evmtestnet.confluxrpc.com", "signature parity", "Mining pools normally pay Core Space addresses"] {
        assert!(node_or_miner_names(text).is_empty(), "{text}: {:?}", node_or_miner_names(text));
    }
}

/// Dependency names that bundle or run a chain node or a miner.
fn node_or_miner_package(name: &str) -> bool {
    let tokens: Vec<String> = name.to_ascii_lowercase().split(|c: char| !c.is_ascii_alphanumeric()).filter(|t| !t.is_empty()).map(str::to_owned).collect();
    let forbidden = ["geth", "ethminer", "xmrig", "ethash", "miner", "mining", "ganache", "hardhat", "anvil", "foundry", "openethereum", "erigon", "besu",
        "nethermind", "reth", "bitcoind", "conflux", "cfxcore", "lighthouse", "prysm"];
    tokens.iter().any(|token| forbidden.contains(&token.as_str())) || !node_or_miner_names(name).is_empty()
}

/// Package names of `Cargo.toml` dependency tables, `Cargo.lock`, `package.json` and
/// `package-lock.json`.
fn dependency_names() -> BTreeMap<&'static str, BTreeSet<String>> {
    let mut names = BTreeMap::new();
    let mut cargo = BTreeSet::new();
    let mut section = String::new();
    for line in read("src-tauri/Cargo.toml").lines() {
        let line = line.trim();
        if line.starts_with('[') {
            section = line.trim_matches(['[', ']']).to_owned();
        } else if section.ends_with("dependencies") && !line.is_empty() && !line.starts_with('#') {
            cargo.insert(line.split(['=', ' ']).next().unwrap().trim_matches('"').to_owned());
        }
    }
    names.insert("Cargo.toml", cargo);
    names.insert("Cargo.lock", read("src-tauri/Cargo.lock").lines().filter_map(|line| line.strip_prefix("name = \"")).map(|name| name.trim_end_matches('"').to_owned()).collect());
    let package: Value = serde_json::from_str(&read("package.json")).unwrap();
    names.insert("package.json", ["dependencies", "devDependencies", "optionalDependencies", "peerDependencies"].iter()
        .flat_map(|table| package[table].as_object().into_iter().flat_map(|deps| deps.keys().cloned())).collect());
    let lock: Value = serde_json::from_str(&read("package-lock.json")).unwrap();
    names.insert("package-lock.json", lock["packages"].as_object().unwrap().keys().filter(|key| !key.is_empty())
        .map(|key| key.rsplit("node_modules/").next().unwrap().to_owned()).collect());
    names
}

/// Task 3.1 (requirement 1): no source of the library, binaries, build script, frontend or
/// scripts names a node or miner executable, and no dependency manifest or lockfile has a crate
/// or package that bundles a node or a miner. Would catch, for example, a `geth` launch, a
/// bundled `ethminer` or a `ganache` development node.
#[test]
fn no_source_names_a_node_or_miner_and_no_dependency_bundles_one() {
    let root = repository();
    let mut files = files_under(&root.join("src-tauri/src"), &[]);
    files.push(root.join("src-tauri/build.rs"));
    files.extend(files_under(&root.join("src"), &[]));
    files.extend(files_under(&root.join("scripts"), &["__pycache__"]));
    assert!(files.len() > 60, "too few files read");
    let mut found = Vec::new();
    for path in &files {
        let text = String::from_utf8_lossy(&std::fs::read(path).unwrap()).into_owned();
        for name in node_or_miner_names(&text) {
            found.push(format!("{}: {name}", relative(path)));
        }
    }
    assert!(found.is_empty(), "node or miner executables named: {found:?}");
    let dependencies = dependency_names();
    for (manifest, minimum, present) in [("Cargo.toml", 20, "tauri"), ("Cargo.lock", 200, "libp2p"), ("package.json", 10, "svelte"), ("package-lock.json", 100, "vite")] {
        assert!(dependencies[manifest].len() >= minimum && dependencies[manifest].contains(present), "{manifest} was not read");
    }
    let bundled: Vec<_> = dependencies.iter().flat_map(|(manifest, names)| names.iter().filter(|name| node_or_miner_package(name)).map(move |name| format!("{manifest}: {name}"))).collect();
    assert!(bundled.is_empty(), "{bundled:?}");
    for name in ["ganache", "hardhat", "@nomicfoundation/hardhat-network", "go-ethereum", "ethminer-sys", "cfxcore", "solana-test-validator-miner"] {
        assert!(node_or_miner_package(name), "{name}");
    }
}

/// Words of operation, argument and option names that would start, stop or configure a node
/// or mining, or set a mining address.
fn node_or_mining_control(name: &str) -> bool {
    let mut words = Vec::new();
    let mut word = String::new();
    for c in name.chars() {
        if !c.is_ascii_alphanumeric() || (c.is_ascii_uppercase() && !word.is_empty() && !word.chars().last().unwrap().is_ascii_uppercase()) {
            if !word.is_empty() {
                words.push(std::mem::take(&mut word).to_ascii_lowercase());
            }
        }
        if c.is_ascii_alphanumeric() {
            word.push(c);
        }
    }
    if !word.is_empty() {
        words.push(word.to_ascii_lowercase());
    }
    let forbidden = ["mine", "miner", "miners", "mining", "node", "nodes", "geth", "coinbase", "etherbase", "hashrate", "ethash", "pool", "pools", "gpu", "threads"];
    words.iter().any(|word| forbidden.contains(&word.as_str())) || !node_or_miner_names(name).is_empty()
}

/// The literal contents of the string literals in `range` of `source`.
fn literals(source: &str, range: std::ops::Range<usize>) -> Vec<String> {
    rust::lex(&source[range.clone()]).into_iter().filter(|(kind, _)| *kind == rust::Kind::Literal)
        .map(|(_, literal)| source[range.start + literal.start..range.start + literal.end].to_owned()).collect()
}

fn function_body(source: &str, name: &str) -> std::ops::Range<usize> {
    let code = rust::code(source);
    rust::functions(&code).into_iter().find(|(found, _, _)| found == name).unwrap_or_else(|| panic!("fn {name}")).2
}

/// The body of the desktop's operation dispatch. The `consumer_command` Tauri command is
/// exactly one call of `consumer_operation`, so that a desktop test can drive it without a Tauri
/// application. `consumer_operation` is exactly one call of `dispatch`, which holds the
/// operations, followed by the webview presentation of its result. Pinning both delegations
/// keeps every operation the command reaches inside the returned body.
fn desktop_dispatch(desktop: &str) -> std::ops::Range<usize> {
    let compact_body = |name: &str| -> String {
        rust::without_comments(&desktop[function_body(desktop, name)])
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect()
    };
    assert_eq!(compact_body("consumer_command"), "{consumer_operation(&state,operation,args).await}", "consumer_command only delegates");
    assert_eq!(
        compact_body("consumer_operation"),
        "{letresult=dispatch(state,operation.clone(),args).await?;Ok(presented(&operation,result))}",
        "consumer_operation only delegates to dispatch and presents its result"
    );
    function_body(desktop, "dispatch")
}

/// The commands of the build script's app manifest: the literals of its `.commands(&[...])`.
fn app_manifest(build: &str) -> Vec<String> {
    let code = rust::code(build);
    let at = code.find(".commands(&[").expect("app manifest commands");
    let open = at + ".commands(&".len();
    literals(build, open..rust::matching(&code, open))
}

/// `--help` of `binary` with `path` subcommands: its subcommand names and its option names.
fn help(binary: &str, path: &[String]) -> (Vec<String>, Vec<String>) {
    let output = std::process::Command::new(binary)
        .env_remove("CHIRAL_CONSUMER_CONFIG")
        .env_remove("CHIRAL_PROVIDER_CONFIG")
        .args(path)
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success(), "{binary} {path:?} --help: {}", String::from_utf8_lossy(&output.stderr));
    let text = String::from_utf8(output.stdout).unwrap();
    let mut subcommands = Vec::new();
    let mut options = Vec::new();
    let mut section = "";
    for line in text.lines() {
        if !line.starts_with(' ') && line.ends_with(':') {
            section = line;
            continue;
        }
        let trimmed = line.trim_start();
        if section == "Commands:" && line.starts_with("  ") && !trimmed.is_empty() {
            let name = trimmed.split_whitespace().next().unwrap();
            if name != "help" {
                subcommands.push(name.to_owned());
            }
        }
        if section == "Options:" || section == "Arguments:" {
            for token in trimmed.split([' ', ',']).filter(|token| token.starts_with("--") || token.starts_with('<')) {
                options.push(token.trim_matches(['<', '>', '[', ']']).to_owned());
            }
        }
    }
    (subcommands, options)
}

/// Task 3.1 (requirement 1, scenario "No surface controls mining or a node"): the desktop's
/// native commands and operations (the build script's app manifest, `generate_handler!` and
/// every operation and argument name of `consumer_command`, which only delegates through
/// `consumer_operation` to `dispatch`, whose default refuses anything else), the GUI's native calls, every `chiral` subcommand and option (`--help`, recursively),
/// the options of the other binaries and the provider operator actions (`provider-admin.py`,
/// the CLI's administration actions and the daemon's administration routes) are enumerated;
/// none starts, stops or configures a node or mining, or sets a mining or coinbase address.
#[test]
fn no_native_operation_command_or_operator_action_controls_a_node_or_mining() {
    let desktop = read("src-tauri/src/desktop.rs");
    let build = read("src-tauri/build.rs");
    let manifest = app_manifest(&build);
    assert_eq!(manifest, ["consumer_command", "inference_stream", "inference_cancel", "get_version_status"]);
    let code = rust::code(&desktop);
    let handler = code.find("generate_handler![").unwrap();
    let handled: Vec<String> = code[handler + "generate_handler![".len()..rust::matching(&code, handler + "generate_handler!".len()) - 1]
        .split(',').map(|name| name.trim().to_owned()).filter(|name| !name.is_empty()).collect();
    assert_eq!(handled, manifest);
    let dispatch = literals(&desktop, desktop_dispatch(&desktop));
    // Every literal that could be an operation or argument name: any spelling without spaces
    // (snake, camel or kebab case), so that no case convention hides one.
    let name_like = |literal: &&str| !literal.is_empty() && literal.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.' || b == b':');
    let names: BTreeSet<&str> = dispatch.iter().map(String::as_str).filter(name_like).collect();
    for expected in ["configure", "logout", "wallet_create", "wallet_import", "wallet_export", "balance", "network_status", "discover", "fund", "provider_admin", "infer", "path", "contract_id"] {
        assert!(names.contains(expected), "{expected} not enumerated from consumer_command");
    }
    assert!(desktop.contains(r#"_ => Err("unsupported desktop operation".into())"#), "the dispatch must refuse unknown operations");
    let stream = literals(&desktop, function_body(&desktop, "inference_stream"));
    let mut surface: Vec<String> = names.iter().map(|name| format!("desktop operation {name}")).collect();
    surface.extend(manifest.iter().map(|name| format!("desktop command {name}")));
    surface.extend(stream.iter().map(String::as_str).filter(name_like).map(|name| format!("stream {name}")));

    // The GUI reaches native code only through these commands; it names no mining control.
    for path in files_under(&repository().join("src"), &[]) {
        let text = std::fs::read_to_string(&path).unwrap();
        let lower = text.to_ascii_lowercase().replace(['_', '-'], "");
        for control in ["startmining", "stopmining", "miningaddress", "mineraddress", "coinbase", "etherbase", "hashrate", "autostartmining", "gethservice", "startnode", "stopnode", "nodestart", "nodestop", "minerstart", "minerstop", "setmining"] {
            assert!(!lower.contains(control), "{}: {control}", relative(&path));
        }
    }

    // Every chiral subcommand, recursively, with its options.
    let chiral = env!("CARGO_BIN_EXE_chiral");
    let mut pending = vec![Vec::<String>::new()];
    let mut commands = 0;
    while let Some(path) = pending.pop() {
        let (subcommands, options) = help(chiral, &path);
        commands += 1;
        surface.extend(options.iter().map(|option| format!("chiral {} {option}", path.join(" "))));
        for subcommand in subcommands {
            surface.push(format!("chiral {} {subcommand}", path.join(" ")));
            let mut next = path.clone();
            next.push(subcommand);
            pending.push(next);
        }
    }
    assert!(commands >= 30, "only {commands} chiral commands enumerated");
    for binary in [env!("CARGO_BIN_EXE_chiral_daemon"), env!("CARGO_BIN_EXE_relay_server"), env!("CARGO_BIN_EXE_chiral_rpc_gateway"), env!("CARGO_BIN_EXE_chiral-policy-sign")] {
        let (subcommands, options) = help(binary, &[]);
        assert!(!options.is_empty());
        surface.extend(subcommands.iter().chain(&options).map(|name| format!("{binary} {name}")));
    }

    // Provider operator actions: the script, the CLI and desktop's administration actions, and
    // the daemon's administration routes.
    let script = read("scripts/provider-admin.py");
    let choices_at = script.find("choices=[").unwrap();
    let choices: Vec<String> = script[choices_at + "choices=[".len()..].split(']').next().unwrap().split(',').map(|choice| choice.trim().trim_matches('\'').to_owned()).collect();
    assert!(choices.len() >= 9, "{choices:?}");
    let consumer = read("src-tauri/src/consumer.rs");
    let actions: Vec<String> = literals(&consumer, function_body(&consumer, "provider_admin")).into_iter().filter(|literal| name_like(&literal.as_str())).collect();
    let app = read("src-tauri/src/app.rs");
    let routes: Vec<String> = literals(&app, function_body(&app, "admin_router")).into_iter().filter_map(|route| route.strip_prefix("/admin/").map(str::to_owned)).collect();
    assert_eq!(routes.len(), 10, "{routes:?}");
    for choice in &choices {
        assert!(routes.contains(choice), "provider-admin.py action {choice} has no route");
    }
    surface.extend(choices.iter().map(|choice| format!("provider-admin.py {choice}")));
    surface.extend(actions.iter().map(|action| format!("CLI provider action {action}")));
    surface.extend(routes.iter().map(|route| format!("daemon route /admin/{route}")));

    assert!(surface.len() > 150, "{} names enumerated", surface.len());
    let controls: Vec<&String> = surface.iter().filter(|name| node_or_mining_control(name.rsplit(' ').next().unwrap())).collect();
    assert!(controls.is_empty(), "node or mining controls: {controls:?}");
    for control in ["start_mining", "miner_setEtherbase", "setMiningAddress", "--mine", "node_start", "gpu-threads", "coinbase", "stopGeth"] {
        assert!(node_or_mining_control(control), "{control}");
        // Each spelling would be enumerated from the dispatch, whatever its case convention.
        assert!(name_like(&control), "{control} would not be enumerated");
    }
}

/// Task 3.2 (requirement 2): the desktop enables no plugin and grants no autostart, shell or
/// process permission, and exposes exactly its four commands. `tauri.conf.json` has no
/// `plugins` member at any depth; no manifest or lockfile has a Tauri plugin (in particular the
/// autostart and shell plugins); `src-tauri/capabilities/` holds only `default.json`, whose
/// permissions are exactly the four commands, event subscription and minimize and destroy.
#[test]
fn the_desktop_enables_no_plugin_and_no_autostart_shell_or_process_permission() {
    fn members(value: &Value, found: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                for (key, child) in map {
                    found.push(key.clone());
                    members(child, found);
                }
            }
            Value::Array(items) => items.iter().for_each(|item| members(item, found)),
            _ => {}
        }
    }
    let configuration: Value = serde_json::from_str(&read("src-tauri/tauri.conf.json")).unwrap();
    let mut keys = Vec::new();
    members(&configuration, &mut keys);
    assert!(keys.contains(&"bundle".to_owned()) && keys.contains(&"security".to_owned()), "tauri.conf.json was not read");
    assert!(!keys.iter().any(|key| key.eq_ignore_ascii_case("plugins")), "tauri.conf.json configures plugins");
    let dependencies = dependency_names();
    for (manifest, names) in &dependencies {
        let plugins: Vec<_> = names.iter().filter(|name| name.starts_with("tauri-plugin") || name.starts_with("@tauri-apps/plugin") || name.contains("autostart") || name.contains("auto-launch") || name.contains("smappservice")).collect();
        assert!(plugins.is_empty(), "{manifest}: {plugins:?}");
    }
    assert!(dependencies["package.json"].contains("@tauri-apps/api") && dependencies["Cargo.toml"].contains("tauri"));
    let capabilities: Vec<String> = std::fs::read_dir(repository().join("src-tauri/capabilities")).unwrap().map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(capabilities, ["default.json"]);
    let capability: Value = serde_json::from_str(&read("src-tauri/capabilities/default.json")).unwrap();
    let permissions: Vec<&str> = capability["permissions"].as_array().unwrap().iter().map(|permission| permission.as_str().unwrap()).collect();
    assert_eq!(permissions, ["allow-consumer-command", "allow-inference-stream", "allow-inference-cancel", "allow-get-version-status",
        "core:event:allow-listen", "core:event:allow-unlisten", "core:window:allow-minimize", "core:window:allow-destroy"]);
    for permission in &permissions {
        for forbidden in ["autostart:", "shell:", "process:", "fs:", "core:app:", "core:path:"] {
            assert!(!permission.starts_with(forbidden), "{permission}");
        }
    }
    let build = read("src-tauri/build.rs");
    let manifest = app_manifest(&build);
    assert_eq!(manifest, ["consumer_command", "inference_stream", "inference_cancel", "get_version_status"]);
    // The four allow-* permissions are those of exactly these commands.
    let granted: Vec<String> = permissions.iter().filter_map(|permission| permission.strip_prefix("allow-")).map(|name| name.replace('-', "_")).collect();
    assert_eq!(granted, manifest);
}

/// Start-at-login, service and scheduler mechanisms, matched case-insensitively in whole files.
const LOGIN_MECHANISMS: [&str; 40] = [
    "launchagents", "launchdaemons", "launchctl", "smappservice", "smloginitem", "lssharedfilelist", "runatload", "loginitem", "login item",
    "login_item", "sfltool", "currentversion\\run", "currentversion\\\\run", "currentversion/run", "runonce", "\\startup", "/startup/",
    "shell:startup", "programs\\startup", "schtasks", "scheduledtask", "register-scheduledjob", "sc.exe", "new-service", "nssm", "autostart",
    "auto-launch", "auto_launch", "systemctl", "/etc/systemd", "systemd/user", "wantedby=", "enable-linger", "crontab", "/etc/cron", "@reboot",
    "/etc/init.d", "update-rc.d", "rc.local", "chkconfig",
];

fn login_mechanisms(text: &str) -> Vec<&'static str> {
    let lower = text.to_ascii_lowercase();
    let mut found: Vec<&'static str> = LOGIN_MECHANISMS.iter().copied().filter(|pattern| lower.contains(pattern)).collect();
    // Unit files and launch agent property lists, by their file name extensions in a path.
    for extension in [".service", ".timer", ".plist", ".desktop"] {
        for (at, _) in lower.match_indices(extension) {
            let next = lower.as_bytes().get(at + extension.len()).copied();
            if next.is_none_or(|byte| !byte.is_ascii_alphanumeric() && byte != b'_') && at > 0 && lower.as_bytes()[at - 1].is_ascii_alphanumeric() {
                found.push(extension.trim_start_matches('.'));
            }
        }
    }
    found
}

#[test]
fn the_login_mechanism_scan_finds_each_mechanism() {
    for text in ["~/Library/LaunchAgents/com.chiral.plist", "launchctl load -w", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run", "schtasks /create /sc onlogon",
        "~/.config/autostart/chiral.desktop", "systemctl --user enable chiral-node", "/etc/systemd/system/geth.service", "@tauri-apps/plugin-autostart",
        "crontab -l", "Register-ScheduledTask", "SMAppService.mainApp.register()", "sc.exe create chiral", "WantedBy=default.target",
        "AppData\\Roaming\\Microsoft\\Windows\\Start Menu\\Programs\\Startup\\Chiral.lnk"] {
        assert!(!login_mechanisms(text).is_empty(), "{text}");
    }
    for text in ["restart: unless-stopped", "Service unavailable", "services:", "the provider service", "startup_timeout_secs", "termination requested during startup"] {
        assert!(login_mechanisms(text).is_empty(), "{text}: {:?}", login_mechanisms(text));
    }
}

/// Task 3.2 (requirement 2, scenario "A legacy service left on the host is not started"):
/// nothing under `src-tauri` (sources, tests, build script, configuration, capabilities,
/// permissions and generated schemas; not the build output or icons), `src`, `scripts`, the
/// `Dockerfile` or `docker-compose.yml` writes a launch agent or login item, a Windows `Run`
/// value, a Startup-folder shortcut or scheduled task, an XDG autostart entry, a systemd unit
/// or a cron entry, or runs a service manager (`launchctl`, `systemctl`, `sc.exe`, `schtasks`),
/// so nothing can start, enable or schedule a service, legacy or not.
#[test]
fn nothing_writes_a_login_entry_or_runs_a_service_manager() {
    let root = repository();
    let this = Path::new(file!()).file_name().unwrap();
    let mut files: Vec<PathBuf> = files_under(&root.join("src-tauri"), &["target", "icons"]).into_iter().filter(|path| path.file_name() != Some(this)).collect();
    files.extend(files_under(&root.join("src"), &[]));
    files.extend(files_under(&root.join("scripts"), &["__pycache__"]));
    files.push(root.join("Dockerfile"));
    files.push(root.join("docker-compose.yml"));
    files.push(root.join("package.json"));
    assert!(files.len() > 100, "too few files read");
    assert!(files.iter().any(|path| path.ends_with("tests/daemon_engine_process_test.rs")) && files.iter().any(|path| path.ends_with("capabilities/default.json")));
    let mut found = Vec::new();
    for path in &files {
        let text = String::from_utf8_lossy(&std::fs::read(path).unwrap()).into_owned();
        for mechanism in login_mechanisms(&text) {
            found.push(format!("{}: {mechanism}", relative(path)));
        }
    }
    assert!(found.is_empty(), "start-at-login or service mechanisms: {found:?}");
    // No packaging file that would install one, anywhere in the repository's own trees.
    for directory in ["src-tauri", "src", "scripts", "public", "dev", "examples", "verified"] {
        let path = root.join(directory);
        if !path.exists() {
            continue;
        }
        for file in files_under(&path, &["target", "node_modules", "__pycache__"]) {
            let name = file.file_name().unwrap().to_string_lossy().to_ascii_lowercase();
            for extension in [".plist", ".service", ".timer", ".desktop", ".wxs", ".nsi", ".iss"] {
                assert!(!name.ends_with(extension), "{}", relative(&file));
            }
        }
    }
}

/// Task 3.2 (requirement 2): the desktop's start is inert. `main` only calls the library's
/// `run`, which only calls `desktop::run`; that builds the application with managed state, a
/// `setup` closure that only installs the tray, the four command handlers and nothing else (no
/// plugin, no task, no thread), and the tray it installs only builds a menu.
#[test]
fn the_desktop_start_only_installs_the_tray() {
    let compact = |text: &str| -> String { rust::code(text).chars().filter(|c| !c.is_whitespace()).collect() };
    let main = read("src-tauri/src/main.rs");
    assert_eq!(compact(&main[function_body(&main, "main")]), "{chiral_network::run()}");
    let lib = read("src-tauri/src/lib.rs");
    assert_eq!(compact(&lib[function_body(&lib, "run")]), "{desktop::run();}");
    let desktop = read("src-tauri/src/desktop.rs");
    let run = compact(&desktop[function_body(&desktop, "run")]);
    assert_eq!(run, "{crate::version::log_policy_key_status();tauri::Builder::default().manage(DesktopState::default()).setup(|app|{install_tray(app);Ok(())}).invoke_handler(tauri::generate_handler![consumer_command,inference_stream,inference_cancel,get_version_status]).run(tauri::generate_context!()).expect(\"\");}");
    for function in ["install_tray", "build_tray", "handle_tray_action", "show_main_window"] {
        let body = rust::code(&desktop[function_body(&desktop, function)]);
        for forbidden in ["spawn", "Command", "thread", "process", "reqwest", "Consumer", "configure", "plugin", "autostart"] {
            assert!(word_offsets(&body, forbidden).is_empty() && !body.contains(&format!("{forbidden}::")), "{function} uses {forbidden}");
        }
    }
    let version = read("src-tauri/src/version.rs");
    let status = rust::code(&version[function_body(&version, "log_policy_key_status")]);
    for forbidden in ["spawn", "Command", "reqwest", "File::create", "write"] {
        assert!(!status.contains(forbidden), "log_policy_key_status uses {forbidden}");
    }
}

/// Task 3.3 (requirement 3, scenarios through "the desktop or the CLI ... and the user then logs
/// out of the desktop") and task 3.4 (the balance "on the desktop Wallet page or with `chiral
/// wallet balance`"): the desktop's profile load, wallet operations and balance read are the
/// very `Consumer` operations that the CLI runs (and that the binary tests below run), and the
/// desktop's logout only drops the session: it cancels streams, forgets the consumer and resets
/// the effective version policy, touching no file. Would catch a desktop path that imports,
/// moves or reads legacy data, or a balance the desktop reads otherwise.
#[test]
fn the_desktop_wallet_operations_are_the_cli_consumer_operations_and_logout_touches_no_file() {
    let compact = |text: &str| -> String { rust::without_comments(text).chars().filter(|c| !c.is_whitespace()).collect() };
    let desktop = read("src-tauri/src/desktop.rs");
    let dispatch = compact(&desktop[desktop_dispatch(&desktop)]);
    for expected in [
        r#"ifoperation=="logout"{end_session(state).await;returnOk(Value::Null);}"#,
        r#"letnext=Arc::new(Consumer::load(Path::new(text(&args,"path")?))?);"#,
        r#""wallet_create"=>value(consumer.wallet_create()?),"#,
        r#""wallet_import"=>value(consumer.wallet_import_file(Path::new(text(&args,"path")?))?),"#,
        r#""wallet_export"=>{consumer.wallet_export_file(Path::new(text(&args,"path")?))?;Ok(Value::Null)}"#,
        r#""balance"=>consumer.balance().await,"#,
    ] {
        assert!(dispatch.contains(expected), "desktop dispatch lacks {expected}");
    }
    let cli = read("src-tauri/src/bin/chiral.rs");
    let run = compact(&cli[function_body(&cli, "run")]);
    for expected in [
        "letconsumer=Consumer::load(&cli.config)?;",
        r#"WalletCommand::Create=>json!({"address":consumer.wallet_create()?}),"#,
        r#"WalletCommand::Import{file}=>{json!({"address":consumer.wallet_import_file(&file)?})}"#,
        "WalletCommand::Export{file}=>{consumer.wallet_export_file(&file)?;",
        "WalletCommand::Balance=>consumer.balance().await?,",
    ] {
        assert!(run.contains(expected), "chiral run lacks {expected}");
    }
    // The whole logout: the session lock, the streams' cancellation and the in-memory policy.
    assert_eq!(compact(&desktop[function_body(&desktop, "end_session")]),
        "{letmutcurrent=state.consumer.write().await;state.cancel_all().await;*current=None;crate::version::reset_effective_policy();}");
    assert_eq!(compact(&desktop[function_body(&desktop, "cancel_all")]),
        "{for(_,cancellation)inself.streams.lock().await.drain(){cancellation.send_replace(true);}}");
    let version = read("src-tauri/src/version.rs");
    let reset = rust::code(&version[function_body(&version, "reset_effective_policy")]);
    assert_eq!(reset.chars().filter(|c| !c.is_whitespace()).collect::<String>(), "{reset_slot(effective_slot());}");
    // An in-memory slot: its write lock, no file.
    let slot = rust::code(&version[function_body(&version, "reset_slot")]);
    for forbidden in ["fs::", "File", "remove", "rename", "persist", "path"] {
        assert!(!slot.contains(forbidden), "reset_slot uses {forbidden}");
    }
}

/// Core-space requests and cross-space transfers: a `cfx_` or `pos_` RPC method, the Core
/// `CrossSpaceCall` internal contract (`0x0888…0006`) or its functions, and Core addresses.
fn core_space_calls(text: &str) -> Vec<String> {
    let lower = text.to_ascii_lowercase();
    let mut found = Vec::new();
    for pattern in ["cfx_", "\"pos_", "0888000000000000000000000000000000000006", "crossspacecall", "transferevm", "withdrawfrommapped", "callevm", "staticcallevm", "cfx:aa", "cfxtest:aa", "cfxtest:", "net1029:"] {
        if lower.contains(pattern) {
            found.push(pattern.to_owned());
        }
    }
    found
}

/// Task 3.4 (requirement 4): `src-tauri/src`, tests included, and the GUI's sources (`src`),
/// which show the balance, have no Core-space RPC request and no cross-space call: no `cfx_` or
/// `pos_` method, no `CrossSpaceCall` address or function and no Core address. Would catch a
/// Core balance read or an in-app Core-to-eSpace transfer, natively or from a page.
#[test]
fn the_native_code_makes_no_core_space_request_or_cross_space_call() {
    let native = files_under(&repository().join("src-tauri/src"), &[]);
    let gui = files_under(&repository().join("src"), &[]);
    assert!(native.len() > 30 && gui.len() > 30, "too few files read");
    assert!(gui.iter().any(|path| path.ends_with("pages/Wallet.svelte")));
    let mut eth_methods = 0;
    let mut found = Vec::new();
    for path in native.iter().chain(&gui) {
        let text = std::fs::read_to_string(path).unwrap();
        eth_methods += text.matches("\"eth_").count();
        for call in core_space_calls(&text) {
            found.push(format!("{}: {call}", relative(path)));
        }
    }
    assert!(eth_methods > 20, "the scan read no chain request");
    assert!(found.is_empty(), "{found:?}");
    for text in [r#"call("cfx_getBalance", json!([address]))"#, "0x0888000000000000000000000000000000000006", "CrossSpaceCall.transferEVM(to)", "cfxtest:aak2rra2njvd77ezwjvx04kkds9fzagfe6d5r8e957"] {
        assert!(!core_space_calls(text).is_empty(), "{text}");
    }
}

// ---- The `chiral` binary, run beside legacy data and observed from outside (macOS) ----

/// A loopback eSpace chain RPC at `/espace/rpc` that records every request it receives, on
/// any path: chain 71 with the pinned genesis, and the `eth_getBalance` of each address from
/// `balances` (zero otherwise). It would answer a Core-space `cfx_getBalance` with a large
/// mining payout, so a client that asked could show it.
#[cfg(unix)]
struct ChainRpc {
    _runtime: tokio::runtime::Runtime,
    url: String,
    seen: std::sync::Arc<std::sync::Mutex<Vec<(String, Value)>>>,
}

#[cfg(unix)]
impl ChainRpc {
    fn new(balances: BTreeMap<String, u128>) -> Self {
        use chiral_network::network::ESPACE_TESTNET_GENESIS;
        let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorded = seen.clone();
        let balances = std::sync::Arc::new(balances);
        let app = axum::Router::new().fallback(move |uri: axum::http::Uri, body: axum::body::Bytes| {
            let (recorded, balances) = (recorded.clone(), balances.clone());
            async move {
                let request: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                recorded.lock().unwrap().push((uri.path().to_owned(), request.clone()));
                let params = &request["params"];
                let result = match request["method"].as_str().unwrap_or_default() {
                    "eth_chainId" => json!("0x47"),
                    "eth_getBlockByNumber" if params[0] == "0x0" => json!({"number": "0x0", "hash": ESPACE_TESTNET_GENESIS, "timestamp": "0x0"}),
                    "eth_getBalance" => {
                        let address = params[0].as_str().unwrap_or_default().to_ascii_lowercase();
                        json!(format!("0x{:x}", balances.get(&address).copied().unwrap_or(0)))
                    }
                    "cfx_getBalance" => json!(format!("0x{:x}", 5_000u128 * 10u128.pow(18))),
                    _ => return axum::Json(json!({"jsonrpc": "2.0", "id": request["id"], "error": {"code": -32601, "message": "method not found"}})),
                };
                axum::Json(json!({"jsonrpc": "2.0", "id": request["id"], "result": result}))
            }
        });
        let url = runtime.block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}/espace/rpc", listener.local_addr().unwrap());
            tokio::spawn(async move {
                let _ = axum::serve(listener, app).await;
            });
            url
        });
        Self { _runtime: runtime, url, seen }
    }
    /// The requests received so far, as path and JSON-RPC method.
    fn requests(&self) -> Vec<(String, String)> {
        self.seen.lock().unwrap().iter().map(|(path, request)| (path.clone(), request["method"].as_str().unwrap_or("<not JSON-RPC>").to_owned())).collect()
    }
    fn bodies(&self) -> Vec<Value> {
        self.seen.lock().unwrap().iter().map(|(_, request)| request.clone()).collect()
    }
}

/// A consumer profile for chain 71 at `rpc_url`, with the absolute `data_dir`.
#[cfg(unix)]
fn profile(path: &Path, rpc_url: &str, data_dir: &Path, deployment: &str) -> chiral_network::network::NetworkConfig {
    use chiral_network::network::{NetworkConfig, ESPACE_TESTNET_CHAIN_ID, ESPACE_TESTNET_GENESIS};
    use chiral_network::protocol::NetworkIdentity;
    let network = NetworkConfig {
        identity: NetworkIdentity { chain_id: ESPACE_TESTNET_CHAIN_ID, genesis_hash: ESPACE_TESTNET_GENESIS.into(), deployment_id: deployment.into() },
        rpc_url: rpc_url.into(),
        rpc_bearer_file: None,
        faucet_url: None,
        explorer_url: None,
        policy_url: None,
    };
    std::fs::write(path, serde_json::to_vec_pretty(&json!({"network": network, "data_dir": data_dir, "catalog_url": null})).unwrap()).unwrap();
    network
}

/// One `chiral` run: exit code, standard output, standard error and, on macOS, the fork
/// notifications of its whole life (`None` where the process is not observed).
#[cfg(unix)]
struct Run {
    code: i32,
    stdout: String,
    stderr: String,
    forks: Option<usize>,
}

#[cfg(unix)]
impl Run {
    fn json(&self) -> Value {
        assert_eq!(self.code, 0, "chiral failed: {}", self.stderr);
        serde_json::from_str(&self.stdout).unwrap_or_else(|_| panic!("stdout is not JSON: {}", self.stdout))
    }
    /// Where the run was observed (macOS), it created no process.
    fn created_no_process(&self, what: &str) {
        assert!(self.forks.is_none_or(|forks| forks == 0), "{what} created a process");
    }
}

/// Proxy variables, removed so that the loopback chain RPC is contacted directly.
#[cfg(unix)]
const PROXY_VARIABLES: [&str; 8] = ["HTTP_PROXY", "http_proxy", "HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy", "NO_PROXY", "no_proxy"];

/// Runs `chiral --config <config> <args>` with `home` as its home directory (so the operating
/// system's data directory of the earlier application is the one under it), `path_first` first
/// on `PATH`, no consumer configuration variable, no XDG directory variable and no proxy
/// variable; waits at most 60 seconds. On macOS it is started suspended and observed from
/// before its first instruction (`process_observer`).
#[cfg(unix)]
fn chiral(root: &Path, config: &Path, home: &Path, path_first: &Path, args: &[&str]) -> Run {
    #[cfg(target_os = "macos")]
    {
        use std::ffi::OsStr;
        let path = format!("{}:{}", path_first.display(), std::env::var("PATH").unwrap_or_default());
        let mut remove = vec!["CHIRAL_CONSUMER_CONFIG", "XDG_DATA_HOME", "XDG_CONFIG_HOME"];
        remove.extend(PROXY_VARIABLES);
        let environment = process_observer::environment(&remove, &[("HOME", home.as_os_str()), ("PATH", OsStr::new(&path))]);
        let mut arguments: Vec<&OsStr> = vec![OsStr::new("--config"), config.as_os_str()];
        arguments.extend(args.iter().map(OsStr::new));
        let (stdout, stderr) = (root.join("chiral.stdout"), root.join("chiral.stderr"));
        let mut process = process_observer::Observed::spawn(Path::new(env!("CARGO_BIN_EXE_chiral")), &arguments, &environment, &stdout, &stderr);
        let code = process.wait_timeout(std::time::Duration::from_secs(60)).unwrap_or_else(|| panic!("chiral {args:?} did not exit"));
        let forks = process.forks();
        Run { code, stdout: std::fs::read_to_string(&stdout).unwrap(), stderr: std::fs::read_to_string(&stderr).unwrap(), forks: Some(forks) }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = root;
        chiral_unobserved(config, home, path_first, args)
    }
}

/// `chiral` with the same environment, not observed: the run where no observer exists.
#[cfg(unix)]
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn chiral_unobserved(config: &Path, home: &Path, path_first: &Path, args: &[&str]) -> Run {
    let path = format!("{}:{}", path_first.display(), std::env::var("PATH").unwrap_or_default());
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_chiral"));
    for name in ["CHIRAL_CONSUMER_CONFIG", "XDG_DATA_HOME", "XDG_CONFIG_HOME"].iter().chain(PROXY_VARIABLES.iter()) {
        command.env_remove(name);
    }
    let output = command.env("HOME", home).env("PATH", path).arg("--config").arg(config).args(args)
        .stdin(std::process::Stdio::null()).output().unwrap();
    Run { code: output.status.code().unwrap_or(-1), stdout: String::from_utf8(output.stdout).unwrap(), stderr: String::from_utf8(output.stderr).unwrap(), forks: None }
}

/// Path, permission bits and SHA-256 of every file under `root`, except those under the
/// directories in `skip`, and every directory.
#[cfg(unix)]
fn digests(root: &Path, skip: &[&Path]) -> BTreeMap<PathBuf, (u32, String)> {
    use std::os::unix::fs::PermissionsExt;
    let mut found = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).unwrap() {
            let path = entry.unwrap().path();
            if skip.iter().any(|skipped| path.starts_with(skipped)) {
                continue;
            }
            let metadata = std::fs::symlink_metadata(&path).unwrap();
            let digest = if metadata.is_dir() {
                pending.push(path.clone());
                "directory".to_owned()
            } else {
                hex::encode(Sha256::digest(std::fs::read(&path).unwrap()))
            };
            found.insert(path, (metadata.permissions().mode(), digest));
        }
    }
    found
}

/// A legacy key that is a valid secp256k1 private key, as the earlier application stored it.
#[cfg(unix)]
const LEGACY_KEY: &str = "4c0883a69102937d6231471b5dbb6204fe5129617082792ae468d01a3f362318";

/// Writes the earlier application's data under `base`: its bundled node and miner binaries
/// (fakes that append to `executed` whenever anything runs them), the Geth chain directory, a
/// plain and a keystore wallet, a wallet export, CHI history, file-transfer reputation, Drive,
/// hosting and download files.
#[cfg(unix)]
fn write_legacy_data(base: &Path, executed: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let recorder = format!("#!/bin/sh\nprintf '%s\\n' \"$0 $*\" >> '{}'\n", executed.display());
    let files: [(&str, Vec<u8>, u32); 18] = [
        ("bin/geth", recorder.clone().into_bytes(), 0o755),
        ("bin/ethminer", recorder.into_bytes(), 0o755),
        ("geth/geth.log", b"INFO [01-01|00:00:00] Starting Geth on Chiral network".to_vec(), 0o644),
        ("geth/geth.pid", b"4242".to_vec(), 0o644),
        ("geth/genesis.json", br#"{"config":{"chainId":98765,"ethash":{}},"difficulty":"0x400"}"#.to_vec(), 0o644),
        ("geth/geth/chaindata/000001.ldb", b"legacy chain database".to_vec(), 0o644),
        ("geth/geth/chaindata/LOCK", Vec::new(), 0o644),
        ("geth/geth/nodekey", b"a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90".to_vec(), 0o600),
        ("geth/keystore/UTC--2025-01-01T00-00-00.000000000Z--90f8bf6a479f320ead074411a4b0e7944ea8c9c1", br#"{"address":"90f8bf6a479f320ead074411a4b0e7944ea8c9c1","crypto":{"cipher":"aes-128-ctr"},"version":3}"#.to_vec(), 0o600),
        ("wallet.key", LEGACY_KEY.as_bytes().to_vec(), 0o600),
        ("consumer/wallet.key", LEGACY_KEY.as_bytes().to_vec(), 0o600),
        ("chiral-wallet.json", format!(r#"{{"privateKey":"0x{LEGACY_KEY}","balance":"42 CHI","balance_chr":"7 CHR"}}"#).into_bytes(), 0o600),
        ("transactions/chi-history.json", br#"[{"hash":"0x01","value":"42 CHI","status":"mined"}]"#.to_vec(), 0o644),
        ("chiral-drive/manifest.json", br#"{"files":[{"name":"report.pdf","size":4}]}"#.to_vec(), 0o644),
        ("chiral-drive/files/report.pdf", b"%PDF".to_vec(), 0o644),
        ("downloads/transfer.part", b"legacy file-transfer data".to_vec(), 0o644),
        ("reputation/file-transfer.json", br#"[{"peer":"12D3KooWLegacy","score":0.9,"transfers":12}]"#.to_vec(), 0o644),
        ("exports/wallet-backup-2025.json", format!(r#"{{"address":"0x90f8bf6a479f320ead074411a4b0e7944ea8c9c1","privateKey":"0x{LEGACY_KEY}"}}"#).into_bytes(), 0o600),
    ];
    for (relative, contents, mode) in files {
        let path = base.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, contents).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
    }
    std::fs::create_dir_all(base.join("hosting")).unwrap();
    std::fs::write(base.join("hosting/sites.json"), br#"[{"site":"legacy","files":1}]"#).unwrap();
}

#[cfg(unix)]
fn canonical_tempdir() -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().canonicalize().unwrap();
    (directory, path)
}

/// Task 3.2 (requirement 2, "the start of an application session"), macOS part: loading a
/// consumer profile with `chiral --config` and running `network show`, `wallet create`,
/// `wallet address` and `wallet balance` against a loopback chain RPC starts no process at all:
/// each `chiral` process, watched by kqueue from before its first instruction until it was
/// reaped, posted no fork notification. The control: the same observation of `/bin/sh -c
/// /usr/bin/true` counts its fork. Would catch a node, miner or legacy service started, or
/// any other child left behind, by a session start or a wallet command.
#[cfg(target_os = "macos")]
#[test]
fn a_cli_session_start_and_wallet_commands_create_no_process() {
    let (_guard, root) = canonical_tempdir();
    let home = root.join("home");
    std::fs::create_dir(&home).unwrap();
    let rpc = ChainRpc::new(BTreeMap::new());
    let config = root.join("consumer.json");
    profile(&config, &rpc.url, &root.join("data"), "startup-boundaries-session");
    for args in [&["network", "show"][..], &["wallet", "create"], &["wallet", "address"], &["wallet", "balance"]] {
        let run = chiral(&root, &config, &home, &root.join("no-such-directory"), args);
        run.json();
        assert_eq!(run.forks, Some(0), "chiral {args:?} created a process");
    }
    assert!(rpc.requests().iter().any(|(_, method)| method == "eth_getBalance"), "the balance was not read from the loopback RPC");
    // The control: the observation counts a child.
    let shell = process_observer::environment(&[], &[]);
    let mut control = process_observer::Observed::spawn(Path::new("/bin/sh"), &[std::ffi::OsStr::new("-c"), std::ffi::OsStr::new("/usr/bin/true; /usr/bin/true")], &shell, &root.join("sh.stdout"), &root.join("sh.stderr"));
    assert_eq!(control.wait_timeout(std::time::Duration::from_secs(30)), Some(0));
    assert!(control.forks() >= 1, "the observation missed the shell's children");
}

/// Task 3.3 (requirement 3, scenarios "Legacy directories under the base are untouched" and "A
/// legacy wallet is not loaded automatically"), through the CLI. The profile's absolute
/// `data_dir` base already holds the earlier application's data (`write_legacy_data`), and so
/// does the earlier application's operating-system data directory under the run's home
/// directory, whose `bin` directory is also first on `PATH`. With no wallet in the network's
/// namespace, `wallet address` and `wallet balance` fail and read no balance: no wallet is
/// loaded from a legacy file at the base, at its unscoped `consumer/` location or in the old
/// data directory. `network show`, `wallet create`, `wallet address`, `wallet balance` and
/// `wallet export --file` then succeed; the created wallet is new (not the legacy key), its key
/// and nonce journal are under `<data_dir>/inference/<network hash>/`, the balance shown is
/// the eSpace `eth_getBalance` of that wallet only, and nothing else was created. Every legacy
/// file and directory keeps its path, permissions and SHA-256, no file appeared in the home
/// directory, the fake node and miner binaries were never executed, and (observed on macOS)
/// no `chiral` process created a process. Would catch an automatic import, migration, deletion or rewrite of
/// legacy data, a wallet taken from a legacy file, a legacy binary run, or state written
/// outside the namespace.
#[cfg(unix)]
#[test]
fn legacy_data_under_the_base_and_the_old_data_directory_is_untouched_and_never_loaded() {
    let (_guard, root) = canonical_tempdir();
    let base = root.join("data");
    let home = root.join("home");
    // `dirs::data_dir()/chiral-network` of the earlier application (`49ed4da9:geth.rs:411`) for
    // this home directory; the runs remove `XDG_DATA_HOME`.
    let old = home.join(if cfg!(target_os = "macos") { "Library/Application Support/chiral-network" } else { ".local/share/chiral-network" });
    let executed = root.join("executed.log");
    write_legacy_data(&base, &executed);
    write_legacy_data(&old, &executed);
    let rpc = ChainRpc::new(BTreeMap::new());
    let config = root.join("consumer.json");
    let network = profile(&config, &rpc.url, &base, "startup-boundaries-legacy");
    let namespace = network.scoped_dir(&base);
    assert!(namespace.starts_with(base.join("inference")));
    let legacy_before = digests(&base, &[]);
    let home_before = digests(&home, &[]);
    assert!(legacy_before.len() >= 20 && home_before.len() >= 20);
    let legacy_address = chiral_network::wallet::address_from_private_key(LEGACY_KEY).unwrap();
    let run = |args: &[&str]| {
        let run = chiral(&root, &config, &home, &old.join("bin"), args);
        run.created_no_process(&format!("chiral {args:?}"));
        run
    };

    // No wallet in the namespace: none is loaded from the legacy files.
    for args in [&["wallet", "address"][..], &["wallet", "balance"]] {
        let refused = run(args);
        assert_ne!(refused.code, 0, "chiral {args:?} found a wallet: {}", refused.stdout);
        assert!(!refused.stdout.contains(&legacy_address) && !refused.stderr.contains(&legacy_address));
        assert!(!namespace.join("consumer/wallet.key").exists());
    }
    assert!(rpc.requests().is_empty(), "a balance was read without a wallet: {:?}", rpc.requests());

    let shown = run(&["network", "show"]).json();
    assert_eq!(shown["identity"], json!(network.identity));
    let created = run(&["wallet", "create"]).json()["address"].as_str().unwrap().to_owned();
    assert_ne!(created.to_ascii_lowercase(), legacy_address.to_ascii_lowercase(), "the created wallet is the legacy key");
    assert_eq!(run(&["wallet", "address"]).json()["address"], json!(created));
    let balance = run(&["wallet", "balance"]).json();
    assert_eq!((&balance["address"], &balance["balance_wei"], &balance["network"]), (&json!(created), &json!("0"), &json!(network.identity)));
    let export = root.join("exports/backup.key");
    std::fs::create_dir(export.parent().unwrap()).unwrap();
    assert_eq!(run(&["wallet", "export", "--file", export.to_str().unwrap()]).json(), json!({"exported": true}));
    assert_eq!(chiral_network::wallet::address_from_private_key(&chiral_network::wallet::load_private_key(&export).unwrap()).unwrap(), created);

    // Only eSpace requests, to the configured path, for the created wallet.
    let requests = rpc.requests();
    assert!(requests.iter().all(|(path, method)| path == "/espace/rpc" && method.starts_with("eth_")), "{requests:?}");
    let balances: Vec<Value> = rpc.bodies().into_iter().filter(|body| body["method"] == "eth_getBalance").map(|body| body["params"].clone()).collect();
    assert_eq!(balances, [json!([created.to_ascii_lowercase(), "latest"])]);

    // The wallet and journal are in the namespace; everything else is untouched.
    assert!(namespace.join("consumer/wallet.key").is_file());
    assert!(namespace.join("wallet-journal/transactions.sqlite3").is_file());
    assert_eq!(digests(&base, &[&base.join("inference")]), legacy_before, "legacy data under the base changed");
    let created_entries: Vec<PathBuf> = std::fs::read_dir(&base).unwrap().map(|entry| entry.unwrap().path()).filter(|path| !legacy_before.contains_key(path)).collect();
    assert_eq!(created_entries, [base.join("inference")]);
    let namespaces: Vec<PathBuf> = std::fs::read_dir(base.join("inference")).unwrap().map(|entry| entry.unwrap().path()).collect();
    assert_eq!(namespaces, [namespace]);
    assert_eq!(digests(&home, &[]), home_before, "the earlier application's data directory changed");
    assert!(!executed.exists(), "a legacy binary ran: {}", std::fs::read_to_string(&executed).unwrap_or_default());
}

/// Task 3.3 (requirement 3, scenario "An explicit import converts no balance") and task 3.4
/// (requirement 4, scenario "A Core payout is not an eSpace balance"), through the CLI. A
/// legacy private key in an owner-only file beside CHI history is imported by path with
/// `wallet import --file`: the source file keeps its bytes, permissions and modification
/// time, the key is stored owner-only in the namespace, and `wallet balance` shows exactly the
/// eSpace balance that `eth_getBalance` of that address reports on the configured network
/// (7 wei), not the 42 CHI of the history or the Core-space payout that a `cfx_getBalance`
/// would report. Every request went to the configured path and was an `eth_*` method. A second
/// wallet whose eSpace balance is 0, while the RPC would report a Core payout, is shown with
/// balance 0. Would catch a converted or credited legacy balance, a Core balance read or
/// shown as eSpace funds, or an import that rewrites or removes its source.
#[cfg(unix)]
#[test]
fn an_explicit_import_leaves_its_source_unchanged_and_shows_only_the_espace_balance() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let (_guard, root) = canonical_tempdir();
    let home = root.join("home");
    std::fs::create_dir(&home).unwrap();
    let legacy = root.join("legacy");
    write_legacy_data(&legacy, &root.join("executed.log"));
    let source = legacy.join("wallet.key");
    let legacy_address = chiral_network::wallet::address_from_private_key(LEGACY_KEY).unwrap();
    let rpc = ChainRpc::new(BTreeMap::from([(legacy_address.to_ascii_lowercase(), 7u128)]));
    let config = root.join("consumer.json");
    let base = root.join("data");
    let network = profile(&config, &rpc.url, &base, "startup-boundaries-import");
    // The modification time is in the past, so that a rewrite at any time would change it.
    let past = libc::timespec { tv_sec: 1_600_000_000, tv_nsec: 0 };
    let path = std::ffi::CString::new(source.to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::utimensat(libc::AT_FDCWD, path.as_ptr(), [past, past].as_ptr(), 0) }, 0);
    let before = (std::fs::read(&source).unwrap(), std::fs::metadata(&source).unwrap());
    let legacy_before = digests(&legacy, &[]);

    let imported = chiral(&root, &config, &home, &root.join("no-such-directory"), &["wallet", "import", "--file", source.to_str().unwrap()]);
    imported.created_no_process("wallet import");
    assert_eq!(imported.json()["address"], json!(legacy_address));
    let after = (std::fs::read(&source).unwrap(), std::fs::metadata(&source).unwrap());
    assert_eq!(after.0, before.0, "the source key file changed");
    assert_eq!((after.1.mode(), after.1.mtime(), after.1.mtime_nsec(), after.1.ino()), (before.1.mode(), before.1.mtime(), before.1.mtime_nsec(), before.1.ino()));
    assert_eq!(digests(&legacy, &[]), legacy_before);
    let stored = network.scoped_dir(&base).join("consumer/wallet.key");
    assert_eq!(std::fs::metadata(&stored).unwrap().permissions().mode() & 0o777, 0o600);

    let balance = chiral(&root, &config, &home, &root.join("no-such-directory"), &["wallet", "balance"]);
    balance.created_no_process("wallet balance");
    let balance = balance.json();
    assert_eq!((&balance["address"], &balance["balance_wei"], &balance["network"]), (&json!(legacy_address), &json!("7"), &json!(network.identity)));
    assert_eq!(balance["network"]["chain_id"], 71);
    let requests = rpc.requests();
    assert!(requests.iter().all(|(path, method)| path == "/espace/rpc" && method.starts_with("eth_")), "{requests:?}");
    assert!(!requests.iter().any(|(_, method)| method.starts_with("cfx_")));
    let reads: Vec<Value> = rpc.bodies().into_iter().filter(|body| body["method"] == "eth_getBalance").map(|body| body["params"].clone()).collect();
    assert_eq!(reads, [json!([legacy_address.to_ascii_lowercase(), "latest"])]);

    // A wallet whose eSpace balance is 0, while Core space would report a payout.
    let other_config = root.join("other.json");
    profile(&other_config, &rpc.url, &root.join("other"), "startup-boundaries-core");
    let created = chiral(&root, &other_config, &home, &root.join("no-such-directory"), &["wallet", "create"]).json()["address"].as_str().unwrap().to_owned();
    let zero = chiral(&root, &other_config, &home, &root.join("no-such-directory"), &["wallet", "balance"]).json();
    assert_eq!((&zero["address"], &zero["balance_wei"]), (&json!(created), &json!("0")));
    assert!(rpc.requests().iter().all(|(_, method)| method.starts_with("eth_")), "{:?}", rpc.requests());
}
