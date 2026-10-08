// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/search.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: File search. Name matching (contains / wildcard / regex / exact),
//           type, extension, size, date, owner and hidden filters, plus
//           optional content search. All filtering - including reading file
//           contents - runs inside jwalk's parallel read_dir callbacks, so the
//           work is spread over every CPU core. Matches are streamed to the
//           caller in batches.
// =============================================================================

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Instant, UNIX_EPOCH};

use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};
use crate::fsutil;
use crate::largefiles::{categorize, FileCategory};

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum MatchMode {
    #[default]
    Contains,
    Glob,
    Regex,
    Exact,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    #[default]
    Any,
    Files,
    Folders,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Owner {
    #[default]
    Any,
    Me,
    Others,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchOptions {
    pub root: String,
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub mode: MatchMode,
    #[serde(default)]
    pub case_sensitive: bool,
    /// Match against the full path instead of the file name.
    #[serde(default)]
    pub match_path: bool,
    #[serde(default)]
    pub kind: EntryKind,
    #[serde(default)]
    pub categories: Vec<FileCategory>,
    /// Extensions without dots, e.g. ["pdf", "docx"].
    #[serde(default)]
    pub extensions: Vec<String>,
    pub min_size: Option<u64>,
    pub max_size: Option<u64>,
    /// Unix seconds.
    pub modified_after: Option<u64>,
    pub modified_before: Option<u64>,
    #[serde(default)]
    pub owner: Owner,
    #[serde(default)]
    pub include_hidden: bool,
    #[serde(default = "yes")]
    pub skip_noise: bool,
    #[serde(default = "yes")]
    pub same_filesystem: bool,
    /// Text that must appear inside the file (text files only).
    pub content: Option<String>,
    #[serde(default = "default_max")]
    pub max_results: usize,
}

fn yes() -> bool {
    true
}
fn default_max() -> usize {
    2000
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentHit {
    pub line: u64,
    pub text: String,
    pub matches: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub size: Option<u64>,
    pub modified: Option<u64>,
    pub category: Option<FileCategory>,
    pub owner: String,
    pub content: Option<ContentHit>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchProgress {
    pub scanned: u64,
    pub matched: u64,
    pub current: String,
    pub elapsed_ms: u64,
    /// New hits since the previous progress event.
    pub batch: Vec<SearchHit>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchSummary {
    pub root: String,
    pub hits: Vec<SearchHit>,
    pub scanned: u64,
    pub truncated: bool,
    pub cancelled: bool,
    pub elapsed_ms: u64,
}

const NOISE_DIRS: [&str; 8] = [
    ".git",
    "node_modules",
    "__pycache__",
    ".venv",
    ".tox",
    ".mypy_cache",
    ".gradle",
    ".cargo",
];
const PSEUDO_ROOTS: [&str; 5] = ["/proc", "/sys", "/dev", "/run", "/snap"];
const MAX_CONTENT_FILE: u64 = 64 * 1024 * 1024;
const MAX_SNIPPET: usize = 220;

/// Convert a shell wildcard (`*.mp4`, `IMG_????.jpg`) to an anchored regex.
pub fn glob_to_regex(glob: &str) -> String {
    let mut re = String::from("^");
    for c in glob.chars() {
        match c {
            '*' => re.push_str(".*"),
            '?' => re.push('.'),
            c => re.push_str(&regex::escape(&c.to_string())),
        }
    }
    re.push('$');
    re
}

enum NameMatcher {
    Any,
    Contains { needle: String, cs: bool },
    Exact { needle: String, cs: bool },
    Regex(Regex),
}

impl NameMatcher {
    fn new(o: &SearchOptions) -> Result<Self> {
        let q = o.query.trim();
        if q.is_empty() {
            return Ok(NameMatcher::Any);
        }
        let build = |pat: &str| {
            RegexBuilder::new(pat)
                .case_insensitive(!o.case_sensitive)
                .size_limit(1 << 20)
                .build()
                .map_err(|e| CoreError::Other(format!("invalid pattern: {e}")))
        };
        let lower = |s: &str| {
            if o.case_sensitive {
                s.to_string()
            } else {
                s.to_lowercase()
            }
        };
        Ok(match o.mode {
            MatchMode::Contains => NameMatcher::Contains {
                needle: lower(q),
                cs: o.case_sensitive,
            },
            MatchMode::Exact => NameMatcher::Exact {
                needle: lower(q),
                cs: o.case_sensitive,
            },
            MatchMode::Glob => NameMatcher::Regex(build(&glob_to_regex(q))?),
            MatchMode::Regex => NameMatcher::Regex(build(q)?),
        })
    }

    fn is_match(&self, s: &str) -> bool {
        match self {
            NameMatcher::Any => true,
            NameMatcher::Contains { needle, cs } => {
                if *cs {
                    s.contains(needle.as_str())
                } else {
                    s.to_lowercase().contains(needle.as_str())
                }
            }
            NameMatcher::Exact { needle, cs } => {
                if *cs {
                    s == needle
                } else {
                    s.to_lowercase() == *needle
                }
            }
            NameMatcher::Regex(r) => r.is_match(s),
        }
    }
}

struct ContentMatcher {
    needle: String,
    cs: bool,
}

impl ContentMatcher {
    /// First matching line and the number of matching lines, or None.
    fn search(&self, path: &Path) -> Option<ContentHit> {
        let mut f = std::fs::File::open(path).ok()?;
        // Skip binary files: NUL byte in the first 8 KiB.
        let mut head = [0u8; 8192];
        let n = f.read(&mut head).ok()?;
        if head[..n].contains(&0) {
            return None;
        }
        let reader = BufReader::new(std::io::Cursor::new(head[..n].to_vec()).chain(f));
        let mut first: Option<(u64, String)> = None;
        let mut count = 0;
        let mut buf = Vec::new();
        let mut reader = reader;
        let mut line_no = 0u64;
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            line_no += 1;
            let line = String::from_utf8_lossy(&buf);
            let hay = if self.cs {
                line.to_string()
            } else {
                line.to_lowercase()
            };
            if let Some(pos) = hay.find(&self.needle) {
                count += 1;
                if first.is_none() {
                    first = Some((line_no, snippet(&line, pos)));
                }
            }
        }
        first.map(|(line, text)| ContentHit {
            line,
            text,
            matches: count,
        })
    }
}

/// A short excerpt of `line` centred on byte offset `pos`.
fn snippet(line: &str, pos: usize) -> String {
    let line = line.trim_end_matches(['\n', '\r']);
    if line.len() <= MAX_SNIPPET {
        return line.trim().to_string();
    }
    let mut start = pos.saturating_sub(MAX_SNIPPET / 3);
    while !line.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = (start + MAX_SNIPPET).min(line.len());
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}{}{}",
        if start > 0 { "…" } else { "" },
        line[start..end].trim(),
        if end < line.len() { "…" } else { "" }
    )
}

struct Filter {
    name: NameMatcher,
    match_path: bool,
    kind: EntryKind,
    categories: Vec<FileCategory>,
    extensions: Vec<String>,
    min_size: Option<u64>,
    max_size: Option<u64>,
    after: Option<u64>,
    before: Option<u64>,
    owner: Owner,
    me: u32,
    content: Option<ContentMatcher>,
    users: HashMap<u32, String>,
}

impl Filter {
    /// Full test for one entry; returns the hit when it matches.
    fn check(
        &self,
        path: &Path,
        name: &str,
        is_dir: bool,
        meta: &std::fs::Metadata,
    ) -> Option<SearchHit> {
        match self.kind {
            EntryKind::Files if is_dir => return None,
            EntryKind::Folders if !is_dir => return None,
            _ => {}
        }
        // Content search only applies to files.
        if is_dir && self.content.is_some() {
            return None;
        }
        let subject = if self.match_path {
            path.to_string_lossy()
        } else {
            name.into()
        };
        if !self.name.is_match(&subject) {
            return None;
        }
        let category = (!is_dir).then(|| categorize(path));
        if !self.categories.is_empty() && !category.is_some_and(|c| self.categories.contains(&c)) {
            return None;
        }
        if !self.extensions.is_empty() {
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_lowercase());
            if !ext.is_some_and(|e| self.extensions.contains(&e)) {
                return None;
            }
        }
        let size = (!is_dir).then_some(meta.len());
        if let (Some(min), Some(s)) = (self.min_size, size) {
            if s < min {
                return None;
            }
        }
        if let (Some(max), Some(s)) = (self.max_size, size) {
            if s > max {
                return None;
            }
        }
        if (self.min_size.is_some() || self.max_size.is_some()) && is_dir {
            return None;
        }
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs());
        if let Some(a) = self.after {
            if modified.is_none_or(|m| m < a) {
                return None;
            }
        }
        if let Some(b) = self.before {
            if modified.is_none_or(|m| m > b) {
                return None;
            }
        }
        let uid = uid_of(meta);
        match self.owner {
            Owner::Me if uid != self.me => return None,
            Owner::Others if uid == self.me => return None,
            _ => {}
        }
        let content = match &self.content {
            Some(c) => {
                if size.unwrap_or(0) > MAX_CONTENT_FILE {
                    return None;
                }
                Some(c.search(path)?)
            }
            None => None,
        };
        Some(SearchHit {
            path: path.display().to_string(),
            name: name.to_string(),
            is_dir,
            size,
            modified,
            category,
            owner: self
                .users
                .get(&uid)
                .cloned()
                .unwrap_or_else(|| uid.to_string()),
            content,
        })
    }
}

#[cfg(unix)]
fn uid_of(m: &std::fs::Metadata) -> u32 {
    std::os::unix::fs::MetadataExt::uid(m)
}
#[cfg(not(unix))]
fn uid_of(_m: &std::fs::Metadata) -> u32 {
    0
}

#[cfg(unix)]
fn current_uid() -> u32 {
    // SAFETY: getuid has no preconditions.
    unsafe { libc::getuid() }
}
#[cfg(not(unix))]
fn current_uid() -> u32 {
    0
}

/// uid -> login name from /etc/passwd.
fn user_names() -> HashMap<u32, String> {
    std::fs::read_to_string("/etc/passwd")
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split(':').collect();
            Some((f.get(2)?.parse().ok()?, f.first()?.to_string()))
        })
        .collect()
}

fn expand_home(p: &str) -> PathBuf {
    match (p.strip_prefix('~'), fsutil::home_dir()) {
        (Some(rest), Some(home)) if rest.is_empty() || rest.starts_with('/') => {
            home.join(rest.trim_start_matches('/'))
        }
        _ => PathBuf::from(p),
    }
}

/// Run a search. `on_progress` receives batches of new hits roughly every 150 ms.
pub fn search(
    opts: &SearchOptions,
    cancel: &AtomicBool,
    on_progress: impl Fn(SearchProgress),
) -> Result<SearchSummary> {
    let root = expand_home(opts.root.trim());
    let root_meta = std::fs::metadata(&root)
        .map_err(|e| CoreError::Other(format!("{}: {e}", root.display())))?;
    if !root_meta.is_dir() {
        return Err(CoreError::Other(format!(
            "{} is not a folder",
            root.display()
        )));
    }
    let content = opts
        .content
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .map(|c| ContentMatcher {
            needle: if opts.case_sensitive {
                c.to_string()
            } else {
                c.to_lowercase()
            },
            cs: opts.case_sensitive,
        });
    let filter = Arc::new(Filter {
        name: NameMatcher::new(opts)?,
        match_path: opts.match_path,
        kind: opts.kind,
        categories: opts.categories.clone(),
        extensions: opts
            .extensions
            .iter()
            .map(|e| e.trim().trim_start_matches('.').to_ascii_lowercase())
            .filter(|e| !e.is_empty())
            .collect(),
        min_size: opts.min_size,
        max_size: opts.max_size,
        after: opts.modified_after,
        before: opts.modified_before,
        owner: opts.owner,
        me: current_uid(),
        content,
        users: user_names(),
    });

    let root_dev = fsutil::device_of(&root_meta);
    let (same_fs, skip_noise) = (opts.same_filesystem, opts.skip_noise);
    let scanned = Arc::new(AtomicU64::new(0));
    let (f2, scanned2) = (filter.clone(), scanned.clone());
    // Internal stop flag shared with the worker threads (cancel, result limit).
    let stop = Arc::new(AtomicBool::new(false));
    let stop_w = stop.clone();
    let started = Instant::now();

    let walker = jwalk::WalkDirGeneric::<((), Option<SearchHit>)>::new(&root)
        .skip_hidden(!opts.include_hidden)
        .follow_links(false)
        .process_read_dir(move |_, dir, _, children| {
            if stop_w.load(Ordering::Relaxed) || PSEUDO_ROOTS.iter().any(|p| dir == Path::new(p)) {
                children.clear();
                return;
            }
            for child in children.iter_mut().flatten() {
                scanned2.fetch_add(1, Ordering::Relaxed);
                let is_dir = child.file_type().is_dir();
                let name = child.file_name().to_string_lossy().into_owned();
                if is_dir && skip_noise && NOISE_DIRS.contains(&name.as_str()) {
                    child.read_children = None;
                    continue;
                }
                let Ok(meta) = child.metadata() else { continue };
                if is_dir {
                    let p = child.path();
                    if (same_fs && fsutil::device_of(&meta) != root_dev)
                        || PSEUDO_ROOTS.iter().any(|r| p == Path::new(r))
                    {
                        child.read_children = None;
                        continue;
                    }
                } else if !child.file_type().is_file() {
                    continue;
                }
                child.client_state = f2.check(&child.path(), &name, is_dir, &meta);
            }
        });

    let mut hits: Vec<SearchHit> = Vec::new();
    let mut batch: Vec<SearchHit> = Vec::new();
    let mut last = Instant::now();
    let mut current = String::new();
    let (mut truncated, mut cancelled) = (false, false);

    for entry in walker {
        if cancel.load(Ordering::Relaxed) {
            stop.store(true, Ordering::Relaxed);
            cancelled = true;
            break;
        }
        let Ok(mut e) = entry else { continue };
        if e.depth == 0 {
            continue; // the search root itself
        }
        if e.file_type().is_dir() {
            current = e.path().display().to_string();
        }
        if let Some(hit) = e.client_state.take() {
            if hits.len() >= opts.max_results {
                truncated = true;
                stop.store(true, Ordering::Relaxed);
                break;
            }
            batch.push(hit.clone());
            hits.push(hit);
        }
        if last.elapsed().as_millis() >= 150 {
            last = Instant::now();
            on_progress(SearchProgress {
                scanned: scanned.load(Ordering::Relaxed),
                matched: hits.len() as u64,
                current: current.clone(),
                elapsed_ms: started.elapsed().as_millis() as u64,
                batch: std::mem::take(&mut batch),
            });
        }
    }
    if !batch.is_empty() {
        on_progress(SearchProgress {
            scanned: scanned.load(Ordering::Relaxed),
            matched: hits.len() as u64,
            current,
            elapsed_ms: started.elapsed().as_millis() as u64,
            batch,
        });
    }
    stop.store(true, Ordering::Relaxed);

    Ok(SearchSummary {
        root: root.display().to_string(),
        hits,
        scanned: scanned.load(Ordering::Relaxed),
        truncated,
        cancelled,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(root: &Path) -> SearchOptions {
        serde_json::from_value(serde_json::json!({ "root": root.display().to_string() })).unwrap()
    }

    fn tree() -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "syspulse-search-{}-{}",
            std::process::id(),
            rand_suffix()
        ));
        std::fs::create_dir_all(d.join("docs/notes")).unwrap();
        std::fs::create_dir_all(d.join("node_modules/pkg")).unwrap();
        std::fs::write(d.join("docs/Report_2024.pdf"), vec![1u8; 5000]).unwrap();
        std::fs::write(
            d.join("docs/notes/todo.txt"),
            "buy milk\nCall Praneeth tomorrow\n",
        )
        .unwrap();
        std::fs::write(d.join("node_modules/pkg/index.js"), "praneeth").unwrap();
        std::fs::write(d.join(".hidden.txt"), "praneeth").unwrap();
        d
    }

    fn rand_suffix() -> u128 {
        std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }

    fn run(o: &SearchOptions) -> Vec<String> {
        let mut v: Vec<String> = search(o, &AtomicBool::new(false), |_| {})
            .unwrap()
            .hits
            .into_iter()
            .map(|h| h.name)
            .collect();
        v.sort();
        v
    }

    #[test]
    fn glob_conversion() {
        assert_eq!(glob_to_regex("*.mp4"), r"^.*\.mp4$");
        assert!(Regex::new(&glob_to_regex("IMG_????.jpg"))
            .unwrap()
            .is_match("IMG_0042.jpg"));
    }

    #[test]
    fn name_modes_and_filters() {
        let d = tree();
        let mut o = opts(&d);
        o.query = "report".into();
        assert_eq!(run(&o), vec!["Report_2024.pdf"]);
        o.case_sensitive = true;
        assert!(run(&o).is_empty());
        o.case_sensitive = false;
        o.mode = MatchMode::Glob;
        o.query = "*.txt".into();
        assert_eq!(run(&o), vec!["todo.txt"], "hidden excluded by default");
        o.include_hidden = true;
        assert_eq!(run(&o), vec![".hidden.txt", "todo.txt"]);
        o.mode = MatchMode::Contains;
        o.query.clear();
        o.kind = EntryKind::Folders;
        assert_eq!(
            run(&o),
            vec!["docs", "notes"],
            "node_modules skipped as noise"
        );
        o.kind = EntryKind::Any;
        o.extensions = vec!["PDF".into()];
        o.min_size = Some(4000);
        assert_eq!(run(&o), vec!["Report_2024.pdf"]);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn content_search() {
        let d = tree();
        let mut o = opts(&d);
        o.content = Some("praneeth".into());
        let r = search(&o, &AtomicBool::new(false), |_| {}).unwrap();
        assert_eq!(r.hits.len(), 1);
        let c = r.hits[0].content.as_ref().unwrap();
        assert_eq!(c.line, 2);
        assert!(c.text.contains("Call Praneeth"));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn bad_regex_is_an_error() {
        let d = tree();
        let mut o = opts(&d);
        o.mode = MatchMode::Regex;
        o.query = "(".into();
        assert!(search(&o, &AtomicBool::new(false), |_| {}).is_err());
        std::fs::remove_dir_all(&d).unwrap();
    }
}
