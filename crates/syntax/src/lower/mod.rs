//! CST → AST lowering.
//!
//! The single place that reads tree-sitter nodes and source text. Everything
//! downstream (analyzers) consumes [`crate::ast`] values.
//!
//! Constructs without a modeled lowering — and statements whose direct
//! syntax is broken — lower to explicit `Partial` values rather than being
//! silently dropped.

mod expr;
#[cfg(test)]
mod grammar_tests;
mod statement;

pub use expr::{lower_expr, lower_type_expr};
pub(crate) use statement::lower_statement;
pub use statement::lower_statements;

use std::cell::Cell;

use crate::ast::{Expr, PartialNode, Script, Spanned, Statement};
use crate::parse::ParsedSource;
use crate::span::ByteRange;
use tree_sitter::Node;

/// How many levels of nested construct lowering descends before it stops.
///
/// Nesting depth is unbounded user input — `RETURN ((((…1…))))` two thousand
/// deep is a four-kilobyte file — and every lowering path is recursive, so
/// without a budget the process died with `stack overflow` and no diagnostic
/// at all. The number is a stack budget, not a language limit: the costliest
/// level (a subquery inside a subquery) takes roughly 18 KiB of an
/// unoptimized lowering frame and as much again in the analysis walk that
/// follows, so 128 levels fits inside even a 2 MiB worker thread, while no
/// hand-written SurrealQL comes within an order of magnitude of it. Left-deep
/// operator chains (`a AND b AND … AND z`) do not count against it —
/// [`expr::lower_expr`] walks their spine iteratively — because generated SQL
/// makes those hundreds of terms long.
pub const MAX_NESTING_DEPTH: u32 = 128;

/// The `cst_kind` a `Partial` carries when lowering stopped at
/// [`MAX_NESTING_DEPTH`] rather than at broken syntax. Analyzers match on it
/// to report the cut-off instead of treating it as unmodeled syntax.
pub const DEPTH_BUDGET_KIND: &str = "over the nesting budget";

/// How many terms of one left-deep operator chain (`a AND b AND … AND z`,
/// `1 + 1 + …`) lowering walks.
///
/// A chain is flat to a reader but left-nested in the CST, and generated SQL
/// (`IN`-list expansion, ORM filters) writes them hundreds of terms long, so
/// they get their own, larger budget: lowering spends no stack per term, and
/// the analysis walk that follows spends about 4 KiB per term rather than the
/// 18 KiB a nested statement costs. 512 terms is the one budget here that
/// wants the 8 MiB a default main thread has rather than a 2 MiB worker's.
pub const MAX_OPERATOR_CHAIN: usize = 512;

thread_local! {
    /// Lowering recursion depth on this thread. Lowering is a synchronous
    /// tree walk, so a thread-local counter needs no plumbing through the
    /// dozens of free functions that make up statement lowering.
    static DEPTH: Cell<u32> = const { Cell::new(0) };
    /// Where the budget first stopped the current walk, for the caller that
    /// reports it. Set here rather than discovered by a later pass: the
    /// truncated `Partial` can end up in a position — a subquery's statement,
    /// say — that no analyzer visits, and a cut-off no one reports is exactly
    /// the silence this whole budget exists to replace.
    static CUTOFF: Cell<Option<ByteRange>> = const { Cell::new(None) };
}

/// One level of the lowering recursion, released on drop (unwinding
/// included), so the counter can never drift.
pub(crate) struct DepthGuard;

impl DepthGuard {
    /// Claims one level, or `None` when the budget is spent.
    pub(crate) fn enter() -> Option<Self> {
        DEPTH.with(|depth| {
            let current = depth.get();
            if current >= MAX_NESTING_DEPTH {
                return None;
            }
            depth.set(current + 1);
            Some(Self)
        })
    }
}

impl Drop for DepthGuard {
    fn drop(&mut self) {
        DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

/// The `PartialNode` that stands in for everything below the budget, and the
/// record that the walk was cut off at all.
pub(crate) fn over_budget(node: Node<'_>) -> PartialNode {
    let span = node_range(node);
    CUTOFF.with(|cutoff| {
        if cutoff.get().is_none() {
            cutoff.set(Some(span));
        }
    });
    PartialNode {
        span,
        cst_kind: DEPTH_BUDGET_KIND.to_string(),
    }
}

/// Lowers `parsed`'s statements, also reporting where the nesting budget
/// stopped the walk — `None` when the whole input fitted.
///
/// One cut-off per lowering: the first is the outermost, and everything under
/// it is the same fact about the same input.
pub fn lower_statements_reporting_cutoff(
    parsed: &ParsedSource,
) -> (Vec<Spanned<Statement>>, Option<ByteRange>) {
    CUTOFF.with(|cutoff| cutoff.set(None));
    let statements = statement::lower_statements(parsed);
    (statements, CUTOFF.with(Cell::take))
}

/// Lowers the first CST node of `cst_kind` (depth-first) as an expression.
///
/// The node-walking entry point for tests and tools that target a specific
/// sub-expression (`FunctionCall`, `Block`, `BinaryExpression`, ...) without
/// reaching for tree-sitter themselves.
pub fn lower_first_expr(parsed: &ParsedSource, cst_kind: &str) -> Option<Spanned<Expr>> {
    let node = find_first(parsed.tree().root_node(), cst_kind)?;
    Some(lower_expr(node, parsed.text()))
}

/// Lowers the first CST node of `cst_kind` (depth-first) as a statement.
pub fn lower_first_statement(parsed: &ParsedSource, cst_kind: &str) -> Option<Spanned<Statement>> {
    let node = find_first(parsed.tree().root_node(), cst_kind)?;
    Some(lower_statement(node, parsed.text()))
}

fn find_first<'tree>(node: Node<'tree>, cst_kind: &str) -> Option<Node<'tree>> {
    // Depth-first with an explicit stack: the CST is as deep as the input
    // nests.
    let mut stack = vec![node];
    while let Some(node) = stack.pop() {
        if node.kind() == cst_kind {
            return Some(node);
        }
        let mut cursor = node.walk();
        let children: Vec<Node<'tree>> = node.children(&mut cursor).collect();
        stack.extend(children.into_iter().rev());
    }
    None
}

/// Lowers a parsed source to a [`Script`], statements in source order.
pub fn lower(parsed: &ParsedSource) -> Script {
    let root = parsed.tree().root_node();
    let mut statements = Vec::new();

    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if !child.is_named() || matches!(child.kind(), "Comment" | "BlockComment") {
            continue;
        }
        statements.push(statement::lower_statement(child, parsed.text()));
    }

    Script { statements }
}

pub(crate) fn node_range(node: Node<'_>) -> ByteRange {
    let start = u32::try_from(node.start_byte()).unwrap_or(u32::MAX);
    let end = u32::try_from(node.end_byte()).unwrap_or(u32::MAX);
    ByteRange::new(start, end).expect("tree-sitter nodes have ordered byte ranges")
}

/// Whether `node` is parser recovery rather than syntax: an `ERROR` node, or
/// a token the parser inserted (see [`crate::parse::is_missing`] for why
/// `Node::is_missing` alone misses the hidden-token case).
pub(crate) fn is_broken(node: Node<'_>) -> bool {
    node.is_error() || crate::parse::is_missing(node)
}

pub(crate) fn partial(node: Node<'_>) -> PartialNode {
    let cst_kind = if crate::parse::is_missing(node) {
        format!("MISSING {}", node.kind())
    } else {
        node.kind().to_string()
    };
    PartialNode {
        span: node_range(node),
        cst_kind,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Statement;
    use crate::parse::parse_source;
    use crate::source::SourceId;

    #[test]
    fn script_lowering_preserves_statement_order() {
        let parsed = parse_source(
            SourceId::new("lower:script"),
            "DEFINE TABLE person;\nSELECT * FROM person;\nRETURN 1;",
        )
        .expect("parses");

        let script = lower(&parsed);

        assert!(matches!(&script.statements[0].node, Statement::Define(_)));
        assert!(matches!(&script.statements[1].node, Statement::Select(_)));
        assert!(matches!(&script.statements[2].node, Statement::Return(_)));
        // Source order is positional: each statement's span starts after the
        // previous one ends.
        let spans: Vec<_> = script.statements.iter().map(|s| s.span).collect();
        assert!(spans.windows(2).all(|w| w[0].end() <= w[1].start()));
    }
}
