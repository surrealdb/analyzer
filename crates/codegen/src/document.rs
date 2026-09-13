//! The **types document**: everything the analyzer knows about a project's
//! types, in one language-neutral, serializable value.
//!
//! Generation used to go straight from an analysis to a string of TypeScript,
//! which made the emitter the only place a fact existed. A second language —
//! and there are three waiting (Rust's `query!` wants named structs, Python's
//! `.into()` wants dataclasses) — would have had to re-derive the same facts
//! from the same analysis, and the two derivations would drift. So the
//! pipeline now has a middle: `analysis → TypesDocument → emitter`, and every
//! emitter renders the *document*.
//!
//! Nothing here is re-modelled. Kinds are upstream `surrealdb_types::Kind`,
//! the same values the engine infers and the schema index stores; the document
//! is a projection of [`SchemaIndex`] and [`AnalysisOutput`] onto the subset a
//! code generator reads, not a parallel type system.
//!
//! # What a value domain becomes
//!
//! A parameter's inferred [`ValueDomain::OneOf`] is folded into its kind as a
//! literal union here rather than in an emitter. A domain is a fact about the
//! query — `WHERE status = $status` against a `'active' | 'retired'` field
//! admits exactly two values — and a TypeScript union, a Rust enum and a
//! Python `Literal[...]` all want that same fact. Folding it once means no
//! emitter has to know what a `ValueDomain` is.

use serde::{Deserialize, Serialize};
use surrealdb_types::{Kind, KindLiteral, Number, Value};
use surrealql_analyzer_workspace::analysis::{AnalysisOutput, ParamInference, ValueDomain};
use surrealql_analyzer_workspace::schema::SchemaIndex;
use surrealql_analyzer_workspace::PartialReason;

pub use surrealql_analyzer_workspace::schema::FieldStep;

/// Where a document's facts came from.
///
/// Static analysis of `.surql` sources and introspection of a running
/// database can disagree — a migration that has not been applied is exactly
/// that disagreement — so a consumer that merges or caches documents is told
/// which one it is holding rather than having to guess.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// Built from the project's `.surql` sources and host files.
    Static,
    /// Built by introspecting a live database.
    Live,
}

/// Everything the analyzer knows about one project's types.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TypesDocument {
    /// The document format version — [`TypesDocument::VERSION`] for anything
    /// this crate writes. A reader that does not recognise it should refuse
    /// rather than guess.
    pub version: u32,
    /// Where the facts came from.
    pub source: Source,
    /// Every defined table, alphabetically.
    pub tables: Vec<TableTypes>,
    /// Every `DEFINE FUNCTION`, alphabetically by `fn::` path.
    pub functions: Vec<FunctionTypes>,
    /// Every `DEFINE PARAM` global, alphabetically.
    pub params: Vec<ParamTypes>,
    /// Every analyzed query, in discovery order.
    pub queries: Vec<QueryTypes>,
}

/// One table's read shape.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TableTypes {
    /// The table name.
    pub name: String,
    /// Declared fields, in declaration-path order.
    pub fields: Vec<FieldTypes>,
    /// The `TYPE RELATION` edge spec, when the table is a relation.
    pub relation: Option<RelationTypes>,
    /// `DEFINE TABLE ... SCHEMAFULL` — only declared fields are retained. A
    /// `false` here (the default, `SCHEMALESS`) means a row may carry keys no
    /// `DEFINE FIELD` describes, which a language whose emitter renders a
    /// closed shape for every table needs to know: a TypeScript `interface`
    /// with no index signature, for one, is a claim that no such key exists.
    pub schemafull: bool,
    /// `DEFINE TABLE ... DROP` — rows are never retained; a write to this
    /// table is fire-and-forget; there is nothing to read back.
    pub drop_table: bool,
}

/// The tables a relation's `in`/`out` links may point at. Empty means any.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationTypes {
    /// Allowed `in` (source) tables.
    pub in_tables: Vec<String>,
    /// Allowed `out` (destination) tables.
    pub out_tables: Vec<String>,
}

/// One `DEFINE FIELD`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FieldTypes {
    /// The dotted path split into segments (`profile.name` →
    /// `["profile", "name"]`).
    pub path: Vec<String>,
    /// The declaration's **structural** path, which keeps the `[*]` element
    /// steps the dotted path collapses away. This is what lets an emitter
    /// tell `items[*].price` (the price of each item) from `items.price`
    /// (a price on the items object), and it is never empty: a declaration
    /// whose idiom carries no element step gets one [`FieldStep::Field`] per
    /// path segment.
    pub steps: Vec<FieldStep>,
    /// The declared kind, or [`Kind::Any`] for an untyped field.
    pub kind: Kind,
    /// Whether the field's key may be absent — its kind admits `NONE`, or the
    /// kind could not be resolved (see [`partial`](Self::partial)) and so
    /// admits anything, absence included.
    pub optional: bool,
    /// `VALUE`/`COMPUTED` — the database writes this field, not the host.
    pub computed: bool,
    /// `READONLY` — writable only at creation.
    pub readonly: bool,
    /// The field has a `DEFAULT` clause (or a `VALUE` that supplies one): a
    /// write can omit this key and the database fills it in. An
    /// insert/write-shape emitter (Rust's `query!`, Python's dataclass) needs
    /// this to know a field is optional to *write* even when its read kind
    /// does not admit `NONE`.
    pub has_default: bool,
    /// `REFERENCE` — the field's `record<...>` link is a reference, so a `<~`
    /// back-traversal on the target table can resolve through it.
    pub reference: bool,
    /// Why [`kind`](Self::kind) is [`Kind::Any`] and not a real inferred type,
    /// when that is the reason: empty for a field that is genuinely untyped
    /// (no `TYPE` clause at all) as well as for one fully resolved. A
    /// resolved-but-unresolvable field — a reference the analyzer could not
    /// follow, a value only known at runtime, syntax it does not model — is
    /// otherwise indistinguishable from an untyped one, and a consumer that
    /// wants to tell "the schema says nothing" from "the schema said
    /// something the analyzer could not read" needs this to do it.
    pub partial: Vec<PartialReason>,
}

/// One `DEFINE FUNCTION`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FunctionTypes {
    /// The `fn::` path.
    pub name: String,
    /// Declared arguments, in order. A function argument is always required,
    /// so every [`ParamTypes::required`] here is `true`.
    pub args: Vec<ParamTypes>,
    /// The return kind: declared (`-> T`) if written, otherwise inferred from
    /// the body when the body has a concrete type.
    pub returns: Option<Kind>,
}

/// One named parameter: a query's `$name`, a `DEFINE PARAM` global, or a
/// function argument.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParamTypes {
    /// The parameter name, without the leading `$`. A host substitution
    /// (`${...}` in a template) is named `__hostN`, numbered in template
    /// order.
    pub name: String,
    /// The kind the parameter must be, with any enumerable value domain
    /// already folded in as a literal union. `None` when nothing constrains
    /// it.
    pub kind: Option<Kind>,
    /// Whether the caller must supply it — false only when a `DEFINE PARAM`
    /// default covers it.
    pub required: bool,
}

/// One analyzed query: what it returns and what it reads.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QueryTypes {
    /// The template's static parts, in order — one part, and no
    /// substitutions, for a plain string.
    pub parts: Vec<String>,
    /// The analyzed text: [`parts`](Self::parts) rejoined with the
    /// `$__hostN` parameter each hole was bound to. This is the text the
    /// analyzer saw and the text a runtime looks the query up by, byte for
    /// byte.
    pub text: String,
    /// The per-statement response kinds, one per statement in source order.
    /// `None` is a statement that does not respond (`LET`, `DEFINE`, …); the
    /// SurrealDB protocol still returns a slot for it.
    pub statements: Vec<Option<Kind>>,
    /// Every parameter the query reads, including the `__hostN`
    /// substitutions.
    pub params: Vec<ParamTypes>,
}

/// The prefix `crates/embed` gives a host substitution, restated because the
/// dependency runs the other way. `surrealql_analyzer_embed::HOST_PARAM_PREFIX`
/// is the definition; a test in `crate::typescript` pins the two together.
pub(crate) const HOST_PARAM_PREFIX: &str = "__host";

impl TypesDocument {
    /// The format version this crate writes.
    pub const VERSION: u32 = 1;

    /// Builds the document for a project: its schema and its analyzed
    /// queries.
    ///
    /// Ordering is fixed here rather than left to an emitter, because a
    /// generated file that reorders itself between runs is a diff nobody can
    /// read: tables and functions and globals sort by name, a table's fields
    /// keep their declaration-path order, and queries stay in discovery
    /// order.
    pub fn new(source: Source, schema: &SchemaIndex, queries: Vec<QueryTypes>) -> Self {
        let tables = schema
            .tables
            .values()
            .map(|table| TableTypes {
                name: table.name.clone(),
                fields: table.fields.values().map(field_types).collect(),
                relation: table.relation.as_ref().map(|relation| RelationTypes {
                    in_tables: relation.in_tables.clone(),
                    out_tables: relation.out_tables.clone(),
                }),
                schemafull: table.schemafull,
                drop_table: table.drop_table,
            })
            .collect();
        let functions = schema
            .functions
            .values()
            .map(|function| FunctionTypes {
                name: function.name.clone(),
                args: function
                    .args
                    .iter()
                    .map(|arg| ParamTypes {
                        name: arg.name.clone(),
                        kind: arg.kind.clone(),
                        required: true,
                    })
                    .collect(),
                returns: function
                    .return_kind
                    .clone()
                    .or_else(|| function.inferred_return.clone()),
            })
            .collect();
        // A `DEFINE PARAM` carries a value, not a type annotation, so the
        // schema index has no kind for one. It is listed anyway: a consumer
        // that only needs the *names* of the globals (to leave them out of a
        // generated parameter object, say) can read them here, and a kind can
        // be added later without moving the field.
        let params = schema
            .params
            .values()
            .map(|param| ParamTypes {
                name: param.name.clone(),
                kind: None,
                required: false,
            })
            .collect();
        Self {
            version: Self::VERSION,
            source,
            tables,
            functions,
            params,
            queries,
        }
    }
}

impl QueryTypes {
    /// Builds one query's entry from its analysis output: the per-statement
    /// response kinds and the inferred parameters.
    ///
    /// This is the one step between "analyzed" and "described", and it is the
    /// step `generate` runs for every embedded query it finds. It lives here
    /// rather than in the host so `tests/golden.rs` — which compiles the
    /// rendered module with `tsc` — exercises the same code path a user's
    /// `surrealkit generate` does, not a re-implementation of it.
    pub fn from_analysis(parts: Vec<String>, output: &AnalysisOutput) -> Self {
        let parts: Vec<String> = parts.iter().map(|part| normalize_newlines(part)).collect();
        Self {
            text: join_with_holes(&parts),
            parts,
            statements: output
                .statements
                .iter()
                .map(|statement| statement.response_kind.clone())
                .collect(),
            params: output.inferred_params.iter().map(param_types).collect(),
        }
    }
}

/// CRLF and lone CR collapsed to LF, because that is what the runtime will ask
/// for.
///
/// A query key must equal, byte for byte, the string the host language hands
/// the client at run time. For a template literal that string is the *cooked*
/// value, and cooking normalises every line-terminator sequence to LF — so in
/// a host file saved with CRLF endings, a multi-line query reaches `db.query`
/// with `\n` while extraction (which reads the file's bytes) sees `\r\n`. A key
/// carrying the `\r` matches nothing at run time, and the same `\r` ends the
/// generated string literal early, so the file does not even parse.
///
/// This is deliberately **not** done in `crates/embed`: its spans index the
/// original bytes, and the language server maps findings back through them, so
/// rewriting the text there would move every diagnostic by one byte per line.
/// The key is the only thing that needs the cooked spelling, so the key is the
/// only thing that gets it.
fn normalize_newlines(text: &str) -> String {
    if !text.contains('\r') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            out.push('\n');
        } else {
            out.push(character);
        }
    }
    out
}

/// Rebuild the analyzed text from a template's static parts, restoring the
/// `$__hostN` parameter each hole became. `parts` is the text split on those
/// parameters, so interleaving the names back in reverses the split exactly.
fn join_with_holes(parts: &[String]) -> String {
    let mut text = String::new();
    for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            text.push('$');
            text.push_str(HOST_PARAM_PREFIX);
            text.push_str(&(index - 1).to_string());
        }
        text.push_str(part);
    }
    text
}

fn field_types(field: &surrealql_analyzer_workspace::schema::FieldDef) -> FieldTypes {
    let steps = if field.steps.is_empty() {
        field
            .path
            .iter()
            .map(|segment| FieldStep::Field(segment.clone()))
            .collect()
    } else {
        field.steps.clone()
    };
    let kind = field.kind.clone().unwrap_or(Kind::Any);
    FieldTypes {
        path: field.path.clone(),
        steps,
        optional: admits_none(&kind),
        kind,
        computed: field.computed,
        readonly: field.readonly,
        has_default: field.has_default,
        reference: field.reference,
        partial: field.partial.clone(),
    }
}

fn param_types(param: &ParamInference) -> ParamTypes {
    ParamTypes {
        name: param.name.clone(),
        kind: domain_kind(param).or_else(|| param.kind.clone()),
        required: param.required,
    }
}

/// A parameter's enumerable value domain as a kind — the literal union its
/// uses imply. `None` when there is no domain, when it is not enumerable, or
/// when a value in it has no literal spelling, in which case the parameter
/// keeps its inferred kind.
fn domain_kind(param: &ParamInference) -> Option<Kind> {
    let Some(ValueDomain::OneOf(values)) = &param.domain else {
        return None;
    };
    let mut literals: Vec<Kind> = Vec::new();
    for value in values {
        let literal = value_literal(value)?;
        let kind = Kind::Literal(literal);
        if !literals.contains(&kind) {
            literals.push(kind);
        }
    }
    match literals.len() {
        0 => None,
        1 => literals.pop(),
        _ => Some(Kind::Either(literals)),
    }
}

fn value_literal(value: &Value) -> Option<KindLiteral> {
    match value {
        Value::String(text) => Some(KindLiteral::String(text.clone())),
        Value::Bool(flag) => Some(KindLiteral::Bool(*flag)),
        Value::Number(Number::Int(int)) => Some(KindLiteral::Integer(*int)),
        Value::Number(Number::Float(float)) => Some(KindLiteral::Float(*float)),
        Value::Number(Number::Decimal(decimal)) => Some(KindLiteral::Decimal(*decimal)),
        _ => None,
    }
}

/// Whether a kind admits `NONE` — the fact that decides, in every target
/// language, whether the thing holding it may be absent.
///
/// `Kind::Any` counts too. It stands for two different things — a field with
/// no `TYPE` clause at all, and one whose declared type the analyzer could
/// not resolve (see [`FieldTypes::partial`]) — and neither is a promise that
/// the key is always present. An untyped `DEFINE FIELD x` still lets a row
/// omit `x` entirely, so rendering it as a required `x: unknown` would be a
/// stronger claim than the schema makes.
fn admits_none(kind: &Kind) -> bool {
    match kind {
        Kind::None | Kind::Any => true,
        Kind::Either(variants) => variants.iter().any(admits_none),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use surrealql_analyzer_workspace::analysis::ParamInference;

    fn param(name: &str, kind: Option<Kind>, domain: Option<ValueDomain>) -> ParamInference {
        ParamInference {
            name: name.into(),
            kind,
            domain,
            required: true,
            spans: Vec::new(),
        }
    }

    #[test]
    fn a_hole_keys_by_the_parameter_the_analyzer_bound() {
        assert_eq!(join_with_holes(&["SELECT 1".into()]), "SELECT 1");
        assert_eq!(
            join_with_holes(&["WHERE a > ".into(), "".into()]),
            "WHERE a > $__host0"
        );
        assert_eq!(
            join_with_holes(&["a = ".into(), " AND b = ".into(), "".into()]),
            "a = $__host0 AND b = $__host1"
        );
    }

    /// An enumerable domain is a fact about the query, not about TypeScript,
    /// so it is folded into the kind here — where every emitter gets it —
    /// rather than re-derived per language.
    #[test]
    fn an_enumerable_domain_folds_into_a_literal_union() {
        let folded = param_types(&param(
            "status",
            Some(Kind::String),
            Some(ValueDomain::OneOf(vec![
                Value::String("open".into()),
                Value::String("closed".into()),
                Value::String("open".into()),
            ])),
        ));

        assert_eq!(
            folded.kind,
            Some(Kind::Either(vec![
                Kind::Literal(KindLiteral::String("open".into())),
                Kind::Literal(KindLiteral::String("closed".into())),
            ])),
            "duplicates collapse and the union keeps its first-seen order"
        );
    }

    /// A domain nothing can spell as a literal leaves the inferred kind
    /// alone: a widened-to-`string` parameter is honest, a wrong literal
    /// union is not.
    #[test]
    fn an_unspellable_domain_keeps_the_inferred_kind() {
        let folded = param_types(&param(
            "at",
            Some(Kind::Datetime),
            Some(ValueDomain::Range {
                min: Some(0),
                max: None,
            }),
        ));

        assert_eq!(folded.kind, Some(Kind::Datetime));
    }

    /// The document is a wire format: a host that writes one and a tool that
    /// reads it back must see the same facts, including the upstream `Kind`s
    /// it does not re-model.
    #[test]
    fn a_document_survives_a_serialization_round_trip() {
        let document = TypesDocument {
            version: TypesDocument::VERSION,
            source: Source::Static,
            tables: vec![TableTypes {
                name: "person".into(),
                fields: vec![FieldTypes {
                    path: vec!["nick".into()],
                    steps: vec![FieldStep::Field("nick".into())],
                    kind: Kind::Either(vec![Kind::None, Kind::String]),
                    optional: true,
                    computed: false,
                    readonly: false,
                    has_default: true,
                    reference: false,
                    partial: vec![PartialReason::Unresolved],
                }],
                relation: Some(RelationTypes {
                    in_tables: vec!["person".into()],
                    out_tables: vec!["team".into()],
                }),
                schemafull: true,
                drop_table: false,
            }],
            functions: vec![FunctionTypes {
                name: "fn::greet".into(),
                args: vec![ParamTypes {
                    name: "name".into(),
                    kind: Some(Kind::String),
                    required: true,
                }],
                returns: Some(Kind::String),
            }],
            params: vec![ParamTypes {
                name: "tenant".into(),
                kind: None,
                required: false,
            }],
            queries: vec![QueryTypes {
                parts: vec!["SELECT name FROM person WHERE age > ".into(), "".into()],
                text: "SELECT name FROM person WHERE age > $__host0".into(),
                statements: vec![None, Some(Kind::Record(vec!["person".into()]))],
                params: vec![ParamTypes {
                    name: "__host0".into(),
                    kind: Some(Kind::Int),
                    required: true,
                }],
            }],
        };

        let json = serde_json::to_string(&document).expect("the document serializes");
        let parsed: TypesDocument = serde_json::from_str(&json).expect("and deserializes");

        assert_eq!(parsed, document);
    }

    /// A host file saved with CRLF endings must produce the key the runtime
    /// asks for, which is the template literal's cooked value — LF.
    #[test]
    fn a_crlf_host_file_keys_by_the_cooked_text() {
        assert_eq!(
            normalize_newlines("SELECT name\r\nFROM person"),
            "SELECT name\nFROM person"
        );
        // A lone CR is a line terminator too, and cooks to LF just the same.
        assert_eq!(normalize_newlines("a\rb"), "a\nb");
        assert_eq!(normalize_newlines("a\nb"), "a\nb");
        // …and the joined key carries the normalised parts, holes and all.
        assert_eq!(
            join_with_holes(&[
                normalize_newlines("SELECT name\r\nFROM person WHERE age > "),
                normalize_newlines("\r\n")
            ]),
            "SELECT name\nFROM person WHERE age > $__host0\n"
        );
    }

    #[test]
    fn optionality_is_read_off_the_kind() {
        assert!(admits_none(&Kind::Either(vec![Kind::None, Kind::String])));
        assert!(!admits_none(&Kind::String));
        assert!(!admits_none(&Kind::Either(vec![Kind::String, Kind::Int])));
    }

    /// `Kind::Any` covers both a field with no `TYPE` clause and one whose
    /// declared type the analyzer could not resolve. Neither promises the key
    /// is always present, so both must admit `NONE` the same way an explicit
    /// `option<T>` does — otherwise a genuinely absent field renders as a
    /// required `x: unknown`.
    #[test]
    fn an_unresolved_or_untyped_field_admits_none() {
        assert!(admits_none(&Kind::Any));
    }
}
