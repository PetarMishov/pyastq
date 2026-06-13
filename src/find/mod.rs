use std::fs;
use std::path::{Path, PathBuf};

use tree_sitter::{Node, Parser};

use crate::query_parser::{FindPattern, value_match};

pub fn find_op(directory: &str, pattern: &str) -> Result<(), String> {
    let pattern = FindPattern::parse(pattern)?;
    let mut files = Vec::new();
    collect_python_files(Path::new(directory), &mut files)?;
    files.sort();

    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .map_err(|error| format!("failed to load Python grammar: {error}"))?;

    for file in files {
        search_file(&mut parser, &file, &pattern)?;
    }

    Ok(())
}

fn collect_python_files(path: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    if path.is_file() {
        if path.extension().is_some_and(|extension| extension == "py") {
            files.push(path.to_owned());
        }
        return Ok(());
    }

    let entries = fs::read_dir(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;

    for entry in entries {
        let entry = entry
            .map_err(|error| format!("could not read an entry in {}: {error}", path.display()))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("could not inspect {}: {error}", entry.path().display()))?;

        if file_type.is_dir() {
            collect_python_files(&entry.path(), files)?;
        } else if file_type.is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "py")
        {
            files.push(entry.path());
        }
    }

    Ok(())
}

fn search_file(parser: &mut Parser, path: &Path, pattern: &FindPattern) -> Result<(), String> {
    let source = fs::read_to_string(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let tree = parser
        .parse(&source, None)
        .ok_or_else(|| format!("could not parse {}", path.display()))?;

    search_node(tree.root_node(), source.as_bytes(), path, pattern)
}

fn search_node(
    node: Node,
    source: &[u8],
    path: &Path,
    pattern: &FindPattern,
) -> Result<(), String> {
    if matches_pattern(node, source, pattern) {
        let position = node.start_position();
        let text = node
            .utf8_text(source)
            .map_err(|error| format!("invalid UTF-8 in {}: {error}", path.display()))?;

        println!(
            "{}:{}:{}: {}",
            path.display(),
            position.row + 1,
            position.column + 1,
            text.lines().next().unwrap_or("")
        );
    }

    {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            search_node(child, source, path, pattern)?;
        }
    }

    Ok(())
}

fn matches_pattern(node: Node, source: &[u8], pattern: &FindPattern) -> bool {
    if node.kind() != pattern.node_kind {
        return false;
    }

    pattern.fields.iter().all(|field| {
        let Some(field_node) = node.child_by_field_name(&field.field_name) else {
            return false;
        };

        if field
            .node_kind
            .as_ref()
            .is_some_and(|kind| field_node.kind() != kind)
        {
            return false;
        }

        field_node
            .utf8_text(source)
            .is_ok_and(|actual| value_match(&field.value, actual))
    })
}

#[cfg(test)]
mod tests {
    use super::matches_pattern;
    use crate::query_parser::{FieldPattern, FindPattern, ValuePattern};
    use tree_sitter::{Node, Parser};

    fn class_pattern(value: ValuePattern) -> FindPattern {
        FindPattern {
            node_kind: "class_definition".to_owned(),
            fields: vec![FieldPattern {
                field_name: "name".to_owned(),
                node_kind: Some("identifier".to_owned()),
                value,
            }],
        }
    }

    fn matching_text(source: &str, pattern: FindPattern) -> Vec<String> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        fn collect_matches(
            node: Node,
            source: &str,
            pattern: &FindPattern,
            matches: &mut Vec<String>,
        ) {
            if matches_pattern(node, source.as_bytes(), pattern) {
                matches.push(node.utf8_text(source.as_bytes()).unwrap().to_owned());
            }

            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_matches(child, source, pattern, matches);
            }
        }

        let mut matches = Vec::new();
        collect_matches(tree.root_node(), source, &pattern, &mut matches);
        matches
    }

    #[test]
    fn finds_exact_direct_calls() {
        let source = "eval('x')\nevaluate('x')\nmodule.eval('x')\n";
        assert_eq!(
            matching_text(source, FindPattern::parse("call:eval").unwrap()),
            vec!["eval('x')"]
        );
    }

    #[test]
    fn finds_exact_class_names() {
        let source = "class User:\n    pass\n\nclass SuperUser:\n    pass\n";
        assert_eq!(
            matching_text(source, FindPattern::parse("class:User").unwrap()),
            vec!["class User:\n    pass"]
        );
    }

    #[test]
    fn finds_class_names_that_start_with_value() {
        let source =
            "class User:\n    pass\n\nclass SuperUser:\n    pass\n\nclass Supervisor:\n    pass\n";
        let pattern = class_pattern(ValuePattern::StartsWith("Super".to_owned()));

        assert_eq!(
            matching_text(source, pattern),
            vec!["class SuperUser:\n    pass", "class Supervisor:\n    pass"]
        );
    }

    #[test]
    fn finds_class_names_that_end_with_value() {
        let source =
            "class User:\n    pass\n\nclass SuperUser:\n    pass\n\nclass UserProfile:\n    pass\n";
        let pattern = class_pattern(ValuePattern::EndsWith("User".to_owned()));

        assert_eq!(
            matching_text(source, pattern),
            vec!["class User:\n    pass", "class SuperUser:\n    pass"]
        );
    }
}
