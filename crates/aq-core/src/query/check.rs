//! A query that can only select nothing is an error (AQ-2).
//!
//! aq is jq for syntax trees, and like jq it answers a mistyped selector with an
//! empty result. At a terminal that is a nuisance; for a caller that reads
//! "nothing found" as "not there" it is a confident wrong answer. This module
//! looks at a *parsed* query against the grammar of the language it will run on
//! and refuses the ones that cannot match:
//!
//!   * every node kind the query names — `desc:K`, `children:K`, `desc:(A | B)`,
//!     `match(K ... field:(K))`, `@type == "K"` / `!= "K"` (and a
//!     `[@type == "K"]` pattern predicate), and `@type =~ "re"` (the regex must
//!     match some kind) — must be a kind the tree can contain. By default that
//!     is a *named, visible* kind (aq's trees hold named nodes only), so an
//!     anonymous token (`class`, `(`) is refused with its own message; with
//!     [`Scope::All`] (`--all-nodes`) tokens are accepted too. `ERROR` is
//!     always allowed, and `*` (`desc:*`) is any kind.
//!   * every `.field` the query reads off a node, and every `field:(K)` in a
//!     `match()`, must be a field name of the grammar.
//!
//! The grammar is described by the [`Grammar`] trait, which the backend that
//! owns the grammar implements (`aq-treesitter` does it for its `Language`);
//! this crate stays free of tree-sitter.
//!
//! ## What is *not* checked
//!
//! * **Fields are checked against the grammar's whole field table, not per node
//!   kind.** tree-sitter's runtime API exposes the global table (`field_count`,
//!   `field_name_for_id`) but not which kinds carry which fields, so
//!   `desc:identifier | .body` passes although an identifier has no `body`.
//!   A typo (`.nme`) is caught; a real field on the wrong kind is not.
//! * A `.name` is only checked where the query certainly applies it to a *node*
//!   — directly on the document root, or after a stage that yields nodes
//!   (`desc`, `children`, `desc:K`, `.field`, `parent`, ...). After `{...}`,
//!   `@meta`, a literal, a builtin or any other stage the input may be a JSON
//!   value (`{n: @text} | .n` reads a key, not a field) and the name is left
//!   alone.
//! * Kinds are validated against the language of each file, so a run over
//!   several languages is the caller's business (see `aq-cli`: an error only
//!   when the query can match in *none* of the languages involved).

use std::fmt;

use regex::Regex;
use serde::{Deserialize, Serialize};

use super::parser::{
    Axis, CmpOp, Expr, MetaField, Pattern, PatternPredicate, StringPart, Value, ANY_KIND,
};

/// What a grammar offers a query to name. Implemented by the backend that owns
/// the grammar (`aq_treesitter::langs::Language`).
pub trait Grammar {
    /// The language's display name (`Dart`, `TypeScript`, ...), used in messages.
    fn name(&self) -> &str;
    /// Named, visible node kinds — what aq's trees contain. (`ERROR` need not be listed.)
    fn node_kinds(&self) -> Vec<String>;
    /// Kinds that exist only as anonymous tokens (`class`, `(`): not in aq's
    /// default trees, present with `--all-nodes`.
    fn anonymous_kinds(&self) -> Vec<String>;
    /// Every field name of the grammar (`name`, `body`, ...).
    fn field_names(&self) -> Vec<String>;
}

/// Which nodes the tree being queried contains.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Named nodes only (aq's default).
    Named,
    /// Named and anonymous nodes (`--all-nodes`).
    All,
}

/// Why a query was refused, machine-readable (canopy carries it as a side
/// result's `query_error`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryError {
    /// `lex`, `parse`, `eval` (the text was refused — set by the caller),
    /// `unknown-node-kind` or `unknown-field` (the text is fine, the grammar
    /// has no such kind / field; set by [`check`]).
    pub kind: String,
    /// One sentence saying what is wrong and what was (not) done.
    pub message: String,
    /// `unknown-node-kind` only: the first kind that is not in the grammar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_kind: Option<String>,
    /// `unknown-field` only: the first field name that is not in the grammar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    /// `unknown-node-kind` / `unknown-field`: the grammar's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// `unknown-node-kind` / `unknown-field`: the nearest real kinds (fields)
    /// to `node_kind` (`field`), best first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suggestions: Vec<String>,
}

impl QueryError {
    /// An error with only a kind and a message (`lex`, `parse`, `eval`).
    pub fn text(kind: &str, message: impl Into<String>) -> Self {
        QueryError {
            kind: kind.into(),
            message: message.into(),
            node_kind: None,
            field: None,
            language: None,
            suggestions: vec![],
        }
    }
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for QueryError {}

// ── The grammar's names ──────────────────────────────────────────────────────

/// A grammar's kinds and fields, sorted for lookup.
#[derive(Debug, Clone, Default)]
pub struct Names {
    /// Named, visible kinds, plus `ERROR`.
    pub nodes: Vec<String>,
    /// Anonymous-only kinds, minus any that are also a node kind (JS `string`).
    pub tokens: Vec<String>,
    pub fields: Vec<String>,
}

fn sorted(mut v: Vec<String>) -> Vec<String> {
    v.retain(|s| !s.is_empty());
    v.sort();
    v.dedup();
    v
}

impl Names {
    pub fn of(grammar: &dyn Grammar) -> Names {
        let mut nodes = grammar.node_kinds();
        nodes.push("ERROR".to_string());
        let nodes = sorted(nodes);
        let mut tokens = sorted(grammar.anonymous_kinds());
        tokens.retain(|t| nodes.binary_search(t).is_err());
        Names {
            nodes,
            tokens,
            fields: sorted(grammar.field_names()),
        }
    }

    pub fn has_node(&self, kind: &str) -> bool {
        self.nodes
            .binary_search_by(|n| n.as_str().cmp(kind))
            .is_ok()
    }

    pub fn has_token(&self, kind: &str) -> bool {
        self.tokens
            .binary_search_by(|n| n.as_str().cmp(kind))
            .is_ok()
    }

    pub fn has_field(&self, name: &str) -> bool {
        self.fields
            .binary_search_by(|n| n.as_str().cmp(name))
            .is_ok()
    }
}

/// Up to three of `names` nearest to `wrong`, best first. Nearness is edit
/// distance, helped by sharing `_`-separated words (`class_definition` is near
/// `class_declaration` because both are a `class_...`).
pub fn suggest(wrong: &str, names: &[String]) -> Vec<String> {
    suggest_within(wrong, names, 4)
}

/// [`suggest`] with an explicit floor for the acceptance limit (`len / 3` or
/// `floor`, whichever is larger). Field names are short and numerous, so they
/// use a tighter floor than kinds to keep the suggestions near.
fn suggest_within(wrong: &str, names: &[String], floor: i64) -> Vec<String> {
    let words = |s: &str| {
        s.split('_')
            .filter(|w| !w.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>()
    };
    let wrong_words = words(wrong);
    let mut scored: Vec<(i64, &String)> = names
        .iter()
        .filter(|n| n.as_str() != "ERROR")
        .map(|n| {
            let shared = words(n).iter().filter(|w| wrong_words.contains(w)).count() as i64;
            let dist = levenshtein(wrong, n) as i64;
            (dist - 3 * shared, n)
        })
        .collect();
    scored.sort();
    let limit = (wrong.len() as i64 / 3).max(floor);
    scored
        .into_iter()
        .filter(|(s, _)| *s <= limit)
        .take(3)
        .map(|(_, n)| n.clone())
        .collect()
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let sub = prev[j] + usize::from(ca != cb);
            cur.push(sub.min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

// ── Walking a query ──────────────────────────────────────────────────────────

/// What flows into / out of a stage, as far as a field read is concerned.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Flow {
    /// Certainly grammar nodes.
    Node,
    /// Certainly JSON values (object keys, not fields).
    Value,
    Unknown,
}

/// A name the query uses, and where (for the message).
struct Named {
    name: String,
    site: String,
}

#[derive(Default)]
struct Found {
    kinds: Vec<Named>,
    /// Patterns that must match at least one node kind.
    regexes: Vec<String>,
    fields: Vec<Named>,
}

/// The last stage of a pipe: `a | b | @type` -> `@type`.
fn last_stage(e: &Expr) -> &Expr {
    match e {
        Expr::Pipe(_, r) => last_stage(r),
        other => other,
    }
}

fn is_type_meta(e: &Expr) -> bool {
    matches!(last_stage(e), Expr::Meta(MetaField::Type))
}

fn string_literal(e: &Expr) -> Option<&str> {
    match e {
        Expr::Literal(Value::String(s)) => Some(s),
        _ => None,
    }
}

fn walk(e: &Expr, input: Flow, found: &mut Found) -> Flow {
    match e {
        Expr::Identity => input,
        Expr::Field(name) => {
            if input == Flow::Node {
                found.fields.push(Named {
                    name: name.clone(),
                    site: format!(".{name}"),
                });
                Flow::Node
            } else if input == Flow::Value {
                Flow::Value
            } else {
                Flow::Unknown
            }
        }
        Expr::Meta(_) | Expr::Literal(_) => Flow::Value,
        Expr::Pipe(a, b) => {
            let mid = walk(a, input, found);
            walk(b, mid, found)
        }
        Expr::Children(_)
        | Expr::Descendants(_)
        | Expr::Parent
        | Expr::Ancestors
        | Expr::Sibling(_) => Flow::Node,
        Expr::Iterate => {
            if input == Flow::Node {
                Flow::Node
            } else {
                Flow::Unknown
            }
        }
        Expr::Index(_) => Flow::Unknown,
        Expr::TypeFilter { axis, types } => {
            let prefix = match axis {
                Axis::Children => "children:",
                Axis::Descendants(_) => "desc:",
                Axis::Self_ => "self:",
            };
            for t in types.iter().filter(|t| t.as_str() != ANY_KIND) {
                found.kinds.push(Named {
                    name: t.clone(),
                    site: format!("{prefix}{t}"),
                });
            }
            Flow::Node
        }
        Expr::Match(Pattern { steps }) => {
            for s in steps {
                found.kinds.push(Named {
                    name: s.node_type.clone(),
                    site: format!("match({} ...)", s.node_type),
                });
                if let Some((field, k)) = &s.field_constraint {
                    found.fields.push(Named {
                        name: field.clone(),
                        site: format!("match(... {field}:({k}))"),
                    });
                    found.kinds.push(Named {
                        name: k.clone(),
                        site: format!("match(... {field}:({k}))"),
                    });
                }
                for PatternPredicate { field, op, value } in &s.predicates {
                    if field == "type" {
                        match op {
                            CmpOp::Eq | CmpOp::NotEq => found.kinds.push(Named {
                                name: value.clone(),
                                site: format!("match(... [@type == \"{value}\"])"),
                            }),
                            CmpOp::RegexMatch => found.regexes.push(value.clone()),
                            _ => {}
                        }
                    }
                }
            }
            Flow::Unknown
        }
        Expr::Select(x) => {
            walk(x, input, found);
            input
        }
        Expr::Array(x) | Expr::LogicNot(x) => {
            walk(x, input, found);
            Flow::Value
        }
        Expr::Object(kv) => {
            for (_, v) in kv {
                walk(v, input, found);
            }
            Flow::Value
        }
        Expr::StringInterp(parts) => {
            for p in parts {
                if let StringPart::Interpolation(x) = p {
                    walk(x, input, found);
                }
            }
            Flow::Value
        }
        Expr::Compare(l, op, r) => {
            let lit = if is_type_meta(l) {
                string_literal(r)
            } else if is_type_meta(r) {
                string_literal(l)
            } else {
                None
            };
            if let Some(s) = lit {
                match op {
                    CmpOp::Eq | CmpOp::NotEq => found.kinds.push(Named {
                        name: s.to_string(),
                        site: format!("@type == \"{s}\""),
                    }),
                    CmpOp::RegexMatch => found.regexes.push(s.to_string()),
                    _ => {}
                }
            }
            walk(l, input, found);
            walk(r, input, found);
            Flow::Value
        }
        Expr::Arithmetic(l, _, r) | Expr::Logic(l, _, r) => {
            walk(l, input, found);
            walk(r, input, found);
            Flow::Value
        }
        Expr::Alternative(a, b) => {
            let (fa, fb) = (walk(a, input, found), walk(b, input, found));
            if fa == fb {
                fa
            } else {
                Flow::Unknown
            }
        }
        Expr::Builtin(_, args) => {
            for a in args {
                walk(a, Flow::Unknown, found);
            }
            Flow::Unknown
        }
        Expr::IfThenElse {
            cond,
            then_branch,
            else_branch,
        } => {
            walk(cond, input, found);
            let t = walk(then_branch, input, found);
            let el = else_branch.as_ref().map(|x| walk(x, input, found));
            match el {
                Some(f) if f == t => t,
                _ => Flow::Unknown,
            }
        }
        Expr::Concat(v) => {
            for x in v {
                walk(x, input, found);
            }
            Flow::Unknown
        }
    }
}

// ── The check ────────────────────────────────────────────────────────────────

/// `Ok` when every node kind and field the query names exists in `grammar`;
/// otherwise a [`QueryError`] (`unknown-node-kind` if any kind is bad, else
/// `unknown-field`) naming every problem. See the module docs for what is and
/// is not checked. The error is boxed: it is large and the path is cold.
pub fn check(expr: &Expr, grammar: &dyn Grammar, scope: Scope) -> Result<(), Box<QueryError>> {
    let mut found = Found::default();
    walk(expr, Flow::Node, &mut found);
    if found.kinds.is_empty() && found.regexes.is_empty() && found.fields.is_empty() {
        return Ok(());
    }
    let names = Names::of(grammar);
    let lang = grammar.name();
    let mut problems: Vec<String> = Vec::new();
    let mut first_kind: Option<(String, Vec<String>)> = None;
    let mut first_field: Option<(String, Vec<String>)> = None;

    let mut seen: Vec<&str> = Vec::new();
    for n in &found.kinds {
        let anonymous_ok = scope == Scope::All && names.has_token(&n.name);
        if names.has_node(&n.name) || anonymous_ok || seen.contains(&n.name.as_str()) {
            continue;
        }
        seen.push(&n.name);
        let near = suggest(&n.name, &names.nodes);
        let mut msg = if names.has_token(&n.name) {
            format!(
                "`{}` is an anonymous token of {lang}, not a node kind: aq only sees named nodes unless --all-nodes is given, so `{}` can never match",
                n.name, n.site
            )
        } else {
            format!(
                "unknown node kind `{}` for {lang} (in `{}`)",
                n.name, n.site
            )
        };
        if !near.is_empty() {
            msg.push_str(&format!("; did you mean {}?", ticked(&near)));
        }
        problems.push(msg);
        first_kind.get_or_insert((n.name.clone(), near));
    }
    for re in &found.regexes {
        if let Ok(re_c) = Regex::new(re) {
            let any_node = names.nodes.iter().any(|k| re_c.is_match(k));
            let any_token = scope == Scope::All && names.tokens.iter().any(|k| re_c.is_match(k));
            if !any_node && !any_token {
                problems.push(format!(
                    "the pattern `{re}` on `@type` matches no node kind of {lang}"
                ));
                first_kind.get_or_insert((re.clone(), vec![]));
            }
        }
    }
    let mut seen_fields: Vec<&str> = Vec::new();
    for f in &found.fields {
        if names.has_field(&f.name) || seen_fields.contains(&f.name.as_str()) {
            continue;
        }
        seen_fields.push(&f.name);
        let near = suggest_within(&f.name, &names.fields, 2);
        let mut msg = format!("unknown field `{}` for {lang} (in `{}`)", f.name, f.site);
        if !near.is_empty() {
            msg.push_str(&format!("; did you mean {}?", ticked(&near)));
        }
        problems.push(msg);
        first_field.get_or_insert((f.name.clone(), near));
    }

    let (kind, node_kind, field, suggestions) = match (first_kind, first_field) {
        (Some((k, s)), _) => ("unknown-node-kind", Some(k), None, s),
        (None, Some((f, s))) => ("unknown-field", None, Some(f), s),
        (None, None) => return Ok(()),
    };
    let message = format!(
        "{} — nothing was searched. Kinds and fields are those of the {lang} grammar this build links; a build with another grammar version can name them differently.",
        problems.join("; ")
    );
    Err(Box::new(QueryError {
        kind: kind.into(),
        message,
        node_kind,
        field,
        language: Some(lang.to_string()),
        suggestions,
    }))
}

fn ticked(v: &[String]) -> String {
    v.iter()
        .map(|k| format!("`{k}`"))
        .collect::<Vec<_>>()
        .join(", ")
}
