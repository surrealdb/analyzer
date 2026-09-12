//! The resolved inputs of one analysis run: a workspace root, the
//! configuration that grades its findings, and the file sets discovered
//! under it.
//!
//! [`Project`] is the seam between "where the files are" and "what the
//! analyzer does with them". A host that already knows its layout — SurrealKit
//! — builds one with [`Project::new`], handing in a [`WorkspaceConfig`] it
//! assembled itself, so it never needs a second config file to say where its
//! schema lives. [`Project::discover`] walks up for a `surrealql-analyzer.toml`
//! for the workspaces that have one.
//!
//! Discovery is shared rather than duplicated per verb: `check` and
//! `generate` must never disagree about which files carry queries. A `check`
//! that skipped a host file would pass a workspace whose `generate` then
//! fails — CI green, build broken.

use std::fs;
use std::path::{Path, PathBuf};

use surrealql_analyzer_workspace::config::WorkspaceConfig;
use walkdir::{DirEntry, WalkDir};

/// The file name [`Project::discover`] looks for.
pub const CONFIG_FILE_NAME: &str = "surrealql-analyzer.toml";

/// The registry file name `generate` writes when the caller names no path.
pub const DEFAULT_REGISTRY_NAME: &str = "surrealql-analyzer.generated.ts";

/// A workspace root paired with the configuration that governs it.
///
/// Cheap to clone and free of I/O until a verb runs, so a host can build one
/// per schema module and analyze each in turn.
#[derive(Clone, Debug)]
pub struct Project {
    root: PathBuf,
    config: WorkspaceConfig,
}

/// The files one analysis run reads, found in a single walk of the root.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sources {
    /// Every `.surql` / `.surrealql` file, schema files first.
    ///
    /// Schema sources are analyzed before query sources so their `DEFINE`s
    /// are in scope for the queries that reference them. A file matching a
    /// `schema` glob is schema; everything else is a query. Ties (a file
    /// matching both) resolve to schema — analyzing a definition early is
    /// always safe.
    pub surrealql: Vec<PathBuf>,
    /// Every host file (`.ts`, `.svelte`, …) that may carry embedded SurrealQL.
    pub host: Vec<PathBuf>,
}

/// A `surrealql-analyzer.toml` that exists but could not be read or parsed.
#[derive(Debug)]
pub enum ConfigError {
    /// The file could not be read.
    Read {
        /// The path that failed.
        path: PathBuf,
        /// The underlying I/O error.
        source: std::io::Error,
    },
    /// The file was read but is not valid configuration.
    Parse(surrealql_analyzer_workspace::config::ConfigError),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Parse(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ConfigError {}

impl Project {
    /// A project at `root` governed by `config`, with no file system lookup
    /// for a config file.
    ///
    /// This is the embedding host's constructor. SurrealKit knows its schema
    /// directory from its own layout, so it builds the [`WorkspaceConfig`]
    /// directly and never writes an analyzer config file into a user's
    /// project.
    pub fn new(root: impl Into<PathBuf>, config: WorkspaceConfig) -> Self {
        Self {
            root: root.into(),
            config,
        }
    }

    /// Walks up from `start_dir` for a [`CONFIG_FILE_NAME`], and loads it.
    ///
    /// A workspace with no config file is not an error: it analyzes under
    /// [`WorkspaceConfig::default`], rooted at `start_dir`. Only a config
    /// that exists and cannot be understood fails.
    pub fn discover(start_dir: &Path) -> Result<Self, ConfigError> {
        let root = find_workspace_root(start_dir);
        let config = load_workspace_config(&root)?;
        Ok(Self { root, config })
    }

    /// The workspace root every discovered path is relative to.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The configuration grading this workspace's findings.
    pub fn config(&self) -> &WorkspaceConfig {
        &self.config
    }

    /// Where [`Project::discover`] would read this project's config file,
    /// whether or not one exists — the path a host hands to the watcher as an
    /// extra input so an edit to it re-runs.
    pub fn config_path(&self) -> PathBuf {
        self.root.join(CONFIG_FILE_NAME)
    }

    /// Where `generate` writes the registry, given the caller's output path.
    pub fn registry_path(&self, out: Option<&Path>) -> PathBuf {
        out.map_or_else(|| self.root.join(DEFAULT_REGISTRY_NAME), Path::to_path_buf)
    }

    /// Every file the analysis reads, in one walk of the root.
    ///
    /// Both sets come from the same filesystem snapshot, and both honour the
    /// config's `[sources] ignore` patterns — ignored directories are never
    /// descended into, so `node_modules/` costs nothing.
    pub fn sources(&self) -> Sources {
        let ignore = &self.config.sources.ignore;
        let mut surrealql = Vec::new();
        let mut host = Vec::new();
        for entry in WalkDir::new(&self.root)
            .into_iter()
            .filter_entry(|entry| should_visit(entry, &self.root, ignore))
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
        {
            let path = entry.into_path();
            if is_surrealql_source(&path) {
                surrealql.push(path);
            } else if is_host_source(&path) {
                host.push(path);
            }
        }
        surrealql.sort();
        host.sort();

        let schema_globs: Vec<glob::Pattern> = self
            .config
            .sources
            .schema
            .iter()
            .filter_map(|pattern| glob::Pattern::new(pattern).ok())
            .collect();
        let (mut schema, mut queries): (Vec<PathBuf>, Vec<PathBuf>) =
            surrealql.into_iter().partition(|path| {
                let relative = path.strip_prefix(&self.root).unwrap_or(path);
                schema_globs
                    .iter()
                    .any(|pattern| pattern.matches_path(relative))
            });
        schema.append(&mut queries);
        Sources {
            surrealql: schema,
            host,
        }
    }

    /// A path shown relative to this root when it lives under it, so output
    /// reads `src/app.ts` rather than an absolute path.
    pub fn display_relative(&self, path: &Path) -> String {
        display_relative(&self.root, path)
    }
}

/// `path` relative to `root` when it lives under it; `path` as given otherwise.
pub(crate) fn display_relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

/// Walks up from `start_dir` for the directory holding a
/// [`CONFIG_FILE_NAME`], falling back to `start_dir` when none is found.
fn find_workspace_root(start_dir: &Path) -> PathBuf {
    let mut current = start_dir.to_path_buf();
    loop {
        if current.join(CONFIG_FILE_NAME).exists() {
            return current;
        }
        if !current.pop() {
            return start_dir.to_path_buf();
        }
    }
}

/// Loads `root`'s config file, or the default configuration when the
/// workspace has none.
fn load_workspace_config(root: &Path) -> Result<WorkspaceConfig, ConfigError> {
    let config_path = root.join(CONFIG_FILE_NAME);
    if !config_path.exists() {
        return Ok(WorkspaceConfig::default());
    }
    let text = fs::read_to_string(&config_path).map_err(|source| ConfigError::Read {
        path: config_path,
        source,
    })?;
    WorkspaceConfig::from_toml_str(&text).map_err(ConfigError::Parse)
}

/// Whether the walker should descend into `entry`, given the ignore patterns.
fn should_visit(entry: &DirEntry, root: &Path, ignore_patterns: &[String]) -> bool {
    if entry.path() == root {
        return true;
    }
    let relative = entry.path().strip_prefix(root).unwrap_or(entry.path());
    !ignore_patterns
        .iter()
        .any(|pattern| matches_simple_ignore(relative, pattern))
}

/// Whether `relative` is covered by one simple ignore `pattern`: a directory
/// name, optionally written `name/**`, anywhere in the path.
pub(crate) fn matches_simple_ignore(relative: &Path, pattern: &str) -> bool {
    let trimmed = pattern.strip_suffix("/**").unwrap_or(pattern);
    relative.components().any(|component| {
        component
            .as_os_str()
            .to_str()
            .is_some_and(|name| name == trimmed)
    })
}

/// Whether `path` is a plain SurrealQL source file.
pub(crate) fn is_surrealql_source(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("surql" | "surrealql")
    )
}

/// Whether `path` is a host file that may carry embedded SurrealQL. Shared
/// with the watcher so the watched set and the discovered set can never
/// disagree — a file the watcher ignores but `generate` reads would go
/// silently stale, which is the exact bug watching exists to fix.
pub(crate) fn is_host_source(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("ts" | "tsx" | "js" | "jsx" | "svelte" | "vue" | "astro")
    )
}
