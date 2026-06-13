#[derive(Debug, PartialEq, Eq)]
pub struct FindPattern {
    pub node_kind: String,
    pub fields: Vec<FieldPattern>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct FieldPattern {
    pub field_name: String,
    pub node_kind: Option<String>,
    pub value: ValuePattern,
}

#[allow(dead_code)]
#[derive(Debug, PartialEq, Eq)]
pub enum ValuePattern {
    Exact(String),
    StartsWith(String),
    EndsWith(String),
}

pub fn value_match(pattern: &ValuePattern, actual: &str) -> bool {
    match pattern {
        ValuePattern::Exact(expected) => actual == expected,
        ValuePattern::StartsWith(expected) => actual.starts_with(expected),
        ValuePattern::EndsWith(expected) => actual.ends_with(expected),
    }
}

impl FindPattern {
    pub fn parse(pattern: &str) -> Result<Self, String> {
        let (kind, name) = pattern
            .split_once(':')
            .ok_or_else(|| "expected a pattern such as `call:eval` or `class:User`".to_owned())?;

        if name.is_empty() {
            return Err("the pattern name cannot be empty".to_owned());
        }

        let (node_kind, field_name) = match kind {
            "call" => ("call", "function"),
            "class" => ("class_definition", "name"),
            _ => {
                return Err(format!(
                    "unsupported pattern kind `{kind}`; expected `call` or `class`"
                ));
            }
        };

        Ok(Self {
            node_kind: node_kind.to_owned(),
            fields: vec![FieldPattern {
                field_name: field_name.to_owned(),
                node_kind: Some("identifier".to_owned()),
                value: ValuePattern::Exact(name.to_owned()),
            }],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{FieldPattern, FindPattern, ValuePattern, value_match};

    #[test]
    fn parses_call_pattern() {
        assert_eq!(
            FindPattern::parse("call:eval"),
            Ok(FindPattern {
                node_kind: "call".to_owned(),
                fields: vec![FieldPattern {
                    field_name: "function".to_owned(),
                    node_kind: Some("identifier".to_owned()),
                    value: ValuePattern::Exact("eval".to_owned()),
                }],
            })
        );
    }

    #[test]
    fn parses_class_pattern() {
        let pattern = FindPattern::parse("class:User").unwrap();

        assert_eq!(pattern.node_kind, "class_definition");
        assert_eq!(pattern.fields[0].field_name, "name");
        assert_eq!(
            pattern.fields[0].value,
            ValuePattern::Exact("User".to_owned())
        );
    }

    #[test]
    fn rejects_invalid_patterns() {
        assert!(FindPattern::parse("eval").is_err());
        assert!(FindPattern::parse("import:os").is_err());
        assert!(FindPattern::parse("call:").is_err());
    }

    #[test]
    fn matches_values() {
        assert!(value_match(&ValuePattern::Exact("User".to_owned()), "User"));
        assert!(value_match(
            &ValuePattern::StartsWith("Super".to_owned()),
            "SuperUser"
        ));
        assert!(value_match(
            &ValuePattern::EndsWith("User".to_owned()),
            "SuperUser"
        ));
        assert!(!value_match(
            &ValuePattern::Exact("User".to_owned()),
            "SuperUser"
        ));
    }
}
