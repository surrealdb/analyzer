//! The `/** … */` TOML header a SurrealDB language test carries.
//!
//! Only the parts the oracle needs are read: which files to skip, which
//! namespace and database to run in, which fixtures to import first, and how
//! many results the test expects — that last one purely as a cross-check that
//! the analyzer and the engine agree on the statement count.

use toml::Value as Toml;

/// Split a test file into its concatenated header TOML and its body.
///
/// The body is the original text with every test comment blanked to spaces,
/// newlines kept. Byte offsets and line numbers therefore survive, and both the
/// analyzer and the engine are handed *identical* text — which is what makes
/// statement `n` on one side statement `n` on the other.
pub fn split(text: &str) -> (String, String) {
    let bytes = text.as_bytes();
    let mut toml = String::new();
    let mut body = bytes.to_vec();
    let mut index = 0;
    while index < bytes.len() {
        if let Some(rest) = text.get(index..) {
            if let Some(end) = block(rest) {
                toml.push_str(&rest[3..end - 2]);
                toml.push('\n');
                blank(&mut body, index, index + end);
                index += end;
                continue;
            }
            if rest.starts_with("//!") && at_line_start(text, index) {
                let end = rest.find('\n').unwrap_or(rest.len());
                toml.push_str(&rest[3..end]);
                toml.push('\n');
                blank(&mut body, index, index + end);
                index += end;
                continue;
            }
        }
        index += 1;
    }
    let body = String::from_utf8(body).unwrap_or_else(|_| text.to_string());
    (toml, body)
}

/// The length of the `/** … */` block starting here, if there is one.
fn block(rest: &str) -> Option<usize> {
    rest.starts_with("/**")
        .then(|| rest[3..].find("*/").map(|offset| 3 + offset + 2))
        .flatten()
}

/// Replace a byte range with spaces, keeping newlines so line numbers hold.
fn blank(body: &mut [u8], from: usize, to: usize) {
    for byte in &mut body[from..to] {
        if *byte != b'\n' {
            *byte = b' ';
        }
    }
}

fn at_line_start(text: &str, index: usize) -> bool {
    text[..index]
        .rsplit('\n')
        .next()
        .is_none_or(|line| line.trim().is_empty())
}

/// The parts of a test header the oracle acts on.
#[derive(Clone, Debug, Default)]
pub struct Header {
    /// The namespace to run in; `None` when the test declares `namespace = false`.
    pub namespace: Option<String>,
    /// The database to run in; `None` when the test declares `database = false`.
    pub database: Option<String>,
    /// Fixture files to run before the test, in declaration order.
    pub imports: Vec<String>,
    /// How many results `[[test.results]]` declares, when it declares any.
    pub result_count: Option<usize>,
    /// Why this file cannot be used, if it cannot be.
    pub skip: Option<String>,
    /// The engine version requirement, if the test states one.
    pub version: Option<String>,
}

impl Header {
    /// Read a test header. An absent or unparseable header is a skip, not an
    /// error: the corpus is upstream's and may grow forms we do not know.
    pub fn parse(toml: &str) -> Result<Self, String> {
        if toml.trim().is_empty() {
            return Err("the file has no test header".to_string());
        }
        let config: Toml =
            toml::from_str(toml).map_err(|_| "the header is not valid TOML".to_string())?;
        let test = config.get("test");
        let env = config.get("env");
        let mut header = Header {
            namespace: name(env, "namespace"),
            database: name(env, "database"),
            imports: env
                .and_then(|env| env.get("imports"))
                .and_then(Toml::as_array)
                .map(|imports| {
                    imports
                        .iter()
                        .filter_map(Toml::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
            version: test
                .and_then(|test| test.get("version"))
                .and_then(Toml::as_str)
                .map(str::to_string),
            ..Header::default()
        };
        header.skip = skip_reason(test, env);
        header.result_count = match test.and_then(|test| test.get("results")) {
            Some(Toml::Array(results)) => Some(results.len()),
            // The table form is the `parsing-error = …` spelling: the file is
            // not SurrealQL the engine will run.
            Some(_) => {
                header
                    .skip
                    .get_or_insert("the test expects a parse error".to_string());
                None
            }
            None => None,
        };
        Ok(header)
    }

    /// Whether this file can be used against an engine of this version.
    pub fn supported(&self, engine: &semver::Version) -> Result<(), String> {
        if let Some(reason) = &self.skip {
            return Err(reason.clone());
        }
        if let Some(requirement) = &self.version {
            let requirement = semver::VersionReq::parse(requirement)
                .map_err(|_| "the version requirement is not semver".to_string())?;
            if !requirement.matches(engine) {
                return Err("the test requires a different engine version".to_string());
            }
        }
        Ok(())
    }
}

/// `namespace` / `database`: a string names one, `true` means the default
/// `"test"`, `false` means run without one.
fn name(env: Option<&Toml>, key: &str) -> Option<String> {
    match env.and_then(|env| env.get(key)) {
        Some(Toml::String(name)) => Some(name.clone()),
        Some(Toml::Boolean(false)) => None,
        _ => Some("test".to_string()),
    }
}

/// Everything that puts a file out of the oracle's reach. Each reason is
/// counted and printed, so the skipped population is visible rather than
/// quietly dropped.
fn skip_reason(test: Option<&Toml>, env: Option<&Toml>) -> Option<String> {
    let flag = |table: Option<&Toml>, key: &str| {
        table
            .and_then(|table| table.get(key))
            .and_then(Toml::as_bool)
    };
    if flag(test, "run") == Some(false) {
        return Some("an import fixture (run = false)".to_string());
    }
    if flag(test, "wip") == Some(true) {
        return Some("a work-in-progress test (wip = true)".to_string());
    }
    if flag(env, "versioned") == Some(true) {
        return Some("the test needs a versioned datastore".to_string());
    }
    for key in ["signin", "signup", "capabilities"] {
        if env.and_then(|env| env.get(key)).is_some() {
            return Some(format!("the test declares [env] {key}"));
        }
    }
    // `auth` bypasses signin to run as some role. Root owner is what the oracle
    // already runs as; anything narrower changes what the statements return.
    if let Some(auth) = env.and_then(|env| env.get("auth")) {
        let root_owner = auth.get("level").and_then(Toml::as_str) == Some("owner")
            && auth.get("namespace").is_none()
            && auth.get("database").is_none();
        if !root_owner {
            return Some("the test runs under restricted auth".to_string());
        }
    }
    if let Some(backends) = env
        .and_then(|env| env.get("backend"))
        .and_then(Toml::as_array)
    {
        let memory = backends
            .iter()
            .filter_map(Toml::as_str)
            .any(|backend| backend == "mem" || backend == "memory");
        if !backends.is_empty() && !memory {
            return Some("the test needs a non-memory backend".to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blanks_the_header_and_keeps_the_offsets() {
        let text = "/**\n[test]\nrun = true\n*/\nRETURN 1;\n";
        let (toml, body) = split(text);
        assert!(toml.contains("[test]"));
        assert_eq!(body.len(), text.len());
        assert_eq!(body.lines().nth(4), Some("RETURN 1;"));
        assert!(body.lines().take(4).all(|line| line.trim().is_empty()));
    }

    #[test]
    fn concatenates_every_test_comment() {
        let (toml, body) = split("//! [env]\n//! namespace = \"ns\"\n/**\n[test]\n*/\nRETURN 1;");
        let header = Header::parse(&toml).expect("a header");
        assert_eq!(header.namespace.as_deref(), Some("ns"));
        assert_eq!(body.trim(), "RETURN 1;");
    }

    #[test]
    fn reads_the_result_count_and_the_skips() {
        let header = Header::parse(
            "[test]\n[[test.results]]\nvalue = \"1\"\n[[test.results]]\nerror = true\n",
        )
        .expect("a header");
        assert_eq!(header.result_count, Some(2));
        assert_eq!(header.skip, None);

        let fixture = Header::parse("[test]\nrun = false\n").expect("a header");
        assert_eq!(
            fixture.skip.as_deref(),
            Some("an import fixture (run = false)")
        );

        let parse_error =
            Header::parse("[test]\n[test.results]\nparsing-error = true\n").expect("a header");
        assert_eq!(
            parse_error.skip.as_deref(),
            Some("the test expects a parse error")
        );
    }

    #[test]
    fn a_version_requirement_gates_the_file() {
        let header = Header::parse("[test]\nversion = \">=4.0.0\"\n").expect("a header");
        let engine = semver::Version::parse("3.2.3").expect("a version");
        assert!(header.supported(&engine).is_err());
        let header = Header::parse("[test]\nversion = \">=3.0.0\"\n").expect("a header");
        assert!(header.supported(&engine).is_ok());
    }

    #[test]
    fn namespace_false_means_no_namespace() {
        let header = Header::parse("[env]\nnamespace = false\ndatabase = false\n[test]\n")
            .expect("a header");
        assert_eq!(header.namespace, None);
        assert_eq!(header.database, None);
        let header = Header::parse("[test]\n").expect("a header");
        assert_eq!(header.namespace.as_deref(), Some("test"));
    }
}
