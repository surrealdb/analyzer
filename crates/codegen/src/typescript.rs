//! The TypeScript emitter: a [`TypesDocument`] rendered as a **types-only**
//! module.
//!
//! The file this writes is a `.d.ts` and contains nothing but types — one
//! `interface` per table, a `Tables` map, and a `Queries` type keyed by exact
//! query text. It declares no values, re-exports no runtime, and augments no
//! module. A consumer parameterises the client with it:
//!
//! ```ts
//! import { createClient } from "@surrealdb/analyzer-client";
//! import type { Queries } from "./surrealql-analyzer";
//!
//! export const db = createClient<Queries>({ url: "ws://localhost:8000/rpc" });
//! const [people] = await db.query("SELECT name FROM person");
//! ```
//!
//! The previous emitter wrote a `.ts` that re-exported `createClient` and the
//! SDK value classes and ended in
//! `declare module "@surrealdb/analyzer-client" { interface SurqlRegistry { … } }`.
//! That bought one import at a real cost: a module augmentation is only an
//! augmentation if its target resolves, and when it did not, TypeScript
//! reported the failure **inside the generated file** — which nobody opens —
//! and dropped the whole registry, leaving every typed query silently `any`.
//! A type argument fails where the user wrote it instead.
//!
//! A user who wants the un-parameterised spellings back — `db.query("…")` on a
//! client built without the type argument, or Svelte's `<Query q="…">` markup
//! form — opts in with one line of their own code:
//!
//! ```ts
//! declare module "@surrealdb/analyzer-client" {
//!   interface SurqlRegistry extends Queries {}
//! }
//! ```
//!
//! This file never emits that line. It is the user's decision, in the user's
//! module, where a broken import is an error they can see.
//!
//! # Why `Queries` is a type alias and the tables are interfaces
//!
//! `createClient<Q extends SurqlRegistryShape>` constrains its argument to
//! `Record<string, …>`, and **an interface never satisfies an index
//! signature**: TypeScript grants an implicit index signature to object type
//! literals and not to interfaces, because an interface can be reopened. So
//! `Queries` is a type alias. The table types are interfaces, which is what a
//! user wants in their own signatures: a shorter name in errors, and a shape
//! they can extend.

use std::collections::{BTreeSet, HashMap};

use surrealdb_types::Kind;

use crate::document::{FieldStep, FieldTypes, TableTypes, TypesDocument, HOST_PARAM_PREFIX};
use crate::{ts_type, TsContext};

/// The npm package the generated module imports its value types from.
pub const CLIENT_PACKAGE: &str = "@surrealdb/analyzer-client";

/// The value types the generated module may need from [`CLIENT_PACKAGE`], in
/// import order. Only the ones a document actually mentions are imported: an
/// unused import in a `.d.ts` is noise at best and a `noUnusedLocals` failure
/// in a strict project at worst.
const VALUE_TYPES: [&str; 5] = ["Decimal", "Duration", "GeoJSON", "RecordId", "Uuid"];

/// Names a table's interface must not take.
///
/// Three groups, and all three are real: the names this module defines itself
/// (`Tables`, `Queries`), the value types it imports (`RecordId`, `Uuid`, …),
/// and the globals the emitted types are written in terms of. That last group
/// is the one that bites — a table called `date`, `record` or `array`
/// `PascalCase`s to `Date`, `Record` or `Array`, and an interface of that name
/// in the same file SHADOWS the global for every type below it. The file still
/// compiles, and `joined: Date` now means the user's table. A `Person` whose
/// `joined` is a `person` row is not a type error anyone will debug quickly,
/// so the collision is resolved by suffix instead.
///
/// Everything `ts_type` can emit is here, plus the handful of globals a
/// consumer of a generated type routinely writes (`Partial`, `Readonly`,
/// `Promise`, …). The cost of an unnecessary entry is one digit in a name;
/// the cost of a missing one is a silently wrong type.
const RESERVED_NAMES: [&str; 22] = [
    // Defined here.
    "Queries",
    "Tables",
    // Imported from the client package.
    "Decimal",
    "Duration",
    "GeoJSON",
    "RecordId",
    "Uuid",
    // Emitted by `ts_type`, or routinely wrapped around what it emits.
    "Array",
    "Boolean",
    "Date",
    "Json",
    "Map",
    "Number",
    "Object",
    "Partial",
    "Promise",
    "Readonly",
    "Record",
    "Set",
    "String",
    "Symbol",
    "Uint8Array",
];

/// A rendered module, and what it needs from [`CLIENT_PACKAGE`].
///
/// The imports are returned rather than left to be found in the text, because
/// the one caller that cares — the "`@surrealdb/analyzer-client` is not
/// installed" warning — cannot tell them apart from a mention. The header
/// names the package twice in prose, so `text.contains(CLIENT_PACKAGE)` is
/// true of every module this crate has ever emitted, including one that
/// imports nothing at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratedModule {
    /// The module text, ready to write.
    pub text: String,
    /// The value types imported from [`CLIENT_PACKAGE`], in import order.
    /// Empty when the module needs none — a project with no tables and no
    /// queries, where the package's absence would cost nothing.
    pub imports: Vec<&'static str>,
}

/// Renders the complete `.d.ts`.
pub fn render_types_module(document: &TypesDocument) -> GeneratedModule {
    let names = interface_names(&document.tables);
    let mut used = BTreeSet::new();
    let mut body = String::new();

    for table in &document.tables {
        body.push_str(&render_table(table, &names, &mut used));
        body.push('\n');
    }

    body.push_str("/** Every table in the schema, keyed by its SurrealQL name. */\n");
    if document.tables.is_empty() {
        body.push_str("export interface Tables {}\n");
    } else {
        body.push_str("export interface Tables {\n");
        for table in &document.tables {
            body.push_str(&format!(
                "  {}: {};\n",
                object_key(&table.name),
                names[&table.name]
            ));
        }
        body.push_str("}\n");
    }

    body.push('\n');
    body.push_str(&render_queries(document, &mut used));

    // In `VALUE_TYPES` order, not in the set's: the import list is part of the
    // file's stable shape, and a name's position in it must not depend on
    // which table happened to use it first.
    let imports: Vec<&'static str> = VALUE_TYPES
        .iter()
        .filter(|name| used.contains(*name))
        .copied()
        .collect();
    let import_line = if imports.is_empty() {
        String::new()
    } else {
        format!(
            "import type {{ {} }} from \"{CLIENT_PACKAGE}\";\n\n",
            imports.join(", ")
        )
    };

    GeneratedModule {
        text: format!("{}{import_line}{body}", header()),
        imports,
    }
}

fn header() -> String {
    format!(
        r#"// Generated by surrealql-analyzer — do not edit.
//
// Types only: nothing in this file exists at runtime. The client is a separate
// import, parameterised by what is declared here.
//
//   import {{ createClient }} from "{CLIENT_PACKAGE}";
//   import type {{ Queries }} from "./surrealql-analyzer";
//
//   export const db = createClient<Queries>({{ url }});
//   const [people] = await db.query("SELECT name FROM person");
//
// Value conventions name what the SurrealDB SDK actually decodes, which is its
// own value classes: record links are `RecordId<"table">`, uuids are `Uuid`,
// durations are `Duration`, decimals are `Decimal`. Datetimes are a native
// `Date` because `createClient` sets `codecOptions.useNativeDates`. `NONE`
// fields are optional keys. Anything crossing a serialisation boundary (SSR, a
// `fetch` response, `JSON.stringify`) goes through `Json<T>`, which maps
// `RecordId<"team">` to `` `team:${{string}}` `` and `Date` to `string`.
//
// The classes matter beyond reading: a `RecordId` parameter encodes to a
// record link on the wire, while a plain string encodes to a SurrealQL
// string, so `WHERE team = $team` matches only with the class.
//
// To type `db.query("…")` on a client built WITHOUT the type argument — and
// the Svelte `<Query q="…">` markup form, which has no call to put one on —
// add this once, in your own code:
//
//   declare module "{CLIENT_PACKAGE}" {{
//     interface SurqlRegistry extends Queries {{}}
//   }}

"#
    )
}

/// One table's interface.
///
/// `id` leads, because every record has one and it is the field a reader
/// looks for first; a relation's `in`/`out` follow, because they are the rest
/// of what the engine supplies rather than what the schema declares. Then the
/// declared fields, in declaration-path order. A field the schema declares
/// under one of those names wins — the generator does not get to overrule a
/// `DEFINE FIELD`.
fn render_table(
    table: &TableTypes,
    names: &HashMap<String, String>,
    used: &mut BTreeSet<&'static str>,
) -> String {
    let mut members: Vec<String> = Vec::new();
    let declared: BTreeSet<&str> = table
        .fields
        .iter()
        .filter(|field| field.path.len() == 1)
        .map(|field| field.path[0].as_str())
        .collect();

    if !declared.contains("id") {
        members.push(format!(
            "  id: {};",
            value_text(&Kind::Record(vec![table.name.clone().into()]), used)
        ));
    }
    if let Some(relation) = &table.relation {
        for (name, tables) in [("in", &relation.in_tables), ("out", &relation.out_tables)] {
            if !declared.contains(name) {
                members.push(format!(
                    "  {name}: {};",
                    value_text(
                        &Kind::Record(tables.iter().map(|table| table.clone().into()).collect()),
                        used,
                    )
                ));
            }
        }
    }
    for (key, node) in build_tree(&table.fields).fields {
        members.push(format!("  {}", render_member(&key, &node, used)));
    }

    format!(
        "export interface {} {{\n{}\n}}\n",
        names[&table.name],
        members.join("\n")
    )
}

/// The query registry: one row per analyzed query, keyed by its exact text.
fn render_queries(document: &TypesDocument, used: &mut BTreeSet<&'static str>) -> String {
    let mut rows = Vec::new();
    let mut seen = BTreeSet::new();
    for query in &document.queries {
        // A query carrying substitutions keys by the text the analyzer saw,
        // holes and all: a Svelte markup attribute reaches the runtime through
        // the preprocessor, which binds these same names, so the key it looks
        // up is this string byte for byte. Keying on a hole-shaped placeholder
        // instead would spell a key nothing ever asks for.
        if !seen.insert(query.text.clone()) {
            continue;
        }
        rows.push(format!(
            "  {}: {{ result: {}; params: {} }};",
            ts_string(&query.text),
            response_tuple(&query.statements, used),
            params_object(&query.params, used),
        ));
    }

    let mut out = String::from(
        "/**\n \
         * Every analyzed query, keyed by its exact text. Hand it to\n \
         * `createClient<Queries>(…)`; a key that is not here is a type error at\n \
         * the call, carrying its own remedy.\n \
         */\n",
    );
    if rows.is_empty() {
        out.push_str("// No embedded queries were found in this project.\n");
        out.push_str("export type Queries = {};\n");
    } else {
        out.push_str("export type Queries = {\n");
        out.push_str(&rows.join("\n"));
        out.push_str("\n};\n");
    }
    out
}

/// The per-statement response tuple. The SurrealDB SDK returns one result per
/// statement, in source order, so the tuple has one element per statement: a
/// responding statement contributes its rendered kind, a non-responder (`LET`,
/// `DEFINE`, …) contributes `null`.
///
/// Every element is a [`TsContext::Value`]: a tuple slot has no key to omit,
/// so an `option<T>` result stays `undefined | T` rather than becoming an
/// optional slot — dropping it would shorten the tuple.
fn response_tuple(statements: &[Option<Kind>], used: &mut BTreeSet<&'static str>) -> String {
    let elements: Vec<String> = statements
        .iter()
        .map(|kind| {
            kind.as_ref()
                .map_or_else(|| "null".into(), |kind| value_text(kind, used))
        })
        .collect();
    format!("[{}]", elements.join(", "))
}

/// The named-parameter object for a query. `__hostN` substitutions are left
/// out: they are filled by the template, not by the caller.
///
/// This is an object type, but **not** a [`TsContext::Property`] position:
/// the `?` here answers "does the query need this argument at all" —
/// `required` is cleared only by a `DEFINE PARAM` default — while a
/// `Property`'s `?` answers "can the value be absent". They are different
/// questions with the same syntax, so the kind is rendered as a value and the
/// marker is the parameter's own. Folding a parameter's `option<T>` into the
/// key as well would change the type (it would let callers omit a key the
/// query requires), not just its spelling.
fn params_object(
    params: &[crate::document::ParamTypes],
    used: &mut BTreeSet<&'static str>,
) -> String {
    let mut parts = Vec::new();
    for param in params {
        if param.name.starts_with(HOST_PARAM_PREFIX) {
            continue;
        }
        let marker = if param.required { "" } else { "?" };
        let text = param
            .kind
            .as_ref()
            .map_or_else(|| "unknown".into(), |kind| value_text(kind, used));
        parts.push(format!("{}{marker}: {text}", object_key(&param.name)));
    }
    if parts.is_empty() {
        "Record<string, never>".into()
    } else {
        format!("{{ {} }}", parts.join("; "))
    }
}

/// A field tree: the structural shape a table's flat `DEFINE FIELD` list
/// describes.
///
/// `DEFINE FIELD settings.timezone` and `DEFINE FIELD items[*].price` are
/// separate declarations that name positions *inside* another field, and
/// TypeScript has no flat spelling for either. The steps a declaration
/// carries say which: a `Field` step descends into an object member, an
/// `Element` step into a collection's element.
#[derive(Default)]
struct Node {
    kind: Option<Kind>,
    optional: bool,
    fields: Vec<(String, Node)>,
    element: Option<Box<Node>>,
}

impl Node {
    fn child(&mut self, name: &str) -> &mut Node {
        if let Some(index) = self
            .fields
            .iter()
            .position(|(existing, _)| existing == name)
        {
            return &mut self.fields[index].1;
        }
        self.fields.push((name.to_string(), Node::default()));
        &mut self.fields.last_mut().expect("just pushed").1
    }

    fn element(&mut self) -> &mut Node {
        self.element
            .get_or_insert_with(|| Box::new(Node::default()))
    }
}

fn build_tree(fields: &[FieldTypes]) -> Node {
    let mut root = Node::default();
    for field in fields {
        let mut node = &mut root;
        for step in &field.steps {
            node = match step {
                FieldStep::Field(name) => node.child(name),
                FieldStep::Element => node.element(),
            };
        }
        node.kind = Some(field.kind.clone());
        node.optional = field.optional;
    }
    root
}

/// One object member: `key: type;`, or `key?: type;` when the field's kind
/// admits `NONE`.
fn render_member(key: &str, node: &Node, used: &mut BTreeSet<&'static str>) -> String {
    let marker = if node.optional { "?" } else { "" };
    format!("{}{marker}: {};", object_key(key), render_node(node, used))
}

/// A node's type. A node with structure below it is rendered from that
/// structure — a `DEFINE FIELD settings TYPE object` says nothing a reader can
/// use, while its subfields say everything — and a leaf is rendered from its
/// declared kind.
fn render_node(node: &Node, used: &mut BTreeSet<&'static str>) -> String {
    if let Some(element) = &node.element {
        return format!("Array<{}>", render_node(element, used));
    }
    if !node.fields.is_empty() {
        let members: Vec<String> = node
            .fields
            .iter()
            .map(|(key, child)| {
                let rendered = render_member(key, child, used);
                rendered
                    .strip_suffix(';')
                    .map_or(rendered.clone(), ToString::to_string)
            })
            .collect();
        return format!("{{ {} }}", members.join("; "));
    }
    node.kind
        .as_ref()
        .map_or_else(|| "unknown".into(), |kind| property_text(kind, used))
}

/// A kind in a value position: `option<string>` is `undefined | string`,
/// because there is no key to omit.
fn value_text(kind: &Kind, used: &mut BTreeSet<&'static str>) -> String {
    note(ts_type(kind, TsContext::Value).text, used)
}

/// A kind in a property position, with the `?` already accounted for by the
/// member's own `optional` flag: the `none` is folded away here so it is not
/// spelled twice.
fn property_text(kind: &Kind, used: &mut BTreeSet<&'static str>) -> String {
    note(ts_type(kind, TsContext::Property).text, used)
}

/// Records which value types a rendered fragment names, and hands the fragment
/// back.
///
/// Every name is recorded where it is *produced*, not found afterwards in the
/// finished file: the file also contains the query keys, which are arbitrary
/// user text. A query that happens to select a column called `uuid` would
/// otherwise add `Uuid` to the imports — an unused import in a `.d.ts`, which
/// a strict project reports.
fn note(text: String, used: &mut BTreeSet<&'static str>) -> String {
    for name in VALUE_TYPES {
        if !used.contains(name) && mentions(&text, name) {
            used.insert(name);
        }
    }
    text
}

/// A stable TypeScript interface name per table: `PascalCase`, never colliding
/// with a name this module already defines, and never with another table's.
///
/// Collisions are resolved by suffixing rather than by mangling, and the
/// suffix goes to whichever table sorts later, so the answer does not depend
/// on discovery order.
fn interface_names(tables: &[TableTypes]) -> HashMap<String, String> {
    let mut taken: BTreeSet<String> = RESERVED_NAMES.iter().map(ToString::to_string).collect();
    let mut names = HashMap::new();
    let mut sorted: Vec<&TableTypes> = tables.iter().collect();
    sorted.sort_by(|left, right| left.name.cmp(&right.name));
    for table in sorted {
        let base = pascal_case(&table.name);
        let mut name = base.clone();
        let mut suffix = 2;
        while !taken.insert(name.clone()) {
            name = format!("{base}{suffix}");
            suffix += 1;
        }
        names.insert(table.name.clone(), name);
    }
    names
}

/// `person` → `Person`, `team_member` → `TeamMember`, `2fa` → `T2fa`.
fn pascal_case(name: &str) -> String {
    let mut out = String::new();
    for word in name.split(|c: char| !c.is_ascii_alphanumeric()) {
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    match out.chars().next() {
        None => "Table".into(),
        // A TypeScript identifier cannot start with a digit.
        Some(first) if first.is_ascii_digit() => format!("T{out}"),
        Some(_) => out,
    }
}

/// An object key, quoted when it is not a plain identifier.
fn object_key(name: &str) -> String {
    if is_identifier(name) {
        name.to_string()
    } else {
        ts_string(name)
    }
}

fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_' || c == '$')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

/// One string, as a TypeScript double-quoted literal.
///
/// Every character a string literal cannot hold raw is escaped, not just the
/// three that are common. A query key is arbitrary user text, and the three
/// classes below each produced a file that does not parse — reported as
/// TS1002 *inside the generated file*, after `generate` said it succeeded:
///
/// * `\r`, from a host file with CRLF line endings. (The key itself no longer
///   carries one — see `QueryTypes::from_analysis` — but a `\r` can still
///   reach here inside a literal string in the query.)
/// * U+2028 LINE SEPARATOR and U+2029 PARAGRAPH SEPARATOR, which are line
///   terminators in JavaScript and so end a string literal exactly as a
///   newline does.
/// * the rest of the C0 controls and DEL, which the grammar does allow raw but
///   which no reader or diff tool survives; they go out as `\u00XX`.
fn ts_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            control if control.is_control() && (control as u32) < 0x80 => {
                out.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// Whether a rendered type fragment names a value type, as a whole word — so
/// a field called `recordIdentifier` does not import `RecordId`.
fn mentions(body: &str, name: &str) -> bool {
    body.match_indices(name).any(|(index, _)| {
        let before = body[..index].chars().next_back();
        let after = body[index + name.len()..].chars().next();
        !before.is_some_and(is_word_char) && !after.is_some_and(is_word_char)
    })
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '$'
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{
        FieldTypes, ParamTypes, QueryTypes, RelationTypes, Source, TableTypes, TypesDocument,
    };
    use surrealdb_types::KindLiteral;

    fn field(path: &str, kind: Kind) -> FieldTypes {
        let segments: Vec<String> = path.split('.').map(ToString::to_string).collect();
        let steps = segments
            .iter()
            .map(|segment| FieldStep::Field(segment.clone()))
            .collect();
        let optional = matches!(&kind, Kind::Either(variants) if variants.contains(&Kind::None));
        FieldTypes {
            path: segments,
            steps,
            kind,
            optional,
            computed: false,
            readonly: false,
        }
    }

    /// The module text. Every assertion below is about what the file says;
    /// the import list is checked on its own, in
    /// `only_the_value_types_a_type_names_are_imported`.
    fn render(document: &TypesDocument) -> String {
        render_types_module(document).text
    }

    fn document(tables: Vec<TableTypes>, queries: Vec<QueryTypes>) -> TypesDocument {
        TypesDocument {
            version: TypesDocument::VERSION,
            source: Source::Static,
            tables,
            functions: Vec::new(),
            params: Vec::new(),
            queries,
        }
    }

    /// The rendered module with its comments stripped. The header explains
    /// the opt-in `declare module` a user may write in their OWN code, so a
    /// bare substring search cannot tell "the file mentions it" from "the file
    /// does it".
    fn code(rendered: &str) -> String {
        rendered
            .lines()
            .filter(|line| {
                let trimmed = line.trim_start();
                !(trimmed.starts_with("//") || trimmed.starts_with('*'))
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn query(text: &str, statements: Vec<Option<Kind>>, params: Vec<ParamTypes>) -> QueryTypes {
        QueryTypes {
            parts: vec![text.into()],
            text: text.into(),
            statements,
            params,
        }
    }

    #[test]
    fn the_restated_host_prefix_still_matches_extraction() {
        // Generation restates this rather than depending on extraction. If the
        // definition moves, every generated key silently stops matching the
        // text the runtime asks for, and results degrade to `unknown` with no
        // error anywhere — so fail here instead.
        assert_eq!(
            HOST_PARAM_PREFIX,
            surrealql_analyzer_embed::HOST_PARAM_PREFIX
        );
    }

    /// The emitted module is types only. No `import`/`export` of a value, and
    /// above all no module augmentation: that is the whole point of the
    /// format.
    #[test]
    fn the_module_declares_no_runtime_and_augments_nothing() {
        let rendered = render(&document(
            vec![TableTypes {
                name: "person".into(),
                fields: vec![field("name", Kind::String)],
                relation: None,
            }],
            vec![query("SELECT name FROM person", vec![None], Vec::new())],
        ));

        let code = code(&rendered);
        assert!(!code.contains("declare module"), "{rendered}");
        assert!(!code.contains("export {"), "{rendered}");
        assert!(!code.contains("export const"), "{rendered}");
        assert!(
            rendered.contains("import type { RecordId } from \"@surrealdb/analyzer-client\";"),
            "only the value types the module mentions are imported: {rendered}"
        );
    }

    #[test]
    fn a_table_becomes_an_interface_with_its_id() {
        let rendered = render(&document(
            vec![TableTypes {
                name: "person".into(),
                fields: vec![
                    field("name", Kind::String),
                    field("nick", Kind::Either(vec![Kind::None, Kind::String])),
                ],
                relation: None,
            }],
            Vec::new(),
        ));

        assert!(
            rendered.contains(
                "export interface Person {\n  \
                 id: RecordId<\"person\">;\n  \
                 name: string;\n  \
                 nick?: string;\n}"
            ),
            "{rendered}"
        );
        assert!(rendered.contains("export interface Tables {\n  person: Person;\n}"));
    }

    /// A relation's `in`/`out` are not declared fields, but they are on every
    /// row the engine returns, and they are typed by the edge spec.
    #[test]
    fn a_relation_table_carries_its_edge_links() {
        let rendered = render(&document(
            vec![TableTypes {
                name: "knows".into(),
                fields: vec![field("since", Kind::Datetime)],
                relation: Some(RelationTypes {
                    in_tables: vec!["person".into()],
                    out_tables: vec!["person".into()],
                }),
            }],
            Vec::new(),
        ));

        assert!(
            rendered.contains(
                "export interface Knows {\n  \
                 id: RecordId<\"knows\">;\n  \
                 in: RecordId<\"person\">;\n  \
                 out: RecordId<\"person\">;\n  \
                 since: Date;\n}"
            ),
            "{rendered}"
        );
    }

    /// The flat `DEFINE FIELD` list is a tree. `address.line1` is a member of
    /// an object, `items[*].price` the member of an array's element — and
    /// TypeScript has no flat spelling for either.
    #[test]
    fn subfields_fold_into_nested_objects_and_arrays() {
        let mut price = field("items.price", Kind::Decimal);
        price.steps = vec![
            FieldStep::Field("items".into()),
            FieldStep::Element,
            FieldStep::Field("price".into()),
        ];
        let rendered = render(&document(
            vec![TableTypes {
                name: "order".into(),
                fields: vec![
                    field("address", Kind::Object),
                    field("address.line1", Kind::String),
                    field("address.city", Kind::String),
                    field("items", Kind::Array(Box::new(Kind::Object), None)),
                    price,
                ],
                relation: None,
            }],
            Vec::new(),
        ));

        assert!(
            rendered.contains("  address: { line1: string; city: string };"),
            "{rendered}"
        );
        assert!(
            rendered.contains("  items: Array<{ price: Decimal }>;"),
            "{rendered}"
        );
    }

    #[test]
    fn a_query_row_carries_its_response_tuple_and_params() {
        let rendered = render(&document(
            Vec::new(),
            vec![query(
                "SELECT name FROM person WHERE team = $team",
                vec![Some(Kind::Array(
                    Box::new(Kind::Literal(KindLiteral::Object(
                        [("name".to_string(), Kind::String)].into_iter().collect(),
                    ))),
                    None,
                ))],
                vec![ParamTypes {
                    name: "team".into(),
                    kind: Some(Kind::Record(vec!["team".into()])),
                    required: true,
                }],
            )],
        ));

        assert!(
            rendered.contains(
                "  \"SELECT name FROM person WHERE team = $team\": \
                 { result: [Array<{ name: string }>]; params: { team: RecordId<\"team\"> } };"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains("export type Queries = {"),
            "`Queries` is a type alias, not an interface: an interface never \
             satisfies the `Record<string, …>` constraint on `createClient`"
        );
    }

    /// A `__hostN` substitution is filled by the template, so it is not an
    /// argument the caller passes and must not appear in the params object.
    #[test]
    fn a_substitution_is_not_a_caller_parameter() {
        let rendered = render(&document(
            Vec::new(),
            vec![QueryTypes {
                parts: vec!["SELECT name FROM person WHERE age > ".into(), "".into()],
                text: "SELECT name FROM person WHERE age > $__host0".into(),
                statements: vec![Some(Kind::Array(Box::new(Kind::Object), None))],
                params: vec![ParamTypes {
                    name: "__host0".into(),
                    kind: Some(Kind::Int),
                    required: true,
                }],
            }],
        ));

        assert!(
            rendered.contains(
                "$__host0\": { result: [Array<Record<string, unknown>>]; \
                               params: Record<string, never> };"
            ),
            "{rendered}"
        );
    }

    /// The two spellings of one `option<string>`, side by side in the file a
    /// consumer imports: inside a row the `none` is a key that may be missing,
    /// and as a statement's whole result there is no key, so it is a union
    /// member. Same kind, same file, two spellings.
    #[test]
    fn an_optional_field_and_an_optional_result_spell_differently() {
        let optional = Kind::Either(vec![Kind::None, Kind::String]);
        let row = Kind::Array(
            Box::new(Kind::Literal(KindLiteral::Object(
                [
                    ("name".to_string(), Kind::String),
                    ("nick".to_string(), optional.clone()),
                ]
                .into_iter()
                .collect(),
            ))),
            None,
        );
        let rendered = render(&document(
            Vec::new(),
            vec![
                query("SELECT name, nick FROM person", vec![Some(row)], Vec::new()),
                query(
                    "SELECT VALUE nick FROM ONLY person:jane",
                    vec![Some(optional)],
                    Vec::new(),
                ),
            ],
        ));

        assert!(
            rendered.contains(
                "\"SELECT name, nick FROM person\": \
                 { result: [Array<{ name: string; nick?: string }>]; \
                 params: Record<string, never> };"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "\"SELECT VALUE nick FROM ONLY person:jane\": \
                 { result: [undefined | string]; \
                 params: Record<string, never> };"
            ),
            "{rendered}"
        );
    }

    /// The params object looks like a property position and is not one. Its
    /// `?` says the query has a default for the argument; the kind's `none`
    /// says the query accepts a NONE *value*. Folding the second into the
    /// first would let a caller omit a key the query requires — a change of
    /// type, not of spelling — so the kind renders as a value.
    #[test]
    fn a_required_param_that_accepts_none_keeps_its_key() {
        let params = vec![
            ParamTypes {
                name: "nick".into(),
                kind: Some(Kind::Either(vec![Kind::None, Kind::String])),
                required: true,
            },
            ParamTypes {
                name: "status".into(),
                kind: Some(Kind::String),
                required: false,
            },
        ];

        assert_eq!(
            params_object(&params, &mut BTreeSet::new()),
            "{ nick: undefined | string; status?: string }"
        );
    }

    /// A key is arbitrary user text, and three classes of character used to
    /// end the generated string literal early — a file that does not parse,
    /// reported inside the generated file after `generate` said it succeeded.
    #[test]
    fn every_character_a_string_literal_cannot_hold_is_escaped() {
        assert_eq!(ts_string("plain"), "\"plain\"");
        assert_eq!(ts_string("say \"hi\""), "\"say \\\"hi\\\"\"");
        assert_eq!(ts_string("back\\slash"), "\"back\\\\slash\"");
        assert_eq!(ts_string("one\ntwo"), "\"one\\ntwo\"");
        // A CR from a CRLF host file, a tab, and the two Unicode line
        // terminators JavaScript recognises inside a string.
        assert_eq!(ts_string("one\r\ntwo"), "\"one\\r\\ntwo\"");
        assert_eq!(ts_string("a\tb"), "\"a\\tb\"");
        assert_eq!(ts_string("a\u{2028}b"), "\"a\\u2028b\"");
        assert_eq!(ts_string("a\u{2029}b"), "\"a\\u2029b\"");
        // The rest of the C0 controls, and DEL.
        assert_eq!(ts_string("a\u{0}b"), "\"a\\u0000b\"");
        assert_eq!(ts_string("a\u{1b}b"), "\"a\\u001bb\"");
        assert_eq!(ts_string("a\u{7f}b"), "\"a\\u007fb\"");
        // Non-ASCII text is not a hazard and stays readable.
        assert_eq!(ts_string("naïve → ok"), "\"naïve → ok\"");
    }

    #[test]
    fn duplicate_query_texts_emit_one_row() {
        let entry = || query("SELECT 1 FROM person", vec![Some(Kind::Int)], Vec::new());
        let rendered = render(&document(Vec::new(), vec![entry(), entry()]));

        assert_eq!(rendered.matches("SELECT 1 FROM person").count(), 1);
    }

    /// Only the value types a rendered TYPE names are imported. The file also
    /// holds the query keys, which are arbitrary user text — a query selecting
    /// a column called `uuid` must not add `Uuid` to the imports, because an
    /// unused import in a `.d.ts` is what a strict project reports.
    #[test]
    fn only_the_value_types_a_type_names_are_imported() {
        let module = render_types_module(&document(
            vec![TableTypes {
                name: "person".into(),
                fields: vec![field("tenure", Kind::Duration)],
                relation: None,
            }],
            vec![query(
                "SELECT uuid, Decimal FROM person",
                vec![Some(Kind::String)],
                Vec::new(),
            )],
        ));

        // `RecordId` from the synthesized `id`, `Duration` from the field —
        // and neither of the two names the query key happens to contain.
        assert_eq!(module.imports, vec!["Duration", "RecordId"]);
        assert!(
            module.text.contains(
                "import type { Duration, RecordId } from \"@surrealdb/analyzer-client\";"
            ),
            "{}",
            module.text
        );
    }

    /// An empty `Queries` must be an empty *object type*, not
    /// `Record<string, never>`: `keyof Record<string, never>` is `string`, so
    /// every query text would "hit" the registry and resolve to `never`
    /// instead of erroring as a miss.
    #[test]
    fn an_empty_project_emits_an_empty_object_type() {
        let rendered = render(&document(Vec::new(), Vec::new()));

        assert!(rendered.contains("export type Queries = {};"), "{rendered}");
        assert!(
            rendered.contains("export interface Tables {}"),
            "{rendered}"
        );
        assert!(
            !code(&rendered).contains("import"),
            "a document that names no value type imports none: {rendered}"
        );
        assert!(render_types_module(&document(Vec::new(), Vec::new()))
            .imports
            .is_empty());
    }

    /// Two table names can `PascalCase` to the same identifier, and a table can
    /// be called `Tables`. Neither may produce a module that does not compile.
    #[test]
    fn interface_names_never_collide() {
        let table = |name: &str| TableTypes {
            name: name.into(),
            fields: Vec::new(),
            relation: None,
        };
        let names = interface_names(&[table("team_member"), table("teamMember"), table("tables")]);

        // The suffix goes to whichever table sorts later by name, so the
        // answer never depends on discovery order.
        assert_eq!(names["teamMember"], "TeamMember");
        assert_eq!(names["team_member"], "TeamMember2");
        assert_eq!(names["tables"], "Tables2");
    }

    /// A table named after a type the emitted file is written in terms of.
    /// `export interface Date { … }` shadows the global for the rest of the
    /// file, so `joined: Date` would quietly mean the user's table instead of
    /// a datetime — and it would still compile.
    #[test]
    fn a_table_never_shadows_a_type_the_file_is_written_in() {
        let table = |name: &str| TableTypes {
            name: name.into(),
            fields: Vec::new(),
            relation: None,
        };
        let names = interface_names(&[
            table("date"),
            table("record"),
            table("array"),
            table("uint8_array"),
            table("json"),
            table("promise"),
            table("partial"),
            table("readonly"),
            table("object"),
            table("string"),
            table("number"),
            table("boolean"),
            table("symbol"),
            table("map"),
            table("set"),
            table("GeoJSON"),
            table("record_id"),
            table("uuid"),
            table("duration"),
            table("decimal"),
            table("queries"),
        ]);

        for (table, name) in &names {
            assert!(
                name.ends_with('2'),
                "`{table}` PascalCases onto a reserved name and must be suffixed, got `{name}`"
            );
        }

        // Case matters, and TypeScript agrees: `Uint8array` shadows nothing,
        // so it is not a collision and takes no suffix.
        assert_eq!(
            interface_names(&[table("uint8array")])["uint8array"],
            "Uint8array"
        );

        // …and the rendered file really does use the suffixed name on both
        // sides, so a `Date` field still means a datetime.
        let rendered = render(&document(
            vec![TableTypes {
                name: "date".into(),
                fields: vec![field("at", Kind::Datetime)],
                relation: None,
            }],
            Vec::new(),
        ));
        assert!(rendered.contains("export interface Date2 {"), "{rendered}");
        assert!(rendered.contains("  at: Date;"), "{rendered}");
        assert!(rendered.contains("  date: Date2;"), "{rendered}");
    }

    #[test]
    fn a_name_that_is_not_an_identifier_is_quoted() {
        let rendered = render(&document(
            vec![TableTypes {
                name: "user-account".into(),
                fields: vec![field("full name", Kind::String)],
                relation: None,
            }],
            Vec::new(),
        ));

        assert!(
            rendered.contains("export interface UserAccount {"),
            "{rendered}"
        );
        assert!(rendered.contains("  \"full name\": string;"), "{rendered}");
        assert!(
            rendered.contains("  \"user-account\": UserAccount;"),
            "{rendered}"
        );
    }
}
