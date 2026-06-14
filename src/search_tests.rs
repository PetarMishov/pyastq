use std::path::Path;

use crate::query::parse_query;
use crate::search::{SearchContext, search_source};

fn context(rule_id: Option<&str>) -> SearchContext<'_> {
    SearchContext {
        rule_id,
        message: None,
        severity: None,
    }
}

#[test]
fn searches_nested_patterns() {
    let source = "class User:\n    def Valid(self):\n        pass\n";
    let query = parse_query("class:* -> function:regex:^[A-Z]").unwrap();
    let findings = search_source(Path::new("example.py"), source, &query, context(None)).unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].text, "class User:");
}

#[test]
fn descendant_chains_keep_each_match_inside_the_previous_match() {
    let source = "\
class User:
    log()

    def save(self):
        pass

class Admin:
    def save(self):
        log()
";
    let query = parse_query("class:* -> function:* -> call:log").unwrap();
    let findings = search_source(Path::new("example.py"), source, &query, context(None)).unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].text, "class Admin:");
}

#[test]
fn honors_inline_suppressions() {
    let source = "eval(value)  # past: ignore no-eval\n";
    let query = parse_query("call:eval").unwrap();
    let findings = search_source(
        Path::new("example.py"),
        source,
        &query,
        context(Some("no-eval")),
    )
    .unwrap();
    assert!(findings.is_empty());
}

#[test]
fn matches_keyword_and_positional_arguments() {
    let source = "func(1, size=4)\nfunc(4, size=1)\nfunc(\"input.txt\")\n";
    let query = parse_query("call:func AND argument:0:1 AND argument:size:>3").unwrap();
    let findings = search_source(Path::new("example.py"), source, &query, context(None)).unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].text, "func(1, size=4)");

    let query = parse_query("call:func AND argument:0:\"input.txt\"").unwrap();
    let findings = search_source(Path::new("example.py"), source, &query, context(None)).unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].text, "func(\"input.txt\")");
}

#[test]
fn supports_negation_and_ancestor_relationships() {
    let source = "\
def safe():
    open_file()

def unsafe():
    open_file()
    close_file()
";
    let query = parse_query(
        "function:* AND descendant(call:open_file) AND NOT descendant(call:close_file)",
    )
    .unwrap();
    let findings = search_source(Path::new("example.py"), source, &query, context(None)).unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].text, "def safe():");

    let query = parse_query("call:open_file AND ancestor(function:safe)").unwrap();
    let findings = search_source(Path::new("example.py"), source, &query, context(None)).unwrap();
    assert_eq!(findings.len(), 1);
}

#[test]
fn honors_file_and_previous_line_suppressions() {
    let query = parse_query("call:eval").unwrap();
    let file_suppressed = "# past: ignore-file no-eval\neval(first)\neval(second)\n";
    assert!(
        search_source(
            Path::new("example.py"),
            file_suppressed,
            &query,
            context(Some("no-eval"))
        )
        .unwrap()
        .is_empty()
    );

    let line_suppressed = "# past: ignore no-eval\neval(value)\n";
    assert!(
        search_source(
            Path::new("example.py"),
            line_suppressed,
            &query,
            context(Some("no-eval"))
        )
        .unwrap()
        .is_empty()
    );
}

#[test]
fn matches_imports_and_value_predicates() {
    let source = "import requests\n\nclass UserService:\n    pass\n";
    let query = parse_query("import:requests").unwrap();
    assert_eq!(
        search_source(Path::new("example.py"), source, &query, context(None))
            .unwrap()
            .len(),
        1
    );

    let query = parse_query("class:ends_with:Service").unwrap();
    assert_eq!(
        search_source(Path::new("example.py"), source, &query, context(None))
            .unwrap()
            .len(),
        1
    );
}
