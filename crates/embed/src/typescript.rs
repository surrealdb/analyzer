//! Embedded-query extraction over the TypeScript grammar.

use tree_sitter::{Node, Parser};

use crate::{EmbeddedQuery, Segment, Substitution, HOST_PARAM_PREFIX};

/// Finds every embedded SurrealQL query in a TypeScript source. `tsx`
/// selects the TSX grammar (needed for files with JSX).
pub fn extract_typescript(text: &str, tsx: bool) -> Vec<EmbeddedQuery> {
    let language = if tsx {
        tree_sitter_typescript::LANGUAGE_TSX
    } else {
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT
    };
    let mut parser = Parser::new();
    if parser.set_language(&language.into()).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(text, None) else {
        return Vec::new();
    };

    let mut queries = Vec::new();
    collect(tree.root_node(), text, &mut queries);
    queries
}

fn collect(node: Node<'_>, text: &str, queries: &mut Vec<EmbeddedQuery>) {
    if node.kind() == "call_expression" {
        if let (Some(function), Some(arguments)) = (
            node.child_by_field_name("function"),
            node.child_by_field_name("arguments"),
        ) {
            if is_query_sink(function, text) {
                if let Some(mut query) = call_string_to_query(arguments, text) {
                    // `defineLive` runs its string as `LIVE SELECT`, which is a
                    // far narrower statement than the SELECT the string parses
                    // as. Recording the sink is what lets the analyzer apply
                    // the live contract to it.
                    query.live = sink_is_live(function, text);
                    queries.push(query);
                }
            } else if is_query_method(function, text) {
                // The runtime API: `db.query("...")` / `db.live("...")`. The
                // first string (or template) argument is the query; a trailing
                // bindings object is ignored. This is what makes the generated
                // registry cover real client code, not just `defineQuery`
                // declarations.
                if let Some(query) = method_call_to_query(arguments, text) {
                    queries.push(query);
                }
            }
        }
    }
    let mut cursor = node.walk();
    let children: Vec<_> = node.children(&mut cursor).collect();
    for child in children {
        collect(child, text, queries);
    }
}

/// The identifiers whose call form carries a query literal: `defineQuery` /
/// `defineLive`, the 0.5 API where a query is a *value* built once and reused.
/// Extraction has to know them, or a query written that way is invisible to
/// `generate` — the registry comes out empty and every call resolves to
/// `unknown`, with nothing to say why.
///
/// There is deliberately no tag here. A `` surql`…` `` tagged template cannot
/// carry its literal into the type system at all: `TemplateStringsArray` has
/// no generic parameter (TypeScript#33304, open since 2019), so the literal is
/// erased before inference runs and the registry lookup has nothing to key on.
/// A sink that can never be typed is a sink that lies about being checked, so
/// the tag form was removed rather than kept as a second-class spelling.
const QUERY_SINKS: [&str; 2] = ["defineQuery", "defineLive"];

/// The sink is `defineLive` specifically — the one whose string the client
/// runs as a live query rather than a one-shot one.
fn sink_is_live(node: Node<'_>, text: &str) -> bool {
    let name = match node.kind() {
        "identifier" => &text[node.byte_range()],
        "member_expression" => match node.child_by_field_name("property") {
            Some(property) => &text[property.byte_range()],
            None => return false,
        },
        _ => return false,
    };
    name == "defineLive"
}

/// The callee names a query sink, either bare (`defineQuery(...)`) or through
/// a member (`sg.defineQuery(...)`).
fn is_query_sink(node: Node<'_>, text: &str) -> bool {
    let names = |name: &str| QUERY_SINKS.contains(&name);
    match node.kind() {
        "identifier" => names(&text[node.byte_range()]),
        "member_expression" => node
            .child_by_field_name("property")
            .is_some_and(|property| names(&text[property.byte_range()])),
        _ => false,
    }
}

/// The call form `defineQuery("...")`: one plain string-literal argument.
/// This is the *typed* form — string literals resolve through the generated
/// registry, which tagged templates cannot (TypeScript never infers
/// literal types for template string arrays).
fn call_string_to_query(arguments: Node<'_>, text: &str) -> Option<EmbeddedQuery> {
    if arguments.kind() != "arguments" {
        return None;
    }
    let mut walker = arguments.walk();
    let literals: Vec<Node<'_>> = arguments
        .children(&mut walker)
        .filter(|child| child.kind() == "string")
        .collect();
    let [string] = literals.as_slice() else {
        return None;
    };
    string_node_to_query(*string, text)
}

/// The function of a call is a `.query`/`.live` member access — the
/// SurrealDB runtime API (`db.query(...)`, `db.live(...)`). We match any
/// receiver rather than a fixed `db` name; the string-literal-first-arg
/// requirement keeps unrelated `.query(...)` calls (which pass options
/// objects, not string literals) from matching.
fn is_query_method(node: Node<'_>, text: &str) -> bool {
    node.kind() == "member_expression"
        && node
            .child_by_field_name("property")
            .is_some_and(|property| matches!(&text[property.byte_range()], "query" | "live"))
}

/// `db.query("...")` / `db.live("...")`: the first string (or template)
/// argument is the query; a trailing bindings object is ignored.
fn method_call_to_query(arguments: Node<'_>, text: &str) -> Option<EmbeddedQuery> {
    if arguments.kind() != "arguments" {
        return None;
    }
    let mut walker = arguments.walk();
    let first = arguments
        .children(&mut walker)
        .find(|child| matches!(child.kind(), "string" | "template_string"))?;
    match first.kind() {
        "template_string" => template_to_query(first, text),
        _ => string_node_to_query(first, text),
    }
}

/// Extracts a single `string` literal node's contents as a query.
/// Content between the quotes; escape sequences pass through verbatim
/// (SurrealQL strings share the common escapes).
fn string_node_to_query(string: Node<'_>, text: &str) -> Option<EmbeddedQuery> {
    let content_start = string.start_byte() + 1;
    let content_end = string.end_byte().saturating_sub(1);
    if content_end < content_start {
        return None;
    }
    let mut query = String::new();
    let mut segments = Vec::new();
    push_fragment(text, content_start..content_end, &mut query, &mut segments);
    Some(EmbeddedQuery {
        text: query,
        host_range: content_start..content_end,
        segments,
        substitutions: Vec::new(),
        live: false,
    })
}

/// Rebuilds the template's contents as analyzable SurrealQL: string
/// fragments copy verbatim (with a segment-map entry each), and every
/// `${...}` substitution becomes a `$__hostN` parameter — a bound value,
/// which is what a substitution almost always is (`WHERE s = ${x}`,
/// `LIMIT ${n}`, ...).
///
/// A `$param` is not valid everywhere, though: SurrealQL takes a plain
/// identifier, not an expression, in `ORDER BY`/`GROUP BY`/a field or table
/// name, so `` `SELECT * FROM t ORDER BY ${field} DESC` `` cannot parse as
/// written — `$__host0` sits where the grammar wants an `Ident`. That used to
/// surface as a hard `S0001` syntax error on the user's `.ts` file, for code
/// the user has no way to fix (the query is only ever assembled at runtime).
///
/// Built optimistically first (every hole as a parameter) and, only if that
/// does not parse clean, rebuilt once more with each hole tree-sitter-
/// surrealql could not place turned into an opaque backtick identifier
/// instead (`` `__host0` ``  — legal wherever a plain identifier is, and
/// exactly as unreadable to the analyzer as a `$param` in a position it
/// cannot check). A hole rebuilt this way is deliberately left out of
/// [`EmbeddedQuery::substitutions`]: it is no longer a value the host binds,
/// so nothing downstream should treat it as a generated parameter. If even
/// that still fails to parse, the whole query is dropped — silence beats a
/// syntax error on code the user cannot edit.
fn template_to_query(template: Node<'_>, text: &str) -> Option<EmbeddedQuery> {
    let content_start = template.start_byte() + 1;
    let content_end = template.end_byte().saturating_sub(1);
    if content_end < content_start {
        return None;
    }

    let mut walker = template.walk();
    let holes: Vec<Node<'_>> = template
        .children(&mut walker)
        .filter(|child| child.kind() == "template_substitution")
        .collect();

    // Attempt 0: every hole as a `$param` — correct, and the only build ever
    // needed, for the overwhelming majority of templates.
    let opaque_none = std::collections::HashSet::new();
    let (query, segments, substitutions) =
        build_template_query(text, content_start, content_end, &holes, &opaque_none);
    let broken = surrealql_error_spans(&query);
    if broken.is_empty() {
        return Some(EmbeddedQuery {
            text: query,
            host_range: content_start..content_end,
            segments,
            substitutions,
            live: false,
        });
    }
    let opaque: std::collections::HashSet<usize> = substitutions
        .iter()
        .filter(|sub| {
            broken
                .iter()
                .any(|span| ranges_overlap(span, &sub.embed_range))
        })
        .filter_map(|sub| {
            sub.param
                .strip_prefix(HOST_PARAM_PREFIX)
                .and_then(|index| index.parse::<usize>().ok())
        })
        .collect();
    if opaque.is_empty() {
        // Nothing here traces back to a hole — a genuine mistake in the
        // template's own text, not a substitution the grammar cannot place.
        // Return it exactly as attempt 0 built it, so the analyzer's own
        // parse of the query still reports the real syntax error, the way
        // it always has.
        return Some(EmbeddedQuery {
            text: query,
            host_range: content_start..content_end,
            segments,
            substitutions,
            live: false,
        });
    }

    // Attempt 1: retry with exactly the holes implicated above turned opaque.
    let (query, segments, substitutions) =
        build_template_query(text, content_start, content_end, &holes, &opaque);
    if surrealql_error_spans(&query).is_empty() {
        return Some(EmbeddedQuery {
            text: query,
            host_range: content_start..content_end,
            segments,
            substitutions,
            live: false,
        });
    }
    // Still unparseable even opaque — give up rather than guess a third
    // time. Silence beats a syntax error on code the user cannot edit.
    None
}

/// Builds the template query text once, with the holes named in `opaque`
/// rewritten as backtick identifiers instead of `$param`s. `holes` is every
/// `template_substitution` child, in source order — its index is each hole's
/// stable identity across the (at most two) times this runs, so a hole that
/// tree-sitter-surrealql accepted on the first attempt keeps the same
/// `$__hostN` name if a *different* hole needs the retry.
fn build_template_query(
    text: &str,
    content_start: usize,
    content_end: usize,
    holes: &[Node<'_>],
    opaque: &std::collections::HashSet<usize>,
) -> (String, Vec<Segment>, Vec<Substitution>) {
    let mut query = String::new();
    let mut segments = Vec::new();
    let mut substitutions = Vec::new();
    let mut host_cursor = content_start;

    for (index, child) in holes.iter().enumerate() {
        push_fragment(
            text,
            host_cursor..child.start_byte(),
            &mut query,
            &mut segments,
        );
        let param = format!("{HOST_PARAM_PREFIX}{index}");
        if opaque.contains(&index) {
            query.push('`');
            query.push_str(&param);
            query.push('`');
        } else {
            let embed_start = query.len();
            query.push('$');
            query.push_str(&param);
            substitutions.push(Substitution {
                param,
                host_range: child.byte_range(),
                embed_range: embed_start..query.len(),
            });
        }
        host_cursor = child.end_byte();
    }
    push_fragment(text, host_cursor..content_end, &mut query, &mut segments);

    (query, segments, substitutions)
}

/// Byte ranges of every unparseable span (`ERROR`/missing-token node)
/// SurrealQL's own grammar finds in `query`. Empty means `query` parses
/// clean — the common case, checked before anything else runs.
fn surrealql_error_spans(query: &str) -> Vec<std::ops::Range<usize>> {
    let mut parser = Parser::new();
    if parser
        .set_language(&tree_sitter_surrealql::LANGUAGE.into())
        .is_err()
    {
        return Vec::new();
    }
    let Some(tree) = parser.parse(query, None) else {
        return Vec::new();
    };
    let mut spans = Vec::new();
    collect_error_spans(tree.root_node(), &mut spans);
    spans
}

fn collect_error_spans(node: Node<'_>, spans: &mut Vec<std::ops::Range<usize>>) {
    if node.is_error() || node.is_missing() {
        spans.push(node.byte_range());
        // The whole span is already claimed as unparseable; nothing inside
        // it narrows the answer further.
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_error_spans(child, spans);
    }
}

fn ranges_overlap(a: &std::ops::Range<usize>, b: &std::ops::Range<usize>) -> bool {
    a.start < b.end && b.start < a.end
}

fn push_fragment(
    text: &str,
    host_range: std::ops::Range<usize>,
    query: &mut String,
    segments: &mut Vec<Segment>,
) {
    if host_range.is_empty() {
        return;
    }
    segments.push(Segment {
        embed_start: query.len(),
        host_start: host_range.start,
        len: host_range.len(),
    });
    query.push_str(&text[host_range]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tagged_template_is_not_a_query() {
        // `surql` was a sink until the type system proved it could never be
        // one: the literal is erased before inference, so a tagged query is a
        // query nothing can check. Extraction must not resurrect it — a
        // finding on text the registry cannot type is a finding with no fix.
        let source = r#"
const name = "Ada";
const q = surql`SELECT * FROM person`;
const other = css`b { color: red }`;
"#;
        assert!(extract_typescript(source, false).is_empty());
    }

    #[test]
    fn substitutions_become_parameters_with_host_ranges() {
        let source =
            "const q = db.query(`SELECT * FROM person WHERE age > ${min} AND name = ${name}`);";
        let queries = extract_typescript(source, false);

        assert_eq!(queries.len(), 1);
        assert_eq!(
            queries[0].text,
            "SELECT * FROM person WHERE age > $__host0 AND name = $__host1"
        );
        assert_eq!(queries[0].substitutions.len(), 2);
        assert_eq!(
            &source[queries[0].substitutions[0].host_range.clone()],
            "${min}"
        );
        assert_eq!(
            &source[queries[0].substitutions[1].host_range.clone()],
            "${name}"
        );
    }

    #[test]
    fn offsets_map_through_substitutions() {
        let source =
            "const q = db.query(`SELECT * FROM person WHERE age > ${min} AND name = 'x'`);";
        let queries = extract_typescript(source, false);
        let query = &queries[0];

        // `person` before the substitution maps verbatim.
        let embedded_person = query.text.find("person").expect("person present");
        let host = query.host_offset(embedded_person);
        assert_eq!(&source[host..host + 6], "person");

        // `name` after the substitution maps verbatim too.
        let embedded_name = query.text.find("name =").expect("name present");
        let host = query.host_offset(embedded_name);
        assert_eq!(&source[host..host + 4], "name");

        // An offset inside the generated parameter maps to the `${`.
        let embedded_param = query.text.find("$__host0").expect("param present");
        let host = query.host_offset(embedded_param + 3);
        assert_eq!(&source[host..host + 2], "${");
    }

    #[test]
    fn call_form_string_literals_extract() {
        let source = "const q = defineQuery(\"SELECT name FROM person WHERE team = $team\");";
        let queries = extract_typescript(source, false);

        assert_eq!(queries.len(), 1);
        assert_eq!(
            queries[0].text,
            "SELECT name FROM person WHERE team = $team"
        );
        assert!(queries[0].substitutions.is_empty());
        let embedded_team = queries[0].text.find("team =").expect("team present");
        let host = queries[0].host_offset(embedded_team);
        assert_eq!(&source[host..host + 4], "team");
    }

    #[test]
    fn db_query_string_literal_extracts() {
        let source =
            "const [rows] = await db.query(\"SELECT name FROM person WHERE team = $team\", { team });";
        let queries = extract_typescript(source, false);

        assert_eq!(queries.len(), 1);
        assert_eq!(
            queries[0].text,
            "SELECT name FROM person WHERE team = $team"
        );
        assert!(queries[0].substitutions.is_empty());
        // The query maps back to the host string, past the bindings object.
        let embedded_team = queries[0].text.find("team =").expect("team present");
        let host = queries[0].host_offset(embedded_team);
        assert_eq!(&source[host..host + 4], "team");
    }

    #[test]
    fn db_live_string_literal_extracts() {
        let source = "const handle = db.live(\"LIVE SELECT * FROM person\");";
        let queries = extract_typescript(source, false);

        assert_eq!(queries.len(), 1);
        assert_eq!(queries[0].text, "LIVE SELECT * FROM person");
    }

    #[test]
    fn unrelated_query_method_with_options_object_is_ignored() {
        // React-Query-style `.query({...})` passes an options object, never a
        // string literal — it must not be picked up as a SurrealQL query.
        let source = "const r = client.query({ url: '/api', method: 'GET' });";
        let queries = extract_typescript(source, false);

        assert!(queries.is_empty());
    }

    #[test]
    fn member_sinks_and_tsx_sources_work() {
        let source =
            "export const App = () => <div>{sg.defineQuery(\"SELECT 1 FROM person\")}</div>;";
        let queries = extract_typescript(source, true);

        assert_eq!(queries.len(), 1);
        assert_eq!(queries[0].text, "SELECT 1 FROM person");
    }

    #[test]
    fn a_hole_in_identifier_position_becomes_an_opaque_placeholder() {
        // `${'n'}` sits where ORDER BY wants a plain field name, not a bound
        // parameter — `$__host1` there does not parse. This used to surface
        // as a hard S0001 on the .ts file for code the user cannot edit; it
        // must now silently rewrite as a backtick identifier instead, and the
        // query as a whole must still parse (and still type the value hole
        // that DOES belong in a value position).
        let source =
            "const q = db.query(`SELECT n, s FROM t WHERE s = ${\"'x'\"} ORDER BY ${'n'} DESC`);";
        let queries = extract_typescript(source, false);

        assert_eq!(queries.len(), 1);
        let query = &queries[0];
        assert!(
            surrealql_error_spans(&query.text).is_empty(),
            "rebuilt query still fails to parse: {}",
            query.text
        );
        // The value hole (`${"'x'"}`) stayed a real, host-bound parameter...
        assert_eq!(query.substitutions.len(), 1);
        assert_eq!(query.substitutions[0].param, "__host0");
        assert_eq!(
            &source[query.substitutions[0].host_range.clone()],
            "${\"'x'\"}"
        );
        // ...and the identifier-position hole (`${'n'}`) became an opaque
        // backtick identifier, not a second substitution.
        assert!(query.text.contains("ORDER BY `__host1` DESC"));
    }

    #[test]
    fn a_genuine_syntax_mistake_unrelated_to_a_hole_still_reports() {
        // A parse error the retry cannot possibly fix (nothing here is a
        // substitution) must still come back as a query — dropping it would
        // silence a real mistake, not just a false positive.
        let source = r#"const q = db.query(`SELECT * FROM t WHERE x = "abc`);"#;
        let queries = extract_typescript(source, false);

        assert_eq!(queries.len(), 1);
        assert!(!surrealql_error_spans(&queries[0].text).is_empty());
    }
}
