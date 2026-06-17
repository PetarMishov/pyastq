use tree_sitter::Node;

use super::model::{
    ArgumentKey, Comparison, Expression, MatchCaptures, NodePattern, PatternKind, Query,
    Relationship, ValuePattern,
};
use super::resolver::{NameResolution, NameResolver};

pub fn captures_query(
    node: Node,
    source: &[u8],
    query: &Query,
    resolver: &NameResolver,
) -> Vec<MatchCaptures> {
    match_node(
        node,
        source,
        &query.anchor,
        resolver,
        &MatchState::default(),
    )
    .into_iter()
    .flat_map(|state| {
        if let Some(condition) = &query.condition {
            match_expression(node, source, condition, resolver, &state)
                .into_iter()
                .map(|state| state.captures)
                .collect()
        } else {
            vec![state.captures]
        }
    })
    .collect()
}

#[derive(Clone, Debug, Default)]
struct MatchState {
    captures: MatchCaptures,
}

impl MatchState {
    fn bind_capture(&self, name: &str, actual: &str) -> Option<Self> {
        match self.captures.get(name) {
            Some(expected) if expected == actual => Some(self.clone()),
            Some(_) => None,
            None => {
                let mut next = self.clone();
                next.captures.insert(name.to_owned(), actual.to_owned());
                Some(next)
            }
        }
    }
}

fn match_expression(
    node: Node,
    source: &[u8],
    expression: &Expression,
    resolver: &NameResolver,
    state: &MatchState,
) -> Vec<MatchState> {
    match expression {
        Expression::Relation(relationship, pattern) => {
            match_relation(node, source, *relationship, pattern, resolver, state)
        }
        Expression::DescendantChain(patterns) => {
            match_descendant_chain(node, source, patterns, resolver, state)
        }
        Expression::And(left, right) => match_expression(node, source, left, resolver, state)
            .into_iter()
            .flat_map(|state| match_expression(node, source, right, resolver, &state))
            .collect(),
        Expression::Or(left, right) => {
            let mut matches = match_expression(node, source, left, resolver, state);
            matches.extend(match_expression(node, source, right, resolver, state));
            matches
        }
        Expression::Not(expression) => {
            if match_expression(node, source, expression, resolver, state).is_empty() {
                vec![state.clone()]
            } else {
                Vec::new()
            }
        }
    }
}

fn match_descendant_chain(
    node: Node,
    source: &[u8],
    patterns: &[NodePattern],
    resolver: &NameResolver,
    state: &MatchState,
) -> Vec<MatchState> {
    let Some((pattern, remaining)) = patterns.split_first() else {
        return vec![state.clone()];
    };

    let mut cursor = node.walk();
    let mut matches = Vec::new();
    for child in node.named_children(&mut cursor) {
        for state in match_node(child, source, pattern, resolver, state) {
            matches.extend(match_descendant_chain(
                child, source, remaining, resolver, &state,
            ));
        }
        matches.extend(match_descendant_chain(
            child, source, patterns, resolver, state,
        ));
    }
    matches
}

fn match_relation(
    node: Node,
    source: &[u8],
    relationship: Relationship,
    pattern: &NodePattern,
    resolver: &NameResolver,
    state: &MatchState,
) -> Vec<MatchState> {
    if matches!(pattern.kind, PatternKind::Argument(_)) {
        return match_argument(node, source, pattern, state);
    }

    match relationship {
        Relationship::Child => {
            let mut cursor = node.walk();
            node.named_children(&mut cursor)
                .flat_map(|child| match_node(child, source, pattern, resolver, state))
                .collect()
        }
        Relationship::Descendant => contains_descendant(node, source, pattern, resolver, state),
        Relationship::Ancestor => {
            let mut parent = node.parent();
            let mut matches = Vec::new();
            while let Some(node) = parent {
                matches.extend(match_node(node, source, pattern, resolver, state));
                parent = node.parent();
            }
            matches
        }
    }
}

fn contains_descendant(
    node: Node,
    source: &[u8],
    pattern: &NodePattern,
    resolver: &NameResolver,
    state: &MatchState,
) -> Vec<MatchState> {
    let mut cursor = node.walk();
    let mut matches = Vec::new();
    for child in node.named_children(&mut cursor) {
        matches.extend(match_node(child, source, pattern, resolver, state));
        matches.extend(contains_descendant(child, source, pattern, resolver, state));
    }
    matches
}

fn match_node(
    node: Node,
    source: &[u8],
    pattern: &NodePattern,
    resolver: &NameResolver,
    state: &MatchState,
) -> Vec<MatchState> {
    let target = match &pattern.kind {
        PatternKind::Call if node.kind() == "call" => node.child_by_field_name("function"),
        PatternKind::Class if node.kind() == "class_definition" => node.child_by_field_name("name"),
        PatternKind::Function if node.kind() == "function_definition" => {
            node.child_by_field_name("name")
        }
        PatternKind::Import
            if matches!(node.kind(), "import_statement" | "import_from_statement") =>
        {
            return match_import(node, source, &pattern.value, state);
        }
        _ => None,
    };

    let Some(actual) = target.and_then(|target| target.utf8_text(source).ok()) else {
        return Vec::new();
    };
    if matches!(pattern.kind, PatternKind::Call) {
        call_value_matches(node, resolver, &pattern.value, actual, state)
    } else {
        match_value(&pattern.value, actual, state)
            .into_iter()
            .collect()
    }
}

fn call_value_matches(
    node: Node,
    resolver: &NameResolver,
    pattern: &ValuePattern,
    actual: &str,
    state: &MatchState,
) -> Vec<MatchState> {
    match resolver.resolve(node, actual) {
        NameResolution::Canonical(canonical) => {
            let mut matches: Vec<_> = match_value(pattern, actual, state).into_iter().collect();
            matches.extend(match_value(pattern, &canonical, state));
            matches
        }
        NameResolution::ShadowedImport if matches!(pattern, ValuePattern::Exact(_)) => Vec::new(),
        NameResolution::ShadowedImport | NameResolution::Unresolved => {
            match_value(pattern, actual, state).into_iter().collect()
        }
    }
}

fn match_import(
    node: Node,
    source: &[u8],
    pattern: &ValuePattern,
    state: &MatchState,
) -> Vec<MatchState> {
    let mut cursor = node.walk();
    let mut matches = Vec::new();
    for child in node.named_children(&mut cursor) {
        if let Ok(actual) = child.utf8_text(source) {
            matches.extend(match_value(pattern, actual, state));
        }
        matches.extend(match_import(child, source, pattern, state));
    }
    matches
}

fn match_argument(
    node: Node,
    source: &[u8],
    pattern: &NodePattern,
    state: &MatchState,
) -> Vec<MatchState> {
    let PatternKind::Argument(key) = &pattern.kind else {
        return Vec::new();
    };
    let Some(arguments) = node.child_by_field_name("arguments") else {
        return Vec::new();
    };

    let mut cursor = arguments.walk();
    let arguments: Vec<_> = arguments.named_children(&mut cursor).collect();
    match key {
        ArgumentKey::Any => arguments
            .into_iter()
            .filter_map(argument_value)
            .flat_map(|argument| node_value_matches(argument, source, &pattern.value, state))
            .collect(),
        ArgumentKey::Position(position) => arguments
            .into_iter()
            .filter(|argument| argument.kind() != "keyword_argument")
            .nth(*position)
            .map(|argument| node_value_matches(argument, source, &pattern.value, state))
            .unwrap_or_default(),
        ArgumentKey::Keyword(expected) => arguments
            .into_iter()
            .filter_map(|argument| {
                (argument.kind() == "keyword_argument"
                    && argument
                        .child_by_field_name("name")
                        .and_then(|name| name.utf8_text(source).ok())
                        .is_some_and(|name| name == expected))
                .then(|| argument.child_by_field_name("value"))
                .flatten()
            })
            .flat_map(|value| node_value_matches(value, source, &pattern.value, state))
            .collect(),
    }
}

fn argument_value(argument: Node) -> Option<Node> {
    if argument.kind() == "keyword_argument" {
        argument.child_by_field_name("value")
    } else {
        Some(argument)
    }
}

fn node_value_matches(
    node: Node,
    source: &[u8],
    pattern: &ValuePattern,
    state: &MatchState,
) -> Vec<MatchState> {
    node.utf8_text(source)
        .ok()
        .and_then(|actual| match_value(pattern, actual, state))
        .into_iter()
        .collect()
}

fn match_value(pattern: &ValuePattern, actual: &str, state: &MatchState) -> Option<MatchState> {
    match pattern {
        ValuePattern::Any => Some(state.clone()),
        ValuePattern::Exact(expected) => (actual == expected
            || strip_string_quotes(actual).is_some_and(|actual| actual == expected))
        .then(|| state.clone()),
        ValuePattern::Contains(expected) => actual.contains(expected).then(|| state.clone()),
        ValuePattern::StartsWith(expected) => actual.starts_with(expected).then(|| state.clone()),
        ValuePattern::EndsWith(expected) => actual.ends_with(expected).then(|| state.clone()),
        ValuePattern::Regex(expression) => expression.is_match(actual).then(|| state.clone()),
        ValuePattern::Numeric(comparison, expected) => actual
            .replace('_', "")
            .parse::<f64>()
            .is_ok_and(|actual| compare(actual, *comparison, *expected))
            .then(|| state.clone()),
        ValuePattern::Capture(name) => state.bind_capture(name, actual),
    }
}

fn strip_string_quotes(value: &str) -> Option<&str> {
    for quote in ["'''", "\"\"\"", "'", "\""] {
        if value.len() >= quote.len() * 2 && value.starts_with(quote) && value.ends_with(quote) {
            return value
                .strip_prefix(quote)
                .and_then(|value| value.strip_suffix(quote));
        }
    }
    None
}

fn compare(actual: f64, comparison: Comparison, expected: f64) -> bool {
    match comparison {
        Comparison::Greater => actual > expected,
        Comparison::GreaterOrEqual => actual >= expected,
        Comparison::Less => actual < expected,
        Comparison::LessOrEqual => actual <= expected,
    }
}
