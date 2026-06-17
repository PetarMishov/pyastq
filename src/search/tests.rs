use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::{SearchContext, SearchOptions, search_path, search_source};
use crate::query::{QueryVariables, parse_query, parse_query_with_variables};

static TEMPORARY_DIRECTORY_ID: AtomicU64 = AtomicU64::new(0);

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
    let source = "eval(value)  # pyastq: ignore no-eval\n";
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
fn captures_undefined_variables_within_each_match() {
    let source = "same(value, value)\ndifferent(first, second)\nstrings(\"x\", \"x\")\n";
    let query = parse_query("call:* AND argument:0:$x AND argument:1:$x").unwrap();
    let findings = search_source(Path::new("example.py"), source, &query, context(None)).unwrap();

    assert_eq!(
        findings
            .iter()
            .map(|finding| finding.text.as_str())
            .collect::<Vec<_>>(),
        ["same(value, value)", "strings(\"x\", \"x\")"]
    );
}

#[test]
fn applies_defined_variables_to_query_templates() {
    let source = "eval(value)\nparse(value)\n";
    let variables = QueryVariables::from([("target".to_owned(), "eval".to_owned())]);
    let query = parse_query_with_variables("call:$target", &variables).unwrap();
    let findings = search_source(Path::new("example.py"), source, &query, context(None)).unwrap();

    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].text, "eval(value)");
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
    let file_suppressed = "# pyastq: ignore-file no-eval\neval(first)\neval(second)\n";
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

    let line_suppressed = "# pyastq: ignore no-eval\neval(value)\n";
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

#[test]
fn resolves_import_aliases_and_from_imports_for_calls() {
    let source = "\
import requests as r
from requests import get
from requests import post as send

r.get(first)
get(second)
send(third)
";
    let get_query = parse_query("call:requests.get").unwrap();
    let findings =
        search_source(Path::new("example.py"), source, &get_query, context(None)).unwrap();
    assert_eq!(
        findings
            .iter()
            .map(|finding| finding.text.as_str())
            .collect::<Vec<_>>(),
        ["r.get(first)", "get(second)"]
    );

    let post_query = parse_query("call:requests.post").unwrap();
    let findings =
        search_source(Path::new("example.py"), source, &post_query, context(None)).unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].text, "send(third)");
}

#[test]
fn imported_names_are_not_resolved_when_shadowed() {
    let source = "\
import requests
import requests as r

requests.get(before)
r.get(before_alias)

def fetch(requests, r):
    requests.get(parameter)
    r.get(alias_parameter)

requests = client
r = client
requests.get(after)
r.get(after_alias)
";
    let query = parse_query("call:requests.get").unwrap();
    let findings = search_source(Path::new("example.py"), source, &query, context(None)).unwrap();
    assert_eq!(
        findings
            .iter()
            .map(|finding| finding.text.as_str())
            .collect::<Vec<_>>(),
        ["requests.get(before)", "r.get(before_alias)"]
    );
}

#[test]
fn function_local_bindings_shadow_imports_for_the_whole_scope() {
    let source = "\
import requests

def fetch():
    requests.get(before_assignment)
    requests = client
";
    let query = parse_query("call:requests.get").unwrap();
    let findings = search_source(Path::new("example.py"), source, &query, context(None)).unwrap();
    assert!(findings.is_empty());
}

#[test]
fn resolves_function_local_imports_and_skips_class_namespaces_for_methods() {
    let source = "\
import requests as module_requests

def fetch():
    from requests import get as fetch_url
    fetch_url(local)

class Client:
    import other_requests as module_requests

    def fetch(self):
        module_requests.get(module)
";
    let requests_query = parse_query("call:requests.get").unwrap();
    let findings = search_source(
        Path::new("example.py"),
        source,
        &requests_query,
        context(None),
    )
    .unwrap();
    assert_eq!(
        findings
            .iter()
            .map(|finding| finding.text.as_str())
            .collect::<Vec<_>>(),
        ["fetch_url(local)", "module_requests.get(module)"]
    );

    let class_import_query = parse_query("call:other_requests.get").unwrap();
    let findings = search_source(
        Path::new("example.py"),
        source,
        &class_import_query,
        context(None),
    )
    .unwrap();
    assert!(findings.is_empty());
}

#[test]
fn comprehension_targets_do_not_leak_and_global_declarations_use_module_imports() {
    let source = "\
import requests

[requests.get(item) for requests in clients]
requests.get(after_comprehension)

def fetch():
    global requests
    requests.get(global_name)
";
    let query = parse_query("call:requests.get").unwrap();
    let findings = search_source(Path::new("example.py"), source, &query, context(None)).unwrap();
    assert_eq!(
        findings
            .iter()
            .map(|finding| finding.text.as_str())
            .collect::<Vec<_>>(),
        [
            "requests.get(after_comprehension)",
            "requests.get(global_name)"
        ]
    );
}

#[test]
fn literal_call_queries_still_work_without_known_imports() {
    let source = "\
requests.get(url)

def fetch(requests):
    requests.get(other)
";
    let query = parse_query("call:requests.get").unwrap();
    let findings = search_source(Path::new("example.py"), source, &query, context(None)).unwrap();
    assert_eq!(findings.len(), 2);
}

#[test]
fn rejects_invalid_python_with_its_location_and_source_line() {
    let query = parse_query("call:eval").unwrap();
    let error = search_source(
        Path::new("invalid.py"),
        "def broken(:\n    pass\n",
        &query,
        context(None),
    )
    .unwrap_err();

    assert_eq!(
        error,
        "invalid.py:1:12: invalid Python syntax: def broken(:"
    );
}

#[test]
fn invalid_python_cannot_be_hidden_by_a_file_suppression() {
    let query = parse_query("call:eval").unwrap();
    let error = search_source(
        Path::new("invalid.py"),
        "# pyastq: ignore-file no-eval\ndef broken(:\n",
        &query,
        context(Some("no-eval")),
    )
    .unwrap_err();

    assert!(error.starts_with("invalid.py:2:"));
}

#[test]
fn caches_unchanged_files_and_invalidates_changed_or_deleted_files() {
    let directory = temporary_directory();
    let first = directory.join("first.py");
    let second = directory.join("second.py");
    std::fs::write(&first, "eval(value)\n").unwrap();
    std::fs::write(&second, "parse(value)\n").unwrap();

    let query = parse_query("call:eval").unwrap();
    let options = SearchOptions {
        cache_key: Some("test|call:eval".to_owned()),
        ..SearchOptions::default()
    };

    let initial = search_path(&directory, &query, &options, context(None)).unwrap();
    assert_eq!(initial.len(), 1);
    assert!(directory.join(".pyastq-cache.json").is_file());

    let unchanged = search_path(&directory, &query, &options, context(None)).unwrap();
    assert_eq!(unchanged.len(), 1);
    assert_eq!(unchanged[0].path, first.display().to_string());

    std::fs::write(&first, "parse(value)\n").unwrap();
    std::fs::write(&second, "eval(value)\n").unwrap();
    let changed = search_path(&directory, &query, &options, context(None)).unwrap();
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0].path, second.display().to_string());

    std::fs::remove_file(&second).unwrap();
    let deleted = search_path(&directory, &query, &options, context(None)).unwrap();
    assert!(deleted.is_empty());

    let cache = std::fs::read_to_string(directory.join(".pyastq-cache.json")).unwrap();
    assert!(!cache.contains("second.py"));
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn parallel_search_preserves_sorted_results_and_cache_reuse() {
    let directory = temporary_directory();
    let first = directory.join("a.py");
    let second = directory.join("b.py");
    std::fs::write(&first, "eval(first)\neval(second)\n").unwrap();
    std::fs::write(&second, "eval(third)\n").unwrap();

    let query = parse_query("call:eval").unwrap();
    let serial_options = SearchOptions {
        cache_key: Some("test|parallel".to_owned()),
        ..SearchOptions::default()
    };
    let serial = search_path(&directory, &query, &serial_options, context(None)).unwrap();
    let parallel_options = SearchOptions {
        num_workers: 4,
        ..serial_options
    };

    let parallel = search_path(&directory, &query, &parallel_options, context(None)).unwrap();
    let serial_locations: Vec<_> = serial
        .iter()
        .map(|finding| (&finding.path, finding.line, finding.column))
        .collect();
    let parallel_locations: Vec<_> = parallel
        .iter()
        .map(|finding| (&finding.path, finding.line, finding.column))
        .collect();

    assert_eq!(parallel_locations, serial_locations);
    assert_eq!(parallel.len(), 3);
    std::fs::remove_dir_all(directory).unwrap();
}

fn temporary_directory() -> PathBuf {
    let id = TEMPORARY_DIRECTORY_ID.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!("pyastq-cache-{}-{id}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    directory
}
