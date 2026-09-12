//! Prints a sample generated `.d.ts`: builds a [`TypesDocument`] by hand and
//! renders it, so the `Kind` → TypeScript mapping and the shape of the
//! emitted module can be eyeballed without running a whole project through
//! `generate`.

use std::collections::BTreeMap;

use surrealdb_types::{Kind, KindLiteral};
use surrealql_analyzer_codegen::{
    render_types_module, FieldStep, FieldTypes, ParamTypes, QueryTypes, Source, TableTypes,
    TypesDocument,
};

fn field(name: &str, kind: Kind) -> FieldTypes {
    FieldTypes {
        path: vec![name.into()],
        steps: vec![FieldStep::Field(name.into())],
        optional: matches!(&kind, Kind::Either(variants) if variants.contains(&Kind::None)),
        kind,
        computed: false,
        readonly: false,
    }
}

fn main() {
    let mut row = BTreeMap::new();
    row.insert("name".to_string(), Kind::String);
    row.insert("age".to_string(), Kind::Int);
    let rows = Kind::Array(Box::new(Kind::Literal(KindLiteral::Object(row))), None);

    let document = TypesDocument {
        version: TypesDocument::VERSION,
        source: Source::Static,
        tables: vec![TableTypes {
            name: "person".into(),
            fields: vec![
                field("name", Kind::String),
                field("age", Kind::Int),
                field("nick", Kind::Either(vec![Kind::None, Kind::String])),
            ],
            relation: None,
        }],
        functions: Vec::new(),
        params: Vec::new(),
        queries: vec![
            QueryTypes {
                parts: vec![
                    "SELECT name, age FROM person WHERE age > ".into(),
                    "".into(),
                ],
                text: "SELECT name, age FROM person WHERE age > $__host0".into(),
                statements: vec![Some(rows.clone())],
                params: vec![ParamTypes {
                    name: "__host0".into(),
                    kind: Some(Kind::Int),
                    required: true,
                }],
            },
            QueryTypes {
                parts: vec!["SELECT name FROM person WHERE team = $team".into()],
                text: "SELECT name FROM person WHERE team = $team".into(),
                statements: vec![Some(rows)],
                params: vec![ParamTypes {
                    name: "team".into(),
                    kind: Some(Kind::Record(vec!["team".into()])),
                    required: true,
                }],
            },
        ],
    };

    print!("{}", render_types_module(&document));
}
