//! The relation the oracle is built on: `value ⊑ kind`, with a precision
//! account for the positions where the kind admits more than the value needed.
//!
//! Equality is the wrong relation between an inferred type and an observed
//! value. A value is one inhabitant of a type, so the only thing a single
//! observation can falsify is *inhabitation*. Everything else the comparison
//! produces — "the kind was `any` here", "the union had four members and one
//! was used" — is a precision signal, not a failure.
//!
//! [`surrealdb_types::Value::is_kind`] is already exactly `⊑`, and it is the
//! engine's own definition, so it cannot drift from the engine. This module
//! delegates to it for every leaf and diverges in exactly one place, described
//! on [`check`].

use surrealdb_types::{Kind, KindLiteral, Value};
use surrealql_analyzer_workspace::render_kind;

/// The *most precise* kind that describes `value`.
///
/// [`surrealdb_types::Value::kind`] is deliberately shallow — every object is
/// `object`, every array is `array<any>`, every number is `number`. That is the
/// right answer for a leaf and useless for a precision metric, so this recurses:
/// objects become literal object kinds, arrays carry the union of their element
/// kinds, and numbers are discriminated into `int` / `float` / `decimal`.
pub fn observed_kind(value: &Value) -> Kind {
    match value {
        // `Number::kind` is already the discriminated one (`int` / `float` /
        // `decimal`); it is `Value::kind` that widens every number to `number`.
        Value::Number(number) => number.kind(),
        Value::RecordId(record) => Kind::Record(vec![record.table.clone()]),
        Value::Object(object) => Kind::Literal(KindLiteral::Object(
            object
                .iter()
                .map(|(key, field)| (key.clone(), observed_kind(field)))
                .collect(),
        )),
        Value::Array(array) => Kind::Array(Box::new(union_of(array.iter())), None),
        Value::Set(set) => Kind::Set(Box::new(union_of(set.iter())), None),
        other => other.kind(),
    }
}

/// The union of the observed kinds of a collection's elements, deduplicated in
/// first-seen order so the rendering is stable.
fn union_of<'a>(values: impl Iterator<Item = &'a Value>) -> Kind {
    let mut members: Vec<Kind> = Vec::new();
    for value in values {
        let kind = observed_kind(value);
        if !members.contains(&kind) {
            members.push(kind);
        }
    }
    match members.len() {
        0 => Kind::Any,
        1 => members.remove(0),
        _ => Kind::Either(members),
    }
}

/// One position where the inferred kind is strictly wider than the observed
/// value needed. Correct, but imprecise — the signal the `any` ratchet and the
/// precision snapshot already care about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Wide {
    /// A bare `any`. The worst case: the analyzer claimed nothing at all.
    Any,
    /// A union of `n` members, only one of which was observed.
    Union(usize),
    /// An `option<T>` where the value was present.
    Optional,
    /// A bare `object` where a literal object kind was available.
    BareObject,
    /// A bare `number` where `int` / `float` / `decimal` was available.
    BareNumber,
}

impl Wide {
    /// The tally bucket this note belongs to.
    pub fn label(self) -> &'static str {
        match self {
            Wide::Any => "any",
            Wide::Union(_) => "union",
            Wide::Optional => "option",
            Wide::BareObject => "object",
            Wide::BareNumber => "number",
        }
    }
}

/// What one `(value, kind)` comparison produced.
#[derive(Debug, Default)]
pub struct Report {
    /// Positions where the value does not inhabit the kind. Non-empty means the
    /// analyzer is wrong, not merely imprecise.
    pub misses: Vec<String>,
    /// Keys the value carries that the kind never mentions — "the analyzer
    /// missed a field". Also a mismatch: a generated client would drop them.
    pub extra_keys: Vec<String>,
    /// The precision account.
    pub wide: Vec<(String, Wide)>,
}

impl Report {
    /// True when the value inhabits the kind.
    pub fn ok(&self) -> bool {
        self.misses.is_empty() && self.extra_keys.is_empty()
    }

    /// Every reason the value failed to inhabit the kind, in one line.
    pub fn detail(&self) -> String {
        let extras = self
            .extra_keys
            .iter()
            .map(|key| format!("`{key}` is present in the value and absent from the kind"));
        self.misses
            .iter()
            .cloned()
            .chain(extras)
            .collect::<Vec<_>>()
            .join("; ")
    }
}

/// `value ⊑ kind`, with a precision account.
///
/// Differs from [`surrealdb_types::Value::is_kind`] in exactly one load-bearing
/// way: **an object key the value omits is read as `Value::None`** rather than
/// failing on key count. SurrealDB does not store a field whose value is `NONE`,
/// so `{ name: string, nickname: option<string> }` is genuinely inhabited by
/// `{ name: 'A' }`, and `KindLiteral::Object::matches`'s exact key-set equality
/// would report that as a mismatch. Keys present in the value and absent from
/// the kind are reported separately, as their own finding class — that is the
/// "the analyzer missed a field" signal.
///
/// Every other position delegates to `is_kind`.
pub fn check(value: &Value, kind: &Kind, path: &str, report: &mut Report) {
    match kind {
        Kind::Any => report.wide.push((path.to_string(), Wide::Any)),
        Kind::Either(members) => check_either(value, kind, members, path, report),
        Kind::Literal(KindLiteral::Object(fields)) => {
            let Value::Object(object) = value else {
                report.misses.push(miss(path, value, kind));
                return;
            };
            for (key, field_kind) in fields {
                let child = object.get(key).cloned().unwrap_or(Value::None);
                check(&child, field_kind, &join(path, key), report);
            }
            for key in object.keys() {
                if !fields.contains_key(key) {
                    report.extra_keys.push(join(path, key));
                }
            }
        }
        Kind::Array(element, max) => {
            let Value::Array(array) = value else {
                report.misses.push(miss(path, value, kind));
                return;
            };
            check_elements(array.iter(), array.len(), element, *max, path, report);
        }
        Kind::Set(element, max) => {
            let Value::Set(set) = value else {
                report.misses.push(miss(path, value, kind));
                return;
            };
            check_elements(set.iter(), set.len(), element, *max, path, report);
        }
        Kind::Object => {
            if value.is_object() {
                report.wide.push((path.to_string(), Wide::BareObject));
            } else {
                report.misses.push(miss(path, value, kind));
            }
        }
        Kind::Number => {
            if value.is_number() {
                report.wide.push((path.to_string(), Wide::BareNumber));
            } else {
                report.misses.push(miss(path, value, kind));
            }
        }
        other => {
            if !value.is_kind(other) {
                report.misses.push(miss(path, value, other));
            }
        }
    }
}

/// A union member matches when the whole sub-check against it comes back clean,
/// so the missing-key adjustment applies inside `option<{ … }>` too.
fn check_either(value: &Value, kind: &Kind, members: &[Kind], path: &str, report: &mut Report) {
    let matched = members.iter().any(|member| {
        let mut probe = Report::default();
        check(value, member, path, &mut probe);
        probe.ok()
    });
    if !matched {
        report.misses.push(miss(path, value, kind));
        return;
    }
    // `option<T>` is `Either([none, T])` after folding — there is no
    // `Kind::Option`. Report it as its own class: an unnecessary `option` is a
    // much milder imprecision than a four-way union.
    let optional = members.len() == 2 && members.contains(&Kind::None);
    if optional && !matches!(value, Value::None) {
        report.wide.push((path.to_string(), Wide::Optional));
    } else if !optional {
        report
            .wide
            .push((path.to_string(), Wide::Union(members.len())));
    }
}

fn check_elements<'a>(
    elements: impl Iterator<Item = &'a Value>,
    len: usize,
    element: &Kind,
    max: Option<u64>,
    path: &str,
    report: &mut Report,
) {
    if let Some(max) = max {
        if len as u64 > max {
            report
                .misses
                .push(format!("{}: {len} elements exceeds max {max}", at(path)));
        }
    }
    for (index, value) in elements.enumerate() {
        check(value, element, &format!("{path}[{index}]"), report);
    }
}

/// A miss, phrased in kinds rather than values.
///
/// Deliberate: the same language test run twice produces different uuids and
/// datetimes, and a baseline whose detail text churns on every regeneration is
/// a baseline nobody reads.
fn miss(path: &str, value: &Value, kind: &Kind) -> String {
    format!(
        "{}: observed {} does not inhabit {}",
        at(path),
        render_kind(&observed_kind(value)),
        render_kind(kind)
    )
}

fn at(path: &str) -> String {
    if path.is_empty() {
        "<response>".to_string()
    } else {
        path.to_string()
    }
}

fn join(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_string()
    } else {
        format!("{path}.{key}")
    }
}

/// The verdict for one `(inferred kind, observed value)` pair.
#[derive(Debug)]
pub enum Verdict {
    /// The kind is exactly the kind of the value. Nothing to do.
    Exact,
    /// The value inhabits the kind but the kind admits more.
    Wider(Vec<(String, Wide)>),
    /// The value does **not** inhabit the kind. An analyzer bug, always.
    Mismatch(Report),
}

/// Compare one observed value against one inferred kind.
pub fn classify(value: &Value, kind: &Kind) -> Verdict {
    let mut report = Report::default();
    check(value, kind, "", &mut report);
    if !report.ok() {
        return Verdict::Mismatch(report);
    }
    if render_kind(&observed_kind(value)) == render_kind(kind) {
        Verdict::Exact
    } else {
        Verdict::Wider(report.wide)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use surrealdb_types::{Array, Number, Object};

    /// `Value` has no `Int` variant — every number goes through `Number`.
    fn int(value: i64) -> Value {
        Value::Number(Number::Int(value))
    }

    fn object(fields: [(&str, Value); 2]) -> Value {
        Value::Object(Object::from(BTreeMap::from(
            fields.map(|(key, value)| (key.to_string(), value)),
        )))
    }

    fn row_kind(fields: [(&str, Kind); 2]) -> Kind {
        Kind::Literal(KindLiteral::Object(BTreeMap::from(
            fields.map(|(key, kind)| (key.to_string(), kind)),
        )))
    }

    #[test]
    fn missing_key_reads_as_none() {
        // SurrealDB does not store a NONE field, so `{ name: 'a' }` really is
        // an inhabitant of `{ name: string, nickname: option<string> }`.
        let kind = row_kind([
            ("name", Kind::String),
            ("nickname", Kind::option(Kind::String)),
        ]);
        let value = Value::Object(Object::from(BTreeMap::from([(
            "name".to_string(),
            Value::String("a".to_string()),
        )])));
        assert!(
            !value.is_kind(&kind),
            "the engine's own relation rejects it"
        );
        let mut report = Report::default();
        check(&value, &kind, "", &mut report);
        assert!(report.ok(), "{:?}", report.misses);
    }

    #[test]
    fn missing_key_still_misses_a_required_field() {
        let kind = row_kind([("name", Kind::String), ("age", Kind::Int)]);
        let value = Value::Object(Object::from(BTreeMap::from([(
            "name".to_string(),
            Value::String("a".to_string()),
        )])));
        let mut report = Report::default();
        check(&value, &kind, "", &mut report);
        assert!(!report.ok());
        assert_eq!(report.misses, ["age: observed none does not inhabit int"]);
    }

    #[test]
    fn an_unexpected_key_is_a_mismatch_of_its_own() {
        let kind = row_kind([("id", Kind::Int), ("name", Kind::String)]);
        let value = object([("id", int(1)), ("extra", Value::Bool(true))]);
        let mut report = Report::default();
        check(&value, &kind, "", &mut report);
        assert_eq!(
            report.misses,
            ["name: observed none does not inhabit string"]
        );
        assert_eq!(report.extra_keys, ["extra"]);
        assert!(!report.ok());
    }

    #[test]
    fn exact_when_the_kind_is_the_value_s_kind() {
        assert!(matches!(
            classify(&Value::String("a".into()), &Kind::String),
            Verdict::Exact
        ));
    }

    #[test]
    fn any_is_always_wider_never_exact() {
        let Verdict::Wider(notes) = classify(&int(1), &Kind::Any) else {
            panic!("`any` must classify as wider");
        };
        assert_eq!(notes, [(String::new(), Wide::Any)]);
    }

    #[test]
    fn a_present_value_under_option_is_wider_not_exact() {
        let Verdict::Wider(notes) = classify(&int(1), &Kind::option(Kind::Int)) else {
            panic!("an unused `option` is imprecision, not a bug");
        };
        assert_eq!(notes, [(String::new(), Wide::Optional)]);
    }

    #[test]
    fn none_under_a_non_optional_kind_is_a_mismatch() {
        // The prototype's `SELECT tags[0]` bug: indexing is not total, so the
        // engine returns NONE where the analyzer promised a string.
        assert!(matches!(
            classify(&Value::None, &Kind::String),
            Verdict::Mismatch(_)
        ));
    }

    #[test]
    fn an_array_over_a_scalar_kind_is_a_mismatch() {
        // `[1,2,3][1..]` — a range slice read as an index.
        let value = Value::Array(Array::from(vec![int(2), int(3)]));
        let Verdict::Mismatch(report) = classify(&value, &Kind::Int) else {
            panic!("an array does not inhabit `int`");
        };
        assert_eq!(
            report.detail(),
            "<response>: observed array<int> does not inhabit int"
        );
    }

    #[test]
    fn a_literal_max_length_that_is_too_short_is_a_mismatch() {
        // `array::fold` leaking a literal-array max length.
        let value = Value::Array(Array::from(vec![int(1), int(2)]));
        let kind = Kind::Array(Box::new(Kind::Int), Some(1));
        let Verdict::Mismatch(report) = classify(&value, &kind) else {
            panic!("two elements do not fit `array<int, 1>`");
        };
        assert_eq!(report.detail(), "<response>: 2 elements exceeds max 1");
    }

    #[test]
    fn a_set_kind_is_not_inhabited_by_an_array_value() {
        let value = Value::Array(Array::from(vec![int(1)]));
        assert!(matches!(
            classify(&value, &Kind::Set(Box::new(Kind::Int), None)),
            Verdict::Mismatch(_)
        ));
    }

    #[test]
    fn a_union_records_its_width() {
        let kind = Kind::Either(vec![Kind::Int, Kind::String, Kind::Bool]);
        let Verdict::Wider(notes) = classify(&int(1), &kind) else {
            panic!("an int inhabits the union");
        };
        assert_eq!(notes, [(String::new(), Wide::Union(3))]);
    }

    #[test]
    fn observed_kind_recurses_where_value_kind_does_not() {
        let value = object([("id", int(1)), ("name", Value::String("a".into()))]);
        assert_eq!(value.kind(), Kind::Object);
        assert_eq!(
            render_kind(&observed_kind(&value)),
            "{ id: int, name: string }"
        );
    }
}
