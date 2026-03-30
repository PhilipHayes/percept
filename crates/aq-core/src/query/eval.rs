use std::collections::HashMap;
use crate::node::AqNode;
use super::parser::{ArithOp, Axis, CmpOp, Combinator, Expr, LogicOp, MetaField, Pattern, PatternPredicate, PatternStep, SiblingKind, StringPart, Value};

/// Evaluation context holding the parent map for navigating up the tree.
struct EvalContext<'a> {
    parent_map: HashMap<usize, &'a dyn AqNode>,
}

/// Get a stable identity key for a node (pointer address).
fn node_key(node: &dyn AqNode) -> usize {
    node as *const dyn AqNode as *const () as usize
}

/// Build a map from child node key → parent node reference.
fn build_parent_map<'a>(root: &'a dyn AqNode) -> HashMap<usize, &'a dyn AqNode> {
    let mut map = HashMap::new();
    build_parent_map_inner(root, &mut map);
    map
}

fn build_parent_map_inner<'a>(node: &'a dyn AqNode, map: &mut HashMap<usize, &'a dyn AqNode>) {
    for child in node.named_children() {
        map.insert(node_key(child), node);
        build_parent_map_inner(child, map);
    }
}

/// Evaluate an aq expression against a root node, producing a stream of results.
pub fn eval<'a>(
    expr: &Expr,
    root: &'a dyn AqNode,
) -> Result<Vec<EvalResult<'a>>, EvalError> {
    let parent_map = build_parent_map(root);
    let ctx = EvalContext { parent_map };
    eval_filter(expr, &EvalResult::Node(root), &ctx)
}

/// A query result — either a reference to a tree node or a computed value.
pub enum EvalResult<'a> {
    Node(&'a dyn AqNode),
    Value(serde_json::Value),
}

impl<'a> Clone for EvalResult<'a> {
    fn clone(&self) -> Self {
        match self {
            EvalResult::Node(n) => EvalResult::Node(*n),
            EvalResult::Value(v) => EvalResult::Value(v.clone()),
        }
    }
}

impl<'a> std::fmt::Debug for EvalResult<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EvalResult::Node(n) => {
                write!(f, "Node({}, {}..{})", n.node_type(), n.start_line(), n.end_line())
            }
            EvalResult::Value(v) => write!(f, "Value({})", v),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Eval error: {message}")]
pub struct EvalError {
    pub message: String,
}

// ---------------------------------------------------------------------------
// Core evaluation
// ---------------------------------------------------------------------------

fn eval_filter<'a>(
    expr: &Expr,
    input: &EvalResult<'a>,
    ctx: &EvalContext<'a>,
) -> Result<Vec<EvalResult<'a>>, EvalError> {
    match expr {
        Expr::Identity => Ok(vec![input.clone()]),

        Expr::Field(name) => eval_field(name, input),

        Expr::Meta(field) => eval_meta(field, input, ctx),

        Expr::Pipe(left, right) => {
            let left_results = eval_filter(left, input, ctx)?;
            let mut out = Vec::new();
            for result in &left_results {
                out.extend(eval_filter(right, result, ctx)?);
            }
            Ok(out)
        }

        Expr::Children(opt_index) => eval_children(opt_index, input),

        Expr::Descendants(opt_depth) => eval_descendants(opt_depth, input),

        Expr::TypeFilter { axis, types } => eval_type_filter(axis, types, input),

        Expr::Parent => {
            if let EvalResult::Node(n) = input {
                let key = node_key(*n);
                if let Some(&parent) = ctx.parent_map.get(&key) {
                    Ok(vec![EvalResult::Node(parent)])
                } else {
                    Ok(vec![])
                }
            } else {
                Ok(vec![])
            }
        }

        Expr::Ancestors => {
            if let EvalResult::Node(n) = input {
                let mut ancestors = Vec::new();
                let mut key = node_key(*n);
                while let Some(&p) = ctx.parent_map.get(&key) {
                    ancestors.push(EvalResult::Node(p));
                    key = node_key(p);
                }
                Ok(ancestors)
            } else {
                Ok(vec![])
            }
        }

        Expr::Sibling(kind) => {
            if let EvalResult::Node(n) = input {
                let self_key = node_key(*n);
                match kind {
                    SiblingKind::All => {
                        if let Some(&parent) = ctx.parent_map.get(&self_key) {
                            Ok(parent.named_children()
                                .into_iter()
                                .filter(|c| node_key(*c) != self_key)
                                .map(EvalResult::Node)
                                .collect())
                        } else {
                            Ok(vec![])
                        }
                    }
                    SiblingKind::Prev => {
                        if let Some(&parent) = ctx.parent_map.get(&self_key) {
                            let children = parent.named_children();
                            let pos = children.iter().position(|c| node_key(*c) == self_key);
                            if let Some(i) = pos {
                                if i > 0 {
                                    Ok(vec![EvalResult::Node(children[i - 1])])
                                } else {
                                    Ok(vec![])
                                }
                            } else {
                                Ok(vec![])
                            }
                        } else {
                            Ok(vec![])
                        }
                    }
                    SiblingKind::Next => {
                        if let Some(&parent) = ctx.parent_map.get(&self_key) {
                            let children = parent.named_children();
                            let pos = children.iter().position(|c| node_key(*c) == self_key);
                            if let Some(i) = pos {
                                if i + 1 < children.len() {
                                    Ok(vec![EvalResult::Node(children[i + 1])])
                                } else {
                                    Ok(vec![])
                                }
                            } else {
                                Ok(vec![])
                            }
                        } else {
                            Ok(vec![])
                        }
                    }
                }
            } else {
                Ok(vec![])
            }
        }

        Expr::Select(cond) => {
            let results = eval_filter(cond, input, ctx)?;
            if is_truthy(&results) {
                Ok(vec![input.clone()])
            } else {
                Ok(vec![])
            }
        }

        Expr::Object(pairs) => {
            let mut map = serde_json::Map::new();
            for (key, value_expr) in pairs {
                let results = eval_filter(value_expr, input, ctx)?;
                let val = results
                    .first()
                    .map(result_to_json)
                    .unwrap_or(serde_json::Value::Null);
                map.insert(key.clone(), val);
            }
            Ok(vec![EvalResult::Value(serde_json::Value::Object(map))])
        }

        Expr::Array(inner) => {
            let results = eval_filter(inner, input, ctx)?;
            let arr: Vec<serde_json::Value> = results.iter().map(result_to_json).collect();
            Ok(vec![EvalResult::Value(serde_json::Value::Array(arr))])
        }

        Expr::Literal(val) => {
            let json = value_to_json(val);
            Ok(vec![EvalResult::Value(json)])
        }

        Expr::Compare(left, op, right) => {
            let l_results = eval_filter(left, input, ctx)?;
            let r_results = eval_filter(right, input, ctx)?;
            let l_val = l_results.first().map(result_to_json).unwrap_or(serde_json::Value::Null);
            let r_val = r_results.first().map(result_to_json).unwrap_or(serde_json::Value::Null);
            let result = compare_values(&l_val, op, &r_val)?;
            Ok(vec![EvalResult::Value(serde_json::Value::Bool(result))])
        }

        Expr::Alternative(left, right) => {
            let l_results = eval_filter(left, input, ctx)?;
            if !l_results.is_empty() && !all_null(&l_results) {
                Ok(l_results)
            } else {
                eval_filter(right, input, ctx)
            }
        }

        Expr::Arithmetic(left, op, right) => {
            let l_results = eval_filter(left, input, ctx)?;
            let r_results = eval_filter(right, input, ctx)?;
            let l_num = extract_number(&l_results)?;
            let r_num = extract_number(&r_results)?;
            let result = match op {
                ArithOp::Add => l_num + r_num,
                ArithOp::Sub => l_num - r_num,
                ArithOp::Mul => l_num * r_num,
                ArithOp::Div => {
                    if r_num == 0.0 {
                        return Err(EvalError {
                            message: "Division by zero".into(),
                        });
                    }
                    l_num / r_num
                }
            };
            Ok(vec![EvalResult::Value(json_number(result))])
        }

        Expr::Logic(left, op, right) => {
            let l_results = eval_filter(left, input, ctx)?;
            let r_results = eval_filter(right, input, ctx)?;
            let result = match op {
                LogicOp::And => is_truthy(&l_results) && is_truthy(&r_results),
                LogicOp::Or => is_truthy(&l_results) || is_truthy(&r_results),
            };
            Ok(vec![EvalResult::Value(serde_json::Value::Bool(result))])
        }

        Expr::LogicNot(inner) => {
            let results = eval_filter(inner, input, ctx)?;
            Ok(vec![EvalResult::Value(serde_json::Value::Bool(
                !is_truthy(&results),
            ))])
        }

        Expr::Builtin(name, args) => eval_builtin(name, args, input, ctx),

        Expr::IfThenElse {
            cond,
            then_branch,
            else_branch,
        } => {
            let cond_results = eval_filter(cond, input, ctx)?;
            if is_truthy(&cond_results) {
                eval_filter(then_branch, input, ctx)
            } else if let Some(else_br) = else_branch {
                eval_filter(else_br, input, ctx)
            } else {
                Ok(vec![input.clone()])
            }
        }

        Expr::Match(pattern) => eval_match(pattern, input),

        Expr::Iterate => {
            match input {
                EvalResult::Value(serde_json::Value::Array(arr)) => {
                    Ok(arr.iter().map(|v| EvalResult::Value(v.clone())).collect())
                }
                EvalResult::Value(serde_json::Value::Object(obj)) => {
                    Ok(obj.values().map(|v| EvalResult::Value(v.clone())).collect())
                }
                EvalResult::Node(n) => {
                    Ok(n.named_children().into_iter().map(EvalResult::Node).collect())
                }
                _ => Ok(vec![]),
            }
        }

        Expr::Index(idx) => {
            match input {
                EvalResult::Value(serde_json::Value::Array(arr)) => {
                    let i = if *idx < 0 {
                        (arr.len() as isize + idx) as usize
                    } else {
                        *idx as usize
                    };
                    if i < arr.len() {
                        Ok(vec![EvalResult::Value(arr[i].clone())])
                    } else {
                        Ok(vec![EvalResult::Value(serde_json::Value::Null)])
                    }
                }
                _ => Ok(vec![EvalResult::Value(serde_json::Value::Null)]),
            }
        }

        Expr::Concat(exprs) => {
            let mut results = Vec::new();
            for expr in exprs {
                results.extend(eval_filter(expr, input, ctx)?);
            }
            Ok(results)
        }

        Expr::StringInterp(parts) => {
            let mut result = String::new();
            for part in parts {
                match part {
                    StringPart::Literal(s) => result.push_str(s),
                    StringPart::Interpolation(expr) => {
                        let results = eval_filter(expr, input, ctx)?;
                        if let Some(r) = results.first() {
                            match r {
                                EvalResult::Value(serde_json::Value::String(s)) => {
                                    result.push_str(s);
                                }
                                EvalResult::Value(serde_json::Value::Null) => {}
                                EvalResult::Value(v) => {
                                    result.push_str(&v.to_string());
                                }
                                EvalResult::Node(n) => {
                                    if let Some(t) = n.text().or_else(|| n.subtree_text()) {
                                        result.push_str(t);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Ok(vec![EvalResult::Value(serde_json::Value::String(result))])
        }
    }
}

// ---------------------------------------------------------------------------
// Expression-specific evaluators
// ---------------------------------------------------------------------------

fn eval_field<'a>(name: &str, input: &EvalResult<'a>) -> Result<Vec<EvalResult<'a>>, EvalError> {
    match input {
        EvalResult::Node(n) => {
            if let Some(child) = n.child_by_field(name) {
                Ok(vec![EvalResult::Node(child)])
            } else {
                Ok(vec![])
            }
        }
        EvalResult::Value(serde_json::Value::Object(map)) => {
            if let Some(val) = map.get(name) {
                Ok(vec![EvalResult::Value(val.clone())])
            } else {
                Ok(vec![])
            }
        }
        _ => Ok(vec![]),
    }
}

fn eval_meta<'a>(field: &MetaField, input: &EvalResult<'a>, ctx: &EvalContext<'a>) -> Result<Vec<EvalResult<'a>>, EvalError> {
    // Format meta fields work on any input (nodes or values)
    match field {
        MetaField::Csv => return format_csv(input),
        MetaField::Tsv => return format_tsv(input),
        MetaField::Json => return format_json_meta(input),
        _ => {}
    }

    match input {
        EvalResult::Node(n) => {
            let val = match field {
                MetaField::Type => serde_json::Value::String(n.node_type().to_string()),
                MetaField::Text => match n.text() {
                    Some(t) => serde_json::Value::String(t.to_string()),
                    None => match n.subtree_text() {
                        Some(t) => serde_json::Value::String(t.to_string()),
                        None => serde_json::Value::Null,
                    },
                },
                MetaField::Start => json_number(n.start_line() as f64),
                MetaField::End => json_number(n.end_line() as f64),
                MetaField::Line => json_number(n.start_line() as f64),
                MetaField::File => match n.source_file() {
                    Some(f) => serde_json::Value::String(f.to_string()),
                    None => serde_json::Value::Null,
                },
                MetaField::SubtreeText => match n.subtree_text() {
                    Some(t) => serde_json::Value::String(t.to_string()),
                    None => serde_json::Value::Null,
                },
                MetaField::Depth => {
                    let mut depth = 0usize;
                    let mut key = node_key(*n);
                    while let Some(&p) = ctx.parent_map.get(&key) {
                        depth += 1;
                        key = node_key(p);
                    }
                    json_number(depth as f64)
                }
                MetaField::Path => {
                    let mut types = vec![n.node_type().to_string()];
                    let mut key = node_key(*n);
                    while let Some(&p) = ctx.parent_map.get(&key) {
                        types.push(p.node_type().to_string());
                        key = node_key(p);
                    }
                    types.reverse();
                    serde_json::Value::Array(
                        types.into_iter().map(serde_json::Value::String).collect(),
                    )
                }
                MetaField::Csv | MetaField::Tsv | MetaField::Json => unreachable!(),
            };
            Ok(vec![EvalResult::Value(val)])
        }
        _ => Ok(vec![EvalResult::Value(serde_json::Value::Null)]),
    }
}

fn eval_children<'a>(
    opt_index: &Option<isize>,
    input: &EvalResult<'a>,
) -> Result<Vec<EvalResult<'a>>, EvalError> {
    if let EvalResult::Node(n) = input {
        let children = n.named_children();
        if let Some(idx) = opt_index {
            let i = if *idx < 0 {
                (children.len() as isize + idx) as usize
            } else {
                *idx as usize
            };
            if i < children.len() {
                Ok(vec![EvalResult::Node(children[i])])
            } else {
                Ok(vec![])
            }
        } else {
            Ok(children.into_iter().map(EvalResult::Node).collect())
        }
    } else {
        Ok(vec![])
    }
}

fn eval_descendants<'a>(
    opt_depth: &Option<usize>,
    input: &EvalResult<'a>,
) -> Result<Vec<EvalResult<'a>>, EvalError> {
    if let EvalResult::Node(n) = input {
        let mut results = Vec::new();
        collect_descendants(*n, *opt_depth, 0, &mut results);
        Ok(results)
    } else {
        Ok(vec![])
    }
}

fn collect_descendants<'a>(
    node: &'a dyn AqNode,
    max_depth: Option<usize>,
    current_depth: usize,
    results: &mut Vec<EvalResult<'a>>,
) {
    for child in node.named_children() {
        results.push(EvalResult::Node(child));
        let next_depth = current_depth + 1;
        if max_depth.is_none() || next_depth < max_depth.unwrap() {
            collect_descendants(child, max_depth, next_depth, results);
        }
    }
}

fn eval_type_filter<'a>(
    axis: &Axis,
    types: &[String],
    input: &EvalResult<'a>,
) -> Result<Vec<EvalResult<'a>>, EvalError> {
    if let EvalResult::Node(n) = input {
        let candidates: Vec<&'a dyn AqNode> = match axis {
            Axis::Children => n.named_children(),
            Axis::Descendants(opt_depth) => {
                let mut results = Vec::new();
                collect_descendants_raw(*n, *opt_depth, 0, &mut results);
                results
            }
            Axis::Self_ => {
                vec![*n]
            }
        };
        Ok(candidates
            .into_iter()
            .filter(|c| types.iter().any(|t| c.node_type() == t.as_str()))
            .map(EvalResult::Node)
            .collect())
    } else {
        Ok(vec![])
    }
}

fn collect_descendants_raw<'a>(
    node: &'a dyn AqNode,
    max_depth: Option<usize>,
    current_depth: usize,
    results: &mut Vec<&'a dyn AqNode>,
) {
    for child in node.named_children() {
        results.push(child);
        let next = current_depth + 1;
        if max_depth.is_none() || next < max_depth.unwrap() {
            collect_descendants_raw(child, max_depth, next, results);
        }
    }
}

// ---------------------------------------------------------------------------
// Match pattern evaluation
// ---------------------------------------------------------------------------

fn eval_match<'a>(
    pattern: &Pattern,
    input: &EvalResult<'a>,
) -> Result<Vec<EvalResult<'a>>, EvalError> {
    let node = match input {
        EvalResult::Node(n) => *n,
        _ => return Ok(vec![]),
    };

    if pattern.steps.is_empty() {
        return Ok(vec![]);
    }

    // The first step (Root combinator) finds initial candidates
    // by searching descendants of the input node for the root type.
    let first_step = &pattern.steps[0];
    let initial_candidates = find_matching_nodes(node, first_step)?;

    if pattern.steps.len() == 1 {
        // Only one step — return all matches
        return Ok(initial_candidates.into_iter().map(EvalResult::Node).collect());
    }

    // For subsequent steps, traverse from each candidate
    let mut current_matches = initial_candidates;
    for step in &pattern.steps[1..] {
        let mut next_matches = Vec::new();
        for candidate in &current_matches {
            let step_matches = match step.combinator {
                Combinator::Root => unreachable!("Root combinator only valid for first step"),
                Combinator::Child => {
                    // Direct children matching the step
                    let mut matches = Vec::new();
                    for child in candidate.named_children() {
                        if node_matches_step(child, step)? {
                            matches.push(child);
                        }
                    }
                    matches
                }
                Combinator::Descendant => {
                    // Any descendant matching the step
                    let mut all_desc = Vec::new();
                    collect_descendants_raw(*candidate, None, 0, &mut all_desc);
                    let mut matches = Vec::new();
                    for desc in all_desc {
                        if node_matches_step(desc, step)? {
                            matches.push(desc);
                        }
                    }
                    matches
                }
            };
            next_matches.extend(step_matches);
        }
        current_matches = next_matches;
    }

    Ok(current_matches.into_iter().map(EvalResult::Node).collect())
}

/// Find nodes matching a pattern step from a given root.
/// For the Root combinator, this searches all descendants for the type.
fn find_matching_nodes<'a>(
    node: &'a dyn AqNode,
    step: &PatternStep,
) -> Result<Vec<&'a dyn AqNode>, EvalError> {
    let mut results = Vec::new();

    // Check if the node itself matches
    if node_matches_step(node, step)? {
        results.push(node);
    }

    // Search all descendants
    let mut all_desc = Vec::new();
    collect_descendants_raw(node, None, 0, &mut all_desc);
    for desc in all_desc {
        if node_matches_step(desc, step)? {
            results.push(desc);
        }
    }

    Ok(results)
}

/// Check if a node fully matches a pattern step (type + field constraint + predicates).
fn node_matches_step(
    node: &dyn AqNode,
    step: &PatternStep,
) -> Result<bool, EvalError> {
    // Check type
    if node.node_type() != step.node_type {
        return Ok(false);
    }
    // Check field constraint: e.g., name:(identifier) requires .name to be of type "identifier"
    if let Some((ref field_name, ref expected_type)) = step.field_constraint {
        match node.child_by_field(field_name) {
            Some(child) => {
                if child.node_type() != expected_type.as_str() {
                    return Ok(false);
                }
            }
            None => return Ok(false),
        }
    }
    // Check predicates
    check_predicates(node, &step.predicates)
}

/// Check if a node satisfies all predicates.
fn check_predicates(
    node: &dyn AqNode,
    predicates: &[PatternPredicate],
) -> Result<bool, EvalError> {
    for pred in predicates {
        let node_val = match pred.field.as_str() {
            "type" => serde_json::Value::String(node.node_type().to_string()),
            "text" => match node.text().or(node.subtree_text()) {
                Some(t) => serde_json::Value::String(t.to_string()),
                None => serde_json::Value::Null,
            },
            "start" | "line" => json_number(node.start_line() as f64),
            "end" => json_number(node.end_line() as f64),
            "file" => match node.source_file() {
                Some(f) => serde_json::Value::String(f.to_string()),
                None => serde_json::Value::Null,
            },
            other => {
                return Err(EvalError {
                    message: format!("Unknown predicate field: @{}", other),
                });
            }
        };

        let pred_val = serde_json::Value::String(pred.value.clone());

        let matches = compare_values(&node_val, &pred.op, &pred_val)?;
        if !matches {
            return Ok(false);
        }
    }
    Ok(true)
}

// ---------------------------------------------------------------------------
// Builtins
// ---------------------------------------------------------------------------

fn eval_builtin<'a>(
    name: &str,
    args: &[Expr],
    input: &EvalResult<'a>,
    ctx: &EvalContext<'a>,
) -> Result<Vec<EvalResult<'a>>, EvalError> {
    match name {
        "length" => {
            let len = match input {
                EvalResult::Node(n) => n.named_children().len(),
                EvalResult::Value(serde_json::Value::String(s)) => s.len(),
                EvalResult::Value(serde_json::Value::Array(a)) => a.len(),
                EvalResult::Value(serde_json::Value::Object(m)) => m.len(),
                EvalResult::Value(serde_json::Value::Null) => 0,
                _ => 0,
            };
            Ok(vec![EvalResult::Value(json_number(len as f64))])
        }

        "keys" => match input {
            EvalResult::Value(serde_json::Value::Object(m)) => {
                let keys: Vec<serde_json::Value> = m
                    .keys()
                    .map(|k| serde_json::Value::String(k.clone()))
                    .collect();
                Ok(vec![EvalResult::Value(serde_json::Value::Array(keys))])
            }
            _ => Ok(vec![EvalResult::Value(serde_json::Value::Array(vec![]))]),
        },

        "not" => {
            let truthy = match input {
                EvalResult::Node(_) => true,
                EvalResult::Value(v) => json_is_truthy(v),
            };
            Ok(vec![EvalResult::Value(serde_json::Value::Bool(!truthy))])
        }

        "empty" => Ok(vec![]),

        "first" => match input {
            EvalResult::Node(n) => {
                let children = n.named_children();
                if let Some(c) = children.first() {
                    Ok(vec![EvalResult::Node(*c)])
                } else {
                    Ok(vec![])
                }
            }
            EvalResult::Value(serde_json::Value::Array(a)) => {
                if let Some(v) = a.first() {
                    Ok(vec![EvalResult::Value(v.clone())])
                } else {
                    Ok(vec![])
                }
            }
            _ => Ok(vec![input.clone()]),
        },

        "last" => match input {
            EvalResult::Node(n) => {
                let children = n.named_children();
                if let Some(c) = children.last() {
                    Ok(vec![EvalResult::Node(*c)])
                } else {
                    Ok(vec![])
                }
            }
            EvalResult::Value(serde_json::Value::Array(a)) => {
                if let Some(v) = a.last() {
                    Ok(vec![EvalResult::Value(v.clone())])
                } else {
                    Ok(vec![])
                }
            }
            _ => Ok(vec![input.clone()]),
        },

        "has" => {
            if args.len() != 1 {
                return Err(EvalError {
                    message: "has() requires exactly one argument".into(),
                });
            }
            let arg_results = eval_filter(&args[0], input, ctx)?;
            let key = match arg_results.first() {
                Some(EvalResult::Value(serde_json::Value::String(s))) => s.clone(),
                _ => {
                    return Err(EvalError {
                        message: "has() argument must evaluate to a string".into(),
                    })
                }
            };
            let result = match input {
                EvalResult::Node(n) => n.child_by_field(&key).is_some(),
                EvalResult::Value(serde_json::Value::Object(m)) => m.contains_key(&key),
                _ => false,
            };
            Ok(vec![EvalResult::Value(serde_json::Value::Bool(result))])
        }

        "startswith" => {
            if args.len() != 1 {
                return Err(EvalError {
                    message: "startswith() requires exactly one argument".into(),
                });
            }
            let input_str = result_to_string(input);
            let arg_results = eval_filter(&args[0], input, ctx)?;
            let prefix = match arg_results.first() {
                Some(EvalResult::Value(serde_json::Value::String(s))) => s.clone(),
                _ => {
                    return Err(EvalError {
                        message: "startswith() argument must evaluate to a string".into(),
                    })
                }
            };
            let result = input_str.map_or(false, |s| s.starts_with(&prefix));
            Ok(vec![EvalResult::Value(serde_json::Value::Bool(result))])
        }

        "endswith" => {
            if args.len() != 1 {
                return Err(EvalError {
                    message: "endswith() requires exactly one argument".into(),
                });
            }
            let input_str = result_to_string(input);
            let arg_results = eval_filter(&args[0], input, ctx)?;
            let suffix = match arg_results.first() {
                Some(EvalResult::Value(serde_json::Value::String(s))) => s.clone(),
                _ => {
                    return Err(EvalError {
                        message: "endswith() argument must evaluate to a string".into(),
                    })
                }
            };
            let result = input_str.map_or(false, |s| s.ends_with(&suffix));
            Ok(vec![EvalResult::Value(serde_json::Value::Bool(result))])
        }

        "contains" => {
            if args.len() != 1 {
                return Err(EvalError {
                    message: "contains() requires exactly one argument".into(),
                });
            }
            let input_str = result_to_string(input);
            let arg_results = eval_filter(&args[0], input, ctx)?;
            let needle = match arg_results.first() {
                Some(EvalResult::Value(serde_json::Value::String(s))) => s.clone(),
                _ => {
                    return Err(EvalError {
                        message: "contains() argument must evaluate to a string".into(),
                    })
                }
            };
            let result = input_str.map_or(false, |s| s.contains(&needle));
            Ok(vec![EvalResult::Value(serde_json::Value::Bool(result))])
        }

        "map" => {
            if args.len() != 1 {
                return Err(EvalError {
                    message: "map() requires exactly one argument".into(),
                });
            }
            match input {
                EvalResult::Value(serde_json::Value::Array(arr)) => {
                    let mut results = Vec::new();
                    for item in arr {
                        let item_result = EvalResult::Value(item.clone());
                        results.extend(eval_filter(&args[0], &item_result, ctx)?);
                    }
                    let arr: Vec<serde_json::Value> = results.iter().map(result_to_json).collect();
                    Ok(vec![EvalResult::Value(serde_json::Value::Array(arr))])
                }
                _ => Err(EvalError {
                    message: "map() requires array input".into(),
                }),
            }
        }

        "select" => {
            // select as a builtin (same as Expr::Select)
            if args.len() != 1 {
                return Err(EvalError {
                    message: "select() requires exactly one argument".into(),
                });
            }
            let results = eval_filter(&args[0], input, ctx)?;
            if is_truthy(&results) {
                Ok(vec![input.clone()])
            } else {
                Ok(vec![])
            }
        }

        "type" => {
            let type_str = match input {
                EvalResult::Node(_) => "node",
                EvalResult::Value(serde_json::Value::Null) => "null",
                EvalResult::Value(serde_json::Value::Bool(_)) => "boolean",
                EvalResult::Value(serde_json::Value::Number(_)) => "number",
                EvalResult::Value(serde_json::Value::String(_)) => "string",
                EvalResult::Value(serde_json::Value::Array(_)) => "array",
                EvalResult::Value(serde_json::Value::Object(_)) => "object",
            };
            Ok(vec![EvalResult::Value(serde_json::Value::String(
                type_str.into(),
            ))])
        }

        "sort_by" => {
            if args.len() != 1 {
                return Err(EvalError { message: "sort_by() requires exactly one argument".into() });
            }
            match input {
                EvalResult::Value(serde_json::Value::Array(arr)) => {
                    let mut keyed: Vec<(serde_json::Value, serde_json::Value)> = Vec::new();
                    for item in arr {
                        let item_result = EvalResult::Value(item.clone());
                        let key_results = eval_filter(&args[0], &item_result, ctx)?;
                        let key = key_results.first().map(result_to_json).unwrap_or(serde_json::Value::Null);
                        keyed.push((key, item.clone()));
                    }
                    keyed.sort_by(|(a, _), (b, _)| compare_json_values(a, b));
                    let sorted: Vec<serde_json::Value> = keyed.into_iter().map(|(_, v)| v).collect();
                    Ok(vec![EvalResult::Value(serde_json::Value::Array(sorted))])
                }
                _ => Err(EvalError { message: "sort_by() requires array input".into() }),
            }
        }

        "group_by" => {
            if args.len() != 1 {
                return Err(EvalError { message: "group_by() requires exactly one argument".into() });
            }
            match input {
                EvalResult::Value(serde_json::Value::Array(arr)) => {
                    // Preserve insertion order of keys
                    let mut key_order: Vec<serde_json::Value> = Vec::new();
                    let mut groups: Vec<(serde_json::Value, Vec<serde_json::Value>)> = Vec::new();
                    for item in arr {
                        let item_result = EvalResult::Value(item.clone());
                        let key_results = eval_filter(&args[0], &item_result, ctx)?;
                        let key = key_results.first().map(result_to_json).unwrap_or(serde_json::Value::Null);
                        if let Some(group) = groups.iter_mut().find(|(k, _)| json_values_equal(k, &key)) {
                            group.1.push(item.clone());
                        } else {
                            key_order.push(key.clone());
                            groups.push((key, vec![item.clone()]));
                        }
                    }
                    let result: Vec<serde_json::Value> = groups
                        .into_iter()
                        .map(|(_, items)| serde_json::Value::Array(items))
                        .collect();
                    Ok(vec![EvalResult::Value(serde_json::Value::Array(result))])
                }
                _ => Err(EvalError { message: "group_by() requires array input".into() }),
            }
        }

        "unique_by" => {
            if args.len() != 1 {
                return Err(EvalError { message: "unique_by() requires exactly one argument".into() });
            }
            match input {
                EvalResult::Value(serde_json::Value::Array(arr)) => {
                    let mut seen_keys: Vec<serde_json::Value> = Vec::new();
                    let mut result = Vec::new();
                    for item in arr {
                        let item_result = EvalResult::Value(item.clone());
                        let key_results = eval_filter(&args[0], &item_result, ctx)?;
                        let key = key_results.first().map(result_to_json).unwrap_or(serde_json::Value::Null);
                        if !seen_keys.iter().any(|k| json_values_equal(k, &key)) {
                            seen_keys.push(key);
                            result.push(item.clone());
                        }
                    }
                    Ok(vec![EvalResult::Value(serde_json::Value::Array(result))])
                }
                _ => Err(EvalError { message: "unique_by() requires array input".into() }),
            }
        }

        "limit" => {
            if args.len() != 1 {
                return Err(EvalError { message: "limit() requires exactly one argument".into() });
            }
            let n = match &args[0] {
                Expr::Literal(Value::Number(n)) => *n as usize,
                _ => {
                    // Evaluate the argument to get a number
                    let results = eval_filter(&args[0], input, ctx)?;
                    match results.first() {
                        Some(EvalResult::Value(serde_json::Value::Number(n))) =>
                            n.as_f64().unwrap_or(0.0) as usize,
                        _ => return Err(EvalError { message: "limit() argument must be a number".into() }),
                    }
                }
            };
            match input {
                EvalResult::Value(serde_json::Value::Array(arr)) => {
                    let limited: Vec<serde_json::Value> = arr.iter().take(n).cloned().collect();
                    Ok(vec![EvalResult::Value(serde_json::Value::Array(limited))])
                }
                _ => Err(EvalError { message: "limit() requires array input".into() }),
            }
        }

        // --- Additional builtins ---

        "flatten" => {
            match input {
                EvalResult::Value(serde_json::Value::Array(arr)) => {
                    let mut result = Vec::new();
                    for item in arr {
                        match item {
                            serde_json::Value::Array(inner) => result.extend(inner.iter().cloned()),
                            other => result.push(other.clone()),
                        }
                    }
                    Ok(vec![EvalResult::Value(serde_json::Value::Array(result))])
                }
                _ => Err(EvalError { message: "flatten requires array input".into() }),
            }
        }

        "add" => {
            match input {
                EvalResult::Value(serde_json::Value::Array(arr)) if arr.is_empty() => {
                    Ok(vec![EvalResult::Value(serde_json::Value::Null)])
                }
                EvalResult::Value(serde_json::Value::Array(arr)) => {
                    // Detect type from first element
                    match &arr[0] {
                        serde_json::Value::Number(_) => {
                            let sum: f64 = arr.iter().map(|v| {
                                v.as_f64().unwrap_or(0.0)
                            }).sum();
                            Ok(vec![EvalResult::Value(json_number(sum))])
                        }
                        serde_json::Value::String(_) => {
                            let mut result = String::new();
                            for v in arr {
                                if let serde_json::Value::String(s) = v {
                                    result.push_str(s);
                                }
                            }
                            Ok(vec![EvalResult::Value(serde_json::Value::String(result))])
                        }
                        serde_json::Value::Array(_) => {
                            let mut result = Vec::new();
                            for v in arr {
                                if let serde_json::Value::Array(inner) = v {
                                    result.extend(inner.iter().cloned());
                                }
                            }
                            Ok(vec![EvalResult::Value(serde_json::Value::Array(result))])
                        }
                        _ => Ok(vec![EvalResult::Value(serde_json::Value::Null)]),
                    }
                }
                _ => Err(EvalError { message: "add requires array input".into() }),
            }
        }

        "any" => {
            match input {
                EvalResult::Value(serde_json::Value::Array(arr)) => {
                    let result = arr.iter().any(|v| !matches!(v, serde_json::Value::Null | serde_json::Value::Bool(false)));
                    Ok(vec![EvalResult::Value(serde_json::Value::Bool(result))])
                }
                _ => Err(EvalError { message: "any requires array input".into() }),
            }
        }

        "all" => {
            match input {
                EvalResult::Value(serde_json::Value::Array(arr)) => {
                    let result = arr.iter().all(|v| !matches!(v, serde_json::Value::Null | serde_json::Value::Bool(false)));
                    Ok(vec![EvalResult::Value(serde_json::Value::Bool(result))])
                }
                _ => Err(EvalError { message: "all requires array input".into() }),
            }
        }

        "reverse" => {
            match input {
                EvalResult::Value(serde_json::Value::Array(arr)) => {
                    let mut result = arr.clone();
                    result.reverse();
                    Ok(vec![EvalResult::Value(serde_json::Value::Array(result))])
                }
                EvalResult::Value(serde_json::Value::String(s)) => {
                    let result: String = s.chars().rev().collect();
                    Ok(vec![EvalResult::Value(serde_json::Value::String(result))])
                }
                _ => Err(EvalError { message: "reverse requires array or string input".into() }),
            }
        }

        "join" => {
            if args.len() != 1 {
                return Err(EvalError { message: "join() requires exactly one argument (separator)".into() });
            }
            let sep = match &args[0] {
                Expr::Literal(Value::String(s)) => s.clone(),
                _ => {
                    let results = eval_filter(&args[0], input, ctx)?;
                    match results.first() {
                        Some(EvalResult::Value(serde_json::Value::String(s))) => s.clone(),
                        _ => return Err(EvalError { message: "join() separator must be a string".into() }),
                    }
                }
            };
            match input {
                EvalResult::Value(serde_json::Value::Array(arr)) => {
                    let parts: Vec<String> = arr.iter().map(|v| match v {
                        serde_json::Value::String(s) => s.clone(),
                        serde_json::Value::Null => String::new(),
                        other => other.to_string(),
                    }).collect();
                    Ok(vec![EvalResult::Value(serde_json::Value::String(parts.join(&sep)))])
                }
                _ => Err(EvalError { message: "join() requires array input".into() }),
            }
        }

        "split" => {
            if args.len() != 1 {
                return Err(EvalError { message: "split() requires exactly one argument (delimiter)".into() });
            }
            let delim = match &args[0] {
                Expr::Literal(Value::String(s)) => s.clone(),
                _ => {
                    let results = eval_filter(&args[0], input, ctx)?;
                    match results.first() {
                        Some(EvalResult::Value(serde_json::Value::String(s))) => s.clone(),
                        _ => return Err(EvalError { message: "split() delimiter must be a string".into() }),
                    }
                }
            };
            let input_str = match input {
                EvalResult::Value(serde_json::Value::String(s)) => s.clone(),
                EvalResult::Node(n) => n.text().or_else(|| n.subtree_text()).unwrap_or("").to_string(),
                _ => return Err(EvalError { message: "split() requires string input".into() }),
            };
            let parts: Vec<serde_json::Value> = input_str
                .split(&delim)
                .map(|s| serde_json::Value::String(s.to_string()))
                .collect();
            Ok(vec![EvalResult::Value(serde_json::Value::Array(parts))])
        }

        "test" => {
            if args.len() != 1 {
                return Err(EvalError { message: "test() requires exactly one argument (regex)".into() });
            }
            let pattern = match &args[0] {
                Expr::Literal(Value::String(s)) => s.clone(),
                _ => {
                    let results = eval_filter(&args[0], input, ctx)?;
                    match results.first() {
                        Some(EvalResult::Value(serde_json::Value::String(s))) => s.clone(),
                        _ => return Err(EvalError { message: "test() pattern must be a string".into() }),
                    }
                }
            };
            let input_str = match input {
                EvalResult::Value(serde_json::Value::String(s)) => s.clone(),
                EvalResult::Node(n) => n.text().or_else(|| n.subtree_text()).unwrap_or("").to_string(),
                _ => return Err(EvalError { message: "test() requires string input".into() }),
            };
            let re = regex::Regex::new(&pattern).map_err(|e| EvalError {
                message: format!("Invalid regex in test(): {}", e),
            })?;
            Ok(vec![EvalResult::Value(serde_json::Value::Bool(re.is_match(&input_str)))])
        }

        "to_number" | "tonumber" => {
            match input {
                EvalResult::Value(serde_json::Value::Number(_)) => Ok(vec![input.clone()]),
                EvalResult::Value(serde_json::Value::String(s)) => {
                    let n: f64 = s.parse().map_err(|_| EvalError {
                        message: format!("Cannot convert to number: {:?}", s),
                    })?;
                    Ok(vec![EvalResult::Value(json_number(n))])
                }
                _ => Err(EvalError { message: "to_number requires number or string input".into() }),
            }
        }

        "to_string" | "tostring" => {
            let s = match input {
                EvalResult::Value(serde_json::Value::String(s)) => s.clone(),
                EvalResult::Value(serde_json::Value::Null) => "null".into(),
                EvalResult::Value(v) => v.to_string(),
                EvalResult::Node(n) => n.text().or_else(|| n.subtree_text()).unwrap_or("").to_string(),
            };
            Ok(vec![EvalResult::Value(serde_json::Value::String(s))])
        }

        "ascii_downcase" => {
            let s = match input {
                EvalResult::Value(serde_json::Value::String(s)) => s.to_lowercase(),
                EvalResult::Node(n) => n.text().or_else(|| n.subtree_text()).unwrap_or("").to_lowercase(),
                _ => return Err(EvalError { message: "ascii_downcase requires string input".into() }),
            };
            Ok(vec![EvalResult::Value(serde_json::Value::String(s))])
        }

        "ascii_upcase" => {
            let s = match input {
                EvalResult::Value(serde_json::Value::String(s)) => s.to_uppercase(),
                EvalResult::Node(n) => n.text().or_else(|| n.subtree_text()).unwrap_or("").to_uppercase(),
                _ => return Err(EvalError { message: "ascii_upcase requires string input".into() }),
            };
            Ok(vec![EvalResult::Value(serde_json::Value::String(s))])
        }

        "count_desc" => {
            // count_desc("type_name") — count descendants of a given type
            if args.len() != 1 {
                return Err(EvalError { message: "count_desc() requires exactly 1 argument".into() });
            }
            let type_results = eval_filter(&args[0], input, ctx)?;
            let type_name = match type_results.first() {
                Some(EvalResult::Value(serde_json::Value::String(s))) => s.clone(),
                _ => return Err(EvalError { message: "count_desc() argument must be a string".into() }),
            };
            match input {
                EvalResult::Node(n) => {
                    let mut all_desc = Vec::new();
                    collect_descendants_raw(*n, None, 0, &mut all_desc);
                    let count = all_desc.iter()
                        .filter(|d| d.node_type() == type_name.as_str())
                        .count();
                    Ok(vec![EvalResult::Value(json_number(count as f64))])
                }
                _ => Ok(vec![EvalResult::Value(json_number(0.0))]),
            }
        }

        "debug" => {
            // debug — print the input to stderr and pass through (useful for debugging queries)
            eprintln!("[debug] {:?}", input);
            Ok(vec![input.clone()])
        }

        "path" => {
            // path — return array of types from root to current node
            match input {
                EvalResult::Node(n) => {
                    let mut types = vec![n.node_type().to_string()];
                    let mut key = node_key(*n);
                    while let Some(&p) = ctx.parent_map.get(&key) {
                        types.push(p.node_type().to_string());
                        key = node_key(p);
                    }
                    types.reverse();
                    let arr: Vec<serde_json::Value> = types.into_iter()
                        .map(serde_json::Value::String)
                        .collect();
                    Ok(vec![EvalResult::Value(serde_json::Value::Array(arr))])
                }
                _ => Ok(vec![EvalResult::Value(serde_json::Value::Array(vec![]))]),
            }
        }

        _ => Err(EvalError {
            message: format!("Unknown builtin: {}", name),
        }),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Convert an EvalResult to a serde_json::Value for serialization.
pub fn result_to_json(r: &EvalResult) -> serde_json::Value {
    match r {
        EvalResult::Node(n) => {
            let mut map = serde_json::Map::new();
            map.insert("@type".into(), serde_json::Value::String(n.node_type().to_string()));
            map.insert("@start".into(), json_number(n.start_line() as f64));
            map.insert("@end".into(), json_number(n.end_line() as f64));
            if let Some(t) = n.text() {
                map.insert("@text".into(), serde_json::Value::String(t.to_string()));
            }
            if let Some(f) = n.source_file() {
                map.insert("@file".into(), serde_json::Value::String(f.to_string()));
            }
            serde_json::Value::Object(map)
        }
        EvalResult::Value(v) => v.clone(),
    }
}

/// jq truthiness: only null and false are falsy
fn is_truthy(results: &[EvalResult]) -> bool {
    if results.is_empty() {
        return false;
    }
    match &results[0] {
        EvalResult::Node(_) => true,
        EvalResult::Value(v) => json_is_truthy(v),
    }
}

fn json_is_truthy(v: &serde_json::Value) -> bool {
    !matches!(v, serde_json::Value::Null | serde_json::Value::Bool(false))
}

fn all_null(results: &[EvalResult]) -> bool {
    results.iter().all(|r| matches!(r, EvalResult::Value(serde_json::Value::Null)))
}

fn value_to_json(val: &Value) -> serde_json::Value {
    match val {
        Value::String(s) => serde_json::Value::String(s.clone()),
        Value::Number(n) => json_number(*n),
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Null => serde_json::Value::Null,
        Value::Array(items) => serde_json::Value::Array(items.iter().map(value_to_json).collect()),
    }
}

fn json_number(n: f64) -> serde_json::Value {
    // Emit integers without decimal point when possible
    if n.fract() == 0.0 && n.abs() < (i64::MAX as f64) {
        serde_json::Value::Number(serde_json::Number::from(n as i64))
    } else {
        serde_json::Number::from_f64(n)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null)
    }
}

fn extract_number(results: &[EvalResult]) -> Result<f64, EvalError> {
    match results.first() {
        Some(EvalResult::Value(serde_json::Value::Number(n))) => {
            n.as_f64().ok_or_else(|| EvalError {
                message: "Cannot convert number to f64".into(),
            })
        }
        Some(EvalResult::Node(n)) => {
            // Allow using start_line/end_line as implicit numbers? No — error
            Err(EvalError {
                message: format!(
                    "Expected number, got node '{}' at line {}",
                    n.node_type(),
                    n.start_line()
                ),
            })
        }
        Some(EvalResult::Value(v)) => Err(EvalError {
            message: format!("Expected number, got {:?}", v),
        }),
        None => Err(EvalError {
            message: "Expected number, got empty result".into(),
        }),
    }
}

fn result_to_string(r: &EvalResult) -> Option<String> {
    match r {
        EvalResult::Value(serde_json::Value::String(s)) => Some(s.clone()),
        EvalResult::Node(n) => n.text().or(n.subtree_text()).map(|s| s.to_string()),
        _ => None,
    }
}

fn compare_values(
    left: &serde_json::Value,
    op: &CmpOp,
    right: &serde_json::Value,
) -> Result<bool, EvalError> {
    match op {
        CmpOp::Eq => Ok(left == right),
        CmpOp::NotEq => Ok(left != right),
        CmpOp::Lt | CmpOp::Gt | CmpOp::Lte | CmpOp::Gte => {
            let l = json_to_f64(left).ok_or_else(|| EvalError {
                message: format!("Cannot compare non-numeric value: {}", left),
            })?;
            let r = json_to_f64(right).ok_or_else(|| EvalError {
                message: format!("Cannot compare non-numeric value: {}", right),
            })?;
            Ok(match op {
                CmpOp::Lt => l < r,
                CmpOp::Gt => l > r,
                CmpOp::Lte => l <= r,
                CmpOp::Gte => l >= r,
                _ => unreachable!(),
            })
        }
        CmpOp::RegexMatch => {
            let s = match left {
                serde_json::Value::String(s) => s.as_str(),
                _ => {
                    return Err(EvalError {
                        message: "Left side of =~ must be a string".into(),
                    })
                }
            };
            let pattern = match right {
                serde_json::Value::String(p) => p.as_str(),
                _ => {
                    return Err(EvalError {
                        message: "Right side of =~ must be a string (regex pattern)".into(),
                    })
                }
            };
            let re = regex::Regex::new(pattern).map_err(|e| EvalError {
                message: format!("Invalid regex '{}': {}", pattern, e),
            })?;
            Ok(re.is_match(s))
        }
    }
}

fn json_to_f64(v: &serde_json::Value) -> Option<f64> {
    match v {
        serde_json::Value::Number(n) => n.as_f64(),
        _ => None,
    }
}

/// Compare two JSON values for ordering (used by sort_by).
/// Order: Null < Bool(false) < Bool(true) < Numbers < Strings
fn compare_json_values(a: &serde_json::Value, b: &serde_json::Value) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    fn type_rank(v: &serde_json::Value) -> u8 {
        match v {
            serde_json::Value::Null => 0,
            serde_json::Value::Bool(false) => 1,
            serde_json::Value::Bool(true) => 2,
            serde_json::Value::Number(_) => 3,
            serde_json::Value::String(_) => 4,
            serde_json::Value::Array(_) => 5,
            serde_json::Value::Object(_) => 6,
        }
    }
    let ra = type_rank(a);
    let rb = type_rank(b);
    if ra != rb {
        return ra.cmp(&rb);
    }
    match (a, b) {
        (serde_json::Value::Number(na), serde_json::Value::Number(nb)) => {
            let fa = na.as_f64().unwrap_or(0.0);
            let fb = nb.as_f64().unwrap_or(0.0);
            fa.partial_cmp(&fb).unwrap_or(Ordering::Equal)
        }
        (serde_json::Value::String(sa), serde_json::Value::String(sb)) => sa.cmp(sb),
        _ => Ordering::Equal,
    }
}

/// Check equality of two JSON values (used by group_by, unique_by).
fn json_values_equal(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    a == b
}

// ---------------------------------------------------------------------------
// Format meta field helpers (@csv, @tsv, @json)
// ---------------------------------------------------------------------------

fn format_csv<'a>(input: &EvalResult<'a>) -> Result<Vec<EvalResult<'a>>, EvalError> {
    match input {
        EvalResult::Value(serde_json::Value::Array(arr)) => {
            let fields: Vec<String> = arr.iter().map(|v| csv_escape(v)).collect();
            Ok(vec![EvalResult::Value(serde_json::Value::String(fields.join(",")))])
        }
        _ => Err(EvalError { message: "@csv requires array input".into() }),
    }
}

fn csv_escape(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => {
            if s.contains(',') || s.contains('"') || s.contains('\n') {
                format!("\"{}\"", s.replace('"', "\"\""))
            } else {
                s.clone()
            }
        }
        serde_json::Value::Null => String::new(),
        serde_json::Value::Bool(b) => if *b { "true".into() } else { "false".into() },
        serde_json::Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

fn format_tsv<'a>(input: &EvalResult<'a>) -> Result<Vec<EvalResult<'a>>, EvalError> {
    match input {
        EvalResult::Value(serde_json::Value::Array(arr)) => {
            let fields: Vec<String> = arr.iter().map(|v| tsv_escape(v)).collect();
            Ok(vec![EvalResult::Value(serde_json::Value::String(fields.join("\t")))])
        }
        _ => Err(EvalError { message: "@tsv requires array input".into() }),
    }
}

fn tsv_escape(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.replace('\t', "\\t").replace('\n', "\\n").replace('\r', "\\r"),
        serde_json::Value::Null => String::new(),
        serde_json::Value::Bool(b) => if *b { "true".into() } else { "false".into() },
        serde_json::Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

fn format_json_meta<'a>(input: &EvalResult<'a>) -> Result<Vec<EvalResult<'a>>, EvalError> {
    let val = result_to_json(input);
    let s = serde_json::to_string(&val).unwrap_or_default();
    Ok(vec![EvalResult::Value(serde_json::Value::String(s))])
}

#[path = "eval_tests.rs"]
#[cfg(test)]
mod tests;
