#[derive(Clone, Debug, PartialEq)]
pub enum Token {
    Atom(String),
    And,
    Or,
    Not,
    Arrow,
    Relation(RelationToken),
    LeftParen,
    RightParen,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RelationToken {
    Child,
    Descendant,
    Ancestor,
}

pub fn tokenize(query: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut characters = query.chars().peekable();

    while let Some(character) = characters.next() {
        if escaped {
            current.push(character);
            escaped = false;
            continue;
        }

        if character == '\\' {
            current.push(character);
            escaped = true;
            continue;
        }

        if let Some(active_quote) = quote {
            if character == active_quote {
                quote = None;
            } else {
                current.push(character);
            }
            continue;
        }

        match character {
            '"' | '\'' => quote = Some(character),
            '-' if characters.peek() == Some(&'>') => {
                characters.next();
                push_current(&mut current, &mut tokens);
                tokens.push(Token::Arrow);
            }
            '(' => {
                push_current(&mut current, &mut tokens);
                tokens.push(Token::LeftParen);
            }
            ')' => {
                push_current(&mut current, &mut tokens);
                tokens.push(Token::RightParen);
            }
            character if character.is_whitespace() => {
                push_current(&mut current, &mut tokens);
            }
            _ => current.push(character),
        }
    }

    if quote.is_some() {
        return Err("unterminated quoted value".to_owned());
    }
    push_current(&mut current, &mut tokens);

    if tokens.is_empty() {
        Err("the query cannot be empty".to_owned())
    } else {
        Ok(tokens)
    }
}

fn push_current(current: &mut String, tokens: &mut Vec<Token>) {
    if current.is_empty() {
        return;
    }

    let value = std::mem::take(current);
    tokens.push(match value.as_str() {
        "AND" => Token::And,
        "OR" => Token::Or,
        "NOT" => Token::Not,
        "child" => Token::Relation(RelationToken::Child),
        "descendant" => Token::Relation(RelationToken::Descendant),
        "ancestor" | "inside" => Token::Relation(RelationToken::Ancestor),
        _ => Token::Atom(value),
    });
}

#[cfg(test)]
mod tests {
    use super::{Token, tokenize};

    #[test]
    fn tokenizes_relationships_and_quoted_values() {
        assert_eq!(
            tokenize("class:* -> function:regex:\"^[A-Z].*\"").unwrap(),
            vec![
                Token::Atom("class:*".to_owned()),
                Token::Arrow,
                Token::Atom("function:regex:^[A-Z].*".to_owned()),
            ]
        );
    }
}
