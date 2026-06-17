use regex::Regex;

use super::lexer::{RelationToken, Token, tokenize};
use super::model::{
    ArgumentKey, Comparison, Expression, NodePattern, PatternKind, Query, QueryVariables,
    Relationship, ValuePattern,
};

#[cfg(test)]
pub fn parse_query(query: &str) -> Result<Query, String> {
    parse_query_with_variables(query, &QueryVariables::new())
}

pub fn parse_query_with_variables(
    query: &str,
    variables: &QueryVariables,
) -> Result<Query, String> {
    Parser::new(tokenize(query)?, variables).parse()
}

struct Parser<'a> {
    tokens: Vec<Token>,
    position: usize,
    variables: &'a QueryVariables,
}

impl<'a> Parser<'a> {
    fn new(tokens: Vec<Token>, variables: &'a QueryVariables) -> Self {
        Self {
            tokens,
            position: 0,
            variables,
        }
    }

    fn parse(mut self) -> Result<Query, String> {
        let anchor = self.parse_pattern()?;
        let condition = if self.is_at_end() {
            None
        } else if matches!(self.peek(), Some(Token::Arrow)) {
            Some(self.parse_or()?)
        } else {
            self.expect(&Token::And, "expected `AND` or `->` after the anchor")?;
            Some(self.parse_or()?)
        };

        if let Some(token) = self.peek() {
            return Err(format!("unexpected token `{token:?}`"));
        }
        Ok(Query { anchor, condition })
    }

    fn parse_or(&mut self) -> Result<Expression, String> {
        let mut expression = self.parse_and()?;
        while self.consume(&Token::Or) {
            expression = Expression::Or(Box::new(expression), Box::new(self.parse_and()?));
        }
        Ok(expression)
    }

    fn parse_and(&mut self) -> Result<Expression, String> {
        let mut expression = self.parse_primary()?;
        while self.consume(&Token::And) {
            expression = Expression::And(Box::new(expression), Box::new(self.parse_primary()?));
        }
        Ok(expression)
    }

    fn parse_primary(&mut self) -> Result<Expression, String> {
        if self.consume(&Token::Not) {
            return Ok(Expression::Not(Box::new(self.parse_primary()?)));
        }

        if self.consume(&Token::LeftParen) {
            let expression = self.parse_or()?;
            self.expect(&Token::RightParen, "expected `)`")?;
            return Ok(expression);
        }

        if self.consume(&Token::Arrow) {
            let mut patterns = vec![self.parse_pattern()?];
            while self.consume(&Token::Arrow) {
                patterns.push(self.parse_pattern()?);
            }
            return Ok(Expression::DescendantChain(patterns));
        }

        let relationship = match self.peek() {
            Some(Token::Relation(relation)) => {
                let relation = *relation;
                self.position += 1;
                relation.into()
            }
            _ => Relationship::Descendant,
        };

        let wrapped = self.consume(&Token::LeftParen);
        let pattern = self.parse_pattern()?;
        if wrapped {
            self.expect(
                &Token::RightParen,
                "expected `)` after relationship pattern",
            )?;
        }
        Ok(Expression::Relation(relationship, pattern))
    }

    fn parse_pattern(&mut self) -> Result<NodePattern, String> {
        let Some(Token::Atom(pattern)) = self.tokens.get(self.position) else {
            return Err("expected a node pattern".to_owned());
        };
        self.position += 1;
        parse_node_pattern(pattern, self.variables)
    }

    fn consume(&mut self, expected: &Token) -> bool {
        if self.tokens.get(self.position) == Some(expected) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, expected: &Token, message: &str) -> Result<(), String> {
        self.consume(expected)
            .then_some(())
            .ok_or_else(|| message.to_owned())
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.position)
    }

    fn is_at_end(&self) -> bool {
        self.position == self.tokens.len()
    }
}

impl From<RelationToken> for Relationship {
    fn from(value: RelationToken) -> Self {
        match value {
            RelationToken::Child => Self::Child,
            RelationToken::Descendant => Self::Descendant,
            RelationToken::Ancestor => Self::Ancestor,
        }
    }
}

fn parse_node_pattern(pattern: &str, variables: &QueryVariables) -> Result<NodePattern, String> {
    let (kind, value) = pattern
        .split_once(':')
        .ok_or_else(|| format!("invalid pattern `{pattern}`; expected `<kind>:<value>`"))?;

    let (kind, value) = match kind {
        "call" => (PatternKind::Call, parse_value(value, variables)?),
        "class" => (PatternKind::Class, parse_value(value, variables)?),
        "function" | "function_definition" => {
            (PatternKind::Function, parse_value(value, variables)?)
        }
        "import" => (PatternKind::Import, parse_value(value, variables)?),
        "argument" => {
            let (key, value) = value.split_once(':').ok_or_else(|| {
                "arguments require a key and value, for example `argument:timeout:30`".to_owned()
            })?;
            (
                PatternKind::Argument(parse_argument_key(key)?),
                parse_value(value, variables)?,
            )
        }
        _ => return Err(format!("unsupported pattern kind `{kind}`")),
    };

    Ok(NodePattern { kind, value })
}

fn parse_argument_key(key: &str) -> Result<ArgumentKey, String> {
    if key == "*" {
        return Ok(ArgumentKey::Any);
    }
    if let Ok(position) = key.parse::<usize>() {
        return Ok(ArgumentKey::Position(position));
    }
    if is_variable_name(key) {
        return Ok(ArgumentKey::Keyword(key.to_owned()));
    }
    Err(format!("invalid argument key `{key}`"))
}

fn parse_value(value: &str, variables: &QueryVariables) -> Result<ValuePattern, String> {
    parse_value_with_stack(value, variables, &mut Vec::new())
}

fn parse_value_with_stack(
    value: &str,
    variables: &QueryVariables,
    stack: &mut Vec<String>,
) -> Result<ValuePattern, String> {
    if let Some(variable) = parse_variable_reference(value)? {
        return match variables.get(variable) {
            Some(replacement) => {
                if stack.iter().any(|name| name == variable) {
                    return Err(format!("variable `{variable}` references itself"));
                }
                stack.push(variable.to_owned());
                let value = parse_value_with_stack(replacement, variables, stack);
                stack.pop();
                value
            }
            None => Ok(ValuePattern::Capture(variable.to_owned())),
        };
    }

    if value == "*" {
        return Ok(ValuePattern::Any);
    }

    for (prefix, comparison) in [
        (">=", Comparison::GreaterOrEqual),
        ("<=", Comparison::LessOrEqual),
        (">", Comparison::Greater),
        ("<", Comparison::Less),
    ] {
        if let Some(number) = value.strip_prefix(prefix) {
            let number = resolve_required_variable(number, variables, "numeric comparison")?;
            return Ok(ValuePattern::Numeric(comparison, parse_number(number)?));
        }
    }

    for (prefix, constructor) in [
        ("exact:", ValuePattern::Exact as fn(String) -> ValuePattern),
        ("contains:", ValuePattern::Contains),
        ("starts_with:", ValuePattern::StartsWith),
        ("ends_with:", ValuePattern::EndsWith),
    ] {
        if let Some(value) = value.strip_prefix(prefix) {
            let value = resolve_required_variable(value, variables, prefix.trim_end_matches(':'))?;
            return Ok(constructor(value.to_owned()));
        }
    }

    if let Some(expression) = value.strip_prefix("regex:") {
        let expression = resolve_required_variable(expression, variables, "regex")?;
        return Regex::new(expression)
            .map(ValuePattern::Regex)
            .map_err(|error| format!("invalid regex `{expression}`: {error}"));
    }

    Ok(ValuePattern::Exact(value.to_owned()))
}

fn parse_number(number: &str) -> Result<f64, String> {
    number
        .replace('_', "")
        .parse()
        .map_err(|_| format!("`{number}` is not a valid number"))
}

fn resolve_required_variable<'a>(
    value: &'a str,
    variables: &'a QueryVariables,
    context: &str,
) -> Result<&'a str, String> {
    let Some(variable) = parse_variable_reference(value)? else {
        return Ok(value);
    };
    variables
        .get(variable)
        .map(String::as_str)
        .ok_or_else(|| format!("undefined variable `${variable}` cannot be used in {context}"))
}

fn parse_variable_reference(value: &str) -> Result<Option<&str>, String> {
    let Some(rest) = value.strip_prefix('$') else {
        return Ok(None);
    };
    let variable = if let Some(rest) = rest.strip_prefix('{') {
        rest.strip_suffix('}')
            .ok_or_else(|| format!("invalid variable reference `{value}`"))?
    } else {
        rest
    };
    if is_variable_name(variable) {
        Ok(Some(variable))
    } else {
        Err(format!("invalid variable reference `{value}`"))
    }
}

pub fn is_variable_name(value: &str) -> bool {
    let mut characters = value.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::{parse_query, parse_query_with_variables};
    use crate::query::QueryVariables;

    #[test]
    fn parses_nested_regex_query() {
        parse_query("class:* -> function:regex:^[A-Z]").unwrap();
    }

    #[test]
    fn parses_boolean_relationships() {
        parse_query("function:* AND descendant(call:open) AND NOT descendant(call:close)").unwrap();
    }

    #[test]
    fn rejects_bad_regex_and_argument_keys() {
        assert!(parse_query("function:regex:[").is_err());
        assert!(parse_query("call:f AND argument:1bad:4").is_err());
    }

    #[test]
    fn parses_defined_and_capture_variables() {
        let variables = QueryVariables::from([("target".to_owned(), "regex:^safe_".to_owned())]);

        parse_query_with_variables("call:$target", &variables).unwrap();
        parse_query("call:* AND argument:0:$x AND argument:1:${x}").unwrap();
        assert!(parse_query("call:$1bad").is_err());
    }

    #[test]
    fn rejects_undefined_variables_inside_predicates() {
        assert_eq!(
            parse_query("call:contains:$target").unwrap_err(),
            "undefined variable `$target` cannot be used in contains"
        );
    }
}
