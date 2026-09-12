//! `generate`: emit the TypeScript declaration file that types every
//! SurrealQL query embedded in the project's host files, and every table its
//! schema defines.
//!
//! The output is a **`.d.ts`**: types and nothing else. It declares no
//! values, re-exports no runtime and augments no module; a consumer imports
//! `Queries` from it and hands that to `createClient<Queries>(…)`. That is
//! why the verb refuses an output path that is not a declaration file — a
//! `.ts` would invite `import { createClient } from "./…"`, which used to
//! work and now resolves to nothing.
//!
//! Like [`check`](crate::check), the verb returns data and renders on
//! request. Findings raised on embedded queries are reported at their host
//! `file:line`: warnings and hints are carried back but do not block, while
//! any error-severity finding aborts *before* writing, so a broken file
//! never overwrites a good one.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use surrealql_analyzer_diagnostics::Severity;

use crate::analyze::{analyze, SourceError};
use crate::describe::document;
use crate::diagnostic::{Diagnostic, Findings};
use crate::project::{display_relative, Project, TYPES_EXTENSION};
use crate::style::Styles;

/// The npm package the generated module imports its value types from.
///
/// Defined by the emitter, restated here because a host asks this crate
/// whether the package is installed.
pub use surrealql_analyzer_codegen::CLIENT_PACKAGE;

/// A successful `generate`.
#[derive(Debug)]
pub struct GenerateReport {
    /// Where the declaration file was written.
    pub path: PathBuf,
    /// The module text that was written.
    pub module: String,
    /// How many embedded queries landed in `Queries`, so a repeating watch
    /// line still shows the run did something.
    pub queries: usize,
    /// Warning/hint findings that survived policy, host-mapped.
    pub warnings: Vec<Diagnostic>,
    /// Whether the module imports from [`CLIENT_PACKAGE`] but that package
    /// cannot be resolved from the written module's directory. See
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
    /// A source could not be read, or the output could not be written.
    Io(SourceError),
    /// The requested output path is not a TypeScript declaration file.
    NotDeclaration(PathBuf),
}

impl fmt::Display for GenerateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Blocked(blocked) => write!(f, "{blocked}"),
            Self::Io(error) => write!(f, "{error}"),
            Self::NotDeclaration(path) => write!(
                f,
                "generate emits types only, so its output must be a TypeScript \
                 declaration file: `{}` does not end in `{TYPES_EXTENSION}`. \
                 Nothing in the generated file exists at runtime — import the \
                 client from `{CLIENT_PACKAGE}` and its types from here.",
                path.display()
            ),
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
            "generate failed: {} error(s) in embedded queries — types not written",
            self.errors
        )
    }
}

impl std::error::Error for GenerateBlocked {}

/// Scans host sources for embedded queries, analyzes them against the
/// project's schema, and writes the declaration file to `out` — or to
/// [`Project::registry_path`]'s default when `out` is `None`.
///
/// `out` must name a `.d.ts`. The check runs before the analysis, so a
/// mistyped flag fails in milliseconds rather than after a full run.
pub fn generate(project: &Project, out: Option<&Path>) -> Result<GenerateReport, GenerateError> {
    let path = project.registry_path(out);
    if !is_declaration_file(&path) {
        return Err(GenerateError::NotDeclaration(path));
    }

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

    // Described before the findings take ownership of the source texts, and
    // before the error gate: the document is the single source of truth for
    // what the project's types are, and the emitter only spells it in
    // TypeScript. Both halves are the codegen crate's, shared with its
    // `tsc`-checked golden test, so the module this writes is the module that
    // test compiles.
    let described = document(&analyzed);
    let findings = Findings::new(resolved, analyzed.texts, project.root());

    let errors = findings.errors();
    if errors > 0 {
        // Don't overwrite a good file with a broken one — bail before writing.
        return Err(GenerateError::Blocked(GenerateBlocked {
            errors,
            diagnostics: findings.diagnostics(),
            findings,
        }));
    }

    let rendered = surrealql_analyzer_codegen::render_types_module(&described);
    let module = rendered.text;
    // The file usually lands beside the client code (`src/lib/surrealql-analyzer.d.ts`),
    // and that directory need not exist yet — a fresh project, or an `out` that
    // names a directory the host has not created. Creating it is the obvious
    // reading of "write the types here", and the alternative is an ENOENT
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

    // Gated on what the module IMPORTS, not on whether its text mentions the
    // package: the header names it twice in prose, so a `contains` is true of
    // every module this crate emits — including one that imports nothing,
    // where the package's absence costs nothing and the warning would be
    // noise on an empty project.
    //
    // Resolution is asked from the *written module's* directory, not the
    // project root: that is the directory TypeScript resolves the import
    // from, and in a monorepo the two are routinely different packages.
    let missing_client = !rendered.imports.is_empty()
        && !client_package_is_resolvable(path.parent().unwrap_or(project.root()));

    Ok(GenerateReport {
        path,
        module,
        queries: described.queries.len(),
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
/// The types-only file no longer augments that package, which removes the
/// worst version of this failure — a dropped augmentation that left every
/// query `any` with no error anywhere. What remains is quieter but still
/// worth saying out loud: the file imports `RecordId`, `Uuid`, `Duration` and
/// `Decimal` from it, and almost every project sets `skipLibCheck: true`,
/// which suppresses errors *inside declaration files*. So an unresolvable
/// import is not reported at all, and every value class it names silently
/// becomes `any` — a `RecordId<"team">` parameter stops being distinguishable
/// from a string, which is exactly the bug the generated types exist to catch.
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

/// Whether `path` names a TypeScript declaration file. The double extension
/// is the whole check: `.d.ts` is what tells every tool in the chain that the
/// file declares types and emits nothing.
fn is_declaration_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(TYPES_EXTENSION) && name.len() > TYPES_EXTENSION.len())
}

/// The warning for a written module that imports from a package which is not
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
        "{bar} this file imports `RecordId`, `Uuid`, `Duration` and `Decimal`\n"
    ));
    out.push_str(&format!(
        "{bar} from it. `skipLibCheck` — which nearly every project sets —\n"
    ));
    out.push_str(&format!(
        "{bar} suppresses errors inside a `.d.ts`, so an import that does not\n"
    ));
    out.push_str(&format!(
        "{bar} resolve is never reported and every one of those classes\n"
    ));
    out.push_str(&format!(
        "{bar} silently becomes `any` — a record link stops being told apart\n"
    ));
    out.push_str(&format!("{bar} from a string.\n"));
    out.push_str(&format!("{bar}\n"));
    out.push_str(&format!(
        "  {} {} npm install {CLIENT_PACKAGE} surrealdb\n",
        styles.frame("="),
        styles.label("help:"),
    ));
    out
}
