use tree_sitter::Node;

use super::model::{
    ArgumentKey, Comparison, Expression, NodePattern, PatternKind, Query, Relationship,
    ValuePattern,
};

pub fn matches_query(node: Node, source: &[u8], query: &Query) -> bool {
    matches_node(node, source, &query.anchor)
        && query
            .condition
            .as_ref()
            .is_none_or(|condition| matches_expression(node, source, condition))
}

fn matches_expression(node: Node, source: &[u8], expression: &Expression) -> bool {
    match expression {
        Expression::Relation(relationship, pattern) => {
            matches_relation(node, source, *relationship, pattern)
        }
        Expression::DescendantChain(patterns) => matches_descendant_chain(node, source, patterns),
        Expression::And(left, right) => {
            matches_expression(node, source, left) && matches_expression(node, source, right)
        }
        Expression::Or(left, right) => {
            matches_expression(node, source, left) || matches_expression(node, source, right)
        }
        Expression::Not(expression) => !matches_expression(node, source, expression),
    }
}

fn matches_descendant_chain(node: Node, source: &[u8], patterns: &[NodePattern]) -> bool {
    let Some((pattern, remaining)) = patterns.split_first() else {
        return true;
    };

    let mut cursor = node.walk();
    node.named_children(&mut cursor).any(|child| {
        (matches_node(child, source, pattern) && matches_descendant_chain(child, source, remaining))
            || matches_descendant_chain(child, source, patterns)
    })
}

fn matches_relation(
    node: Node,
    source: &[u8],
    relationship: Relationship,
    pattern: &NodePattern,
) -> bool {
    if matches!(pattern.kind, PatternKind::Argument(_)) {
        return matches_argument(node, source, pattern);
    }

    match relationship {
        Relationship::Child => {
            let mut cursor = node.walk();
            node.named_children(&mut cursor)
                .any(|child| matches_node(child, source, pattern))
        }
        Relationship::Descendant => contains_descendant(node, source, pattern),
        Relationship::Ancestor => {
            let mut parent = node.parent();
            while let Some(node) = parent {
                if matches_node(node, source, pattern) {
                    return true;
                }
                parent = node.parent();
            }
            false
        }
    }
}

fn contains_descendant(node: Node, source: &[u8], pattern: &NodePattern) -> bool {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).any(|child| {
        matches_node(child, source, pattern) || contains_descendant(child, source, pattern)
    })
}

fn matches_node(node: Node, source: &[u8], pattern: &NodePattern) -> bool {
    let target = match &pattern.kind {
        PatternKind::Call if node.kind() == "call" => node.child_by_field_name("function"),
        PatternKind::Class if node.kind() == "class_definition" => node.child_by_field_name("name"),
        PatternKind::Function if node.kind() == "function_definition" => {
            node.child_by_field_name("name")
        }
        PatternKind::Import
            if matches!(node.kind(), "import_statement" | "import_from_statement") =>
        {
            return import_matches(node, source, &pattern.value);
        }
        _ => None,
    };

    target
        .and_then(|target| target.utf8_text(source).ok())
        .is_some_and(|actual| value_matches(&pattern.value, actual))
}

fn import_matches(node: Node, source: &[u8], pattern: &ValuePattern) -> bool {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).any(|child| {
        child
            .utf8_text(source)
            .is_ok_and(|actual| value_matches(pattern, actual))
            || import_matches(child, source, pattern)
    })
}

fn matches_argument(node: Node, source: &[u8], pattern: &NodePattern) -> bool {
    let PatternKind::Argument(key) = &pattern.kind else {
        return false;
    };
    let Some(arguments) = node.child_by_field_name("arguments") else {
        return false;
    };

    let mut cursor = arguments.walk();
    let arguments: Vec<_> = arguments.named_children(&mut cursor).collect();
    match key {
        ArgumentKey::Any => arguments
            .into_iter()
            .filter_map(argument_value)
            .any(|argument| node_value_matches(argument, source, &pattern.value)),
        ArgumentKey::Position(position) => arguments
            .into_iter()
            .filter(|argument| argument.kind() != "keyword_argument")
            .nth(*position)
            .is_some_and(|argument| node_value_matches(argument, source, &pattern.value)),
        ArgumentKey::Keyword(expected) => arguments.into_iter().any(|argument| {
            argument.kind() == "keyword_argument"
                && argument
                    .child_by_field_name("name")
                    .and_then(|name| name.utf8_text(source).ok())
                    .is_some_and(|name| name == expected)
                && argument
                    .child_by_field_name("value")
                    .is_some_and(|value| node_value_matches(value, source, &pattern.value))
        }),
    }
}

fn argument_value(argument: Node) -> Option<Node> {
    if argument.kind() == "keyword_argument" {
        argument.child_by_field_name("value")
    } else {
        Some(argument)
    }
}

fn node_value_matches(node: Node, source: &[u8], pattern: &ValuePattern) -> bool {
    node.utf8_text(source)
        .is_ok_and(|actual| value_matches(pattern, actual))
}

fn value_matches(pattern: &ValuePattern, actual: &str) -> bool {
    match pattern {
        ValuePattern::Any => true,
        ValuePattern::Exact(expected) => {
            actual == expected
                || strip_string_quotes(actual).is_some_and(|actual| actual == expected)
        }
        ValuePattern::Contains(expected) => actual.contains(expected),
        ValuePattern::StartsWith(expected) => actual.starts_with(expected),
        ValuePattern::EndsWith(expected) => actual.ends_with(expected),
        ValuePattern::Regex(expression) => expression.is_match(actual),
        ValuePattern::Numeric(comparison, expected) => actual
            .replace('_', "")
            .parse::<f64>()
            .is_ok_and(|actual| compare(actual, *comparison, *expected)),
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
