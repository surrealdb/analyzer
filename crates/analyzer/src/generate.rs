//! `generate`: emit the typed TypeScript client and literal-keyed query
//! registry for every SurrealQL query embedded in the project's host files.
//!
//! Like [`check`](crate::check), the verb returns data and renders on
//! request. Findings raised on embedded queries are reported at their host
//! `file:line`: warnings and hints are carried back but do not block, while
//! any error-severity finding aborts *before* writing, so a broken registry
//! never overwrites a good one.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use surrealql_analyzer_codegen::QueryEntry;
use surrealql_analyzer_diagnostics::Severity;

use crate::analyze::{analyze, SourceError};
use crate::diagnostic::{Diagnostic, Findings};
use crate::project::{display_relative, Project};
use crate::style::Styles;

/// The npm package the generated module imports from and augments.
pub const CLIENT_PACKAGE: &str = "@surrealdb/analyzer-client";

/// A successful `generate`.
#[derive(Debug)]
pub struct GenerateReport {
    /// Where the registry was written.
    pub path: PathBuf,
    /// The module text that was written.
    pub module: String,
    /// How many embedded queries landed in the registry, so a repeating
    /// watch line still shows the run did something.
    pub queries: usize,
    /// Warning/hint findings that survived policy, host-mapped.
    pub warnings: Vec<Diagnostic>,
    /// Whether the module augments [`CLIENT_PACKAGE`] but that package cannot
    /// be resolved from the written module's directory. See
    /// [`client_package_is_resolvable`] for why this matters so much.
    pub missing_client: bool,
    root: PathBuf,
    findings: Findings,
}

impl GenerateReport {
    /// One rustc-style block per surviving warning/hint.
    pub fn render_warnings(&self, styles: Styles) -> Vec<String> {
        self.findings.render(styles)
    }

    /// The "`@surrealdb/analyzer-client` is not installed" block, when
    /// [`missing_client`](Self::missing_client) is set.
    pub fn render_missing_client(&self, styles: Styles) -> Option<String> {
        self.missing_client
            .then(|| missing_client_warning(&self.path, &self.root, styles))
    }
}

/// `generate` could not produce a registry.
#[derive(Debug)]
pub enum GenerateError {
    /// An embedded query has an error-severity finding, so nothing was
    /// written.
    Blocked(GenerateBlocked),
    /// A source could not be read, or the registry could not be written.
    Io(SourceError),
}

impl fmt::Display for GenerateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Blocked(blocked) => write!(f, "{blocked}"),
            Self::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for GenerateError {}

impl From<SourceError> for GenerateError {
    fn from(error: SourceError) -> Self {
        Self::Io(error)
    }
}

/// `generate` refused to write because an embedded query has an
/// error-severity finding. Carries every finding host-mapped to `file:line`
/// so the user can fix the query the client actually runs.
#[derive(Debug)]
pub struct GenerateBlocked {
    /// How many error-severity findings blocked the write.
    pub errors: usize,
    /// Every finding on the failing run, errors first, host-mapped.
    pub diagnostics: Vec<Diagnostic>,
    findings: Findings,
}

impl GenerateBlocked {
    /// One rustc-style block per finding, errors first.
    pub fn render(&self, styles: Styles) -> Vec<String> {
        self.findings.render(styles)
    }
}

impl fmt::Display for GenerateBlocked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "generate failed: {} error(s) in embedded queries — registry not written",
            self.errors
        )
    }
}

impl std::error::Error for GenerateBlocked {}

/// Scans host sources for embedded queries, analyzes them against the
/// project's schema, and writes the typed registry to `out` — or to
/// [`Project::registry_path`]'s default when `out` is `None`.
pub fn generate(project: &Project, out: Option<&Path>) -> Result<GenerateReport, GenerateError> {
    let analyzed = analyze(project)?;

    // Only the embedded queries' findings decide this verb: they are the
    // queries the registry types. Errors first, so a blocked run reads them
    // before the warnings that came with them.
    let mut resolved: Vec<_> = analyzed
        .resolve(project)
        .into_iter()
        .filter(|resolved| resolved.embedded)
        .map(|resolved| (resolved.finding, resolved.severity))
        .collect();
    resolved.sort_by_key(|(_, severity)| *severity != Severity::Error);
    let findings = Findings::new(resolved, analyzed.texts, project.root());

    let errors = findings.errors();
    if errors > 0 {
        // Don't overwrite a good registry with a broken one — bail before writing.
        return Err(GenerateError::Blocked(GenerateBlocked {
            errors,
            diagnostics: findings.diagnostics(),
            findings,
        }));
    }

    // Entry construction (the per-statement response tuple, the params) is
    // the codegen crate's, shared with its `tsc`-checked golden test so the
    // module this writes is the module that test compiles.
    let entries: Vec<QueryEntry> = analyzed
        .embedded
        .queries
        .iter()
        .filter_map(|entry| {
            let output = analyzed.analysis.sources.get(&entry.source_id)?;
            Some(QueryEntry::from_analysis(entry.query.parts(), output))
        })
        .collect();

    let path = project.registry_path(out);
    let module = surrealql_analyzer_codegen::render_registry(&entries);
    // The registry usually lands beside the client code (`src/lib/db.generated.ts`),
    // and that directory need not exist yet — a fresh project, or an `out` that
    // names a directory the host has not created. Creating it is the obvious
    // reading of "write the registry here", and the alternative is an ENOENT
    // that names the file rather than the missing directory.
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|source| SourceError {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    fs::write(&path, &module).map_err(|source| SourceError {
        path: path.clone(),
        source,
    })?;

    // Resolution is asked from the *written module's* directory, not the
    // project root: that is the directory TypeScript resolves the import
    // from, and in a monorepo the two are routinely different packages.
    let missing_client = module.contains(CLIENT_PACKAGE)
        && !client_package_is_resolvable(path.parent().unwrap_or(project.root()));

    Ok(GenerateReport {
        path,
        module,
        queries: entries.len(),
        warnings: findings.diagnostics(),
        missing_client,
        root: project.root().to_path_buf(),
        findings,
    })
}

/// Whether Node/TypeScript would resolve [`CLIENT_PACKAGE`] from `dir`, by the
/// rule they both use: walk up from the importing file's directory and take the
/// first `node_modules` that contains the package.
///
/// This is the difference between a generated file that types everything and
/// one that types nothing. The module ends in
/// `declare module "@surrealdb/analyzer-client" { … }`, and a module augmentation is
/// only an augmentation if the target resolves. When it does not, TypeScript
/// reports `TS2664: Invalid module name in augmentation` **inside the generated
/// file** and drops the block — so the user's own `db.query(…)` keeps
/// compiling, silently as `any`, with no error anywhere near it. Nothing about
/// that failure points at the missing dependency, which is why `generate` has
/// to say it out loud.
///
/// `package.json` is the marker rather than the directory, because a leftover
/// empty `node_modules/@surrealdb/analyzer-client/` resolves for neither tool.
pub fn client_package_is_resolvable(dir: &Path) -> bool {
    let scope_path: PathBuf = CLIENT_PACKAGE.split('/').collect();
    let mut current = Some(dir);
    while let Some(directory) = current {
        if directory
            .join("node_modules")
            .join(&scope_path)
            .join("package.json")
            .exists()
        {
            return true;
        }
        current = directory.parent();
    }
    false
}

/// The warning for a written module that augments a package which is not
/// installed. Shaped like a finding — header, location, explanation, fix — so
/// it reads in the same language as everything else `generate` reports.
fn missing_client_warning(out_path: &Path, root: &Path, styles: Styles) -> String {
    let bar = styles.frame("  |");
    let mut out = format!(
        "{}: {}\n",
        styles.severity(Severity::Warning, "warning"),
        styles.message(&format!("`{CLIENT_PACKAGE}` is not installed"))
    );
    out.push_str(&format!(
        "  {} {}\n",
        styles.frame("-->"),
        styles.path(&display_relative(root, out_path))
    ));
    out.push_str(&format!("{bar}\n"));
    out.push_str(&format!(
        "{bar} this file augments `declare module \"{CLIENT_PACKAGE}\"`, and an\n"
    ));
    out.push_str(&format!(
        "{bar} augmentation whose target does not resolve is silently dropped —\n"
    ));
    out.push_str(&format!(
        "{bar} TypeScript reports TS2664 here, and every query typed through it\n"
    ));
    out.push_str(&format!("{bar} degrades to `any` with no error on it.\n"));
    out.push_str(&format!("{bar}\n"));
    out.push_str(&format!(
        "  {} {} npm install {CLIENT_PACKAGE} surrealdb\n",
        styles.frame("="),
        styles.label("help:"),
    ));
    out
}
