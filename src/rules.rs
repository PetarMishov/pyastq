use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::cache::{SearchCache, content_hash};
use crate::files::{FileFilter, collect_python_files};
use crate::query::{Query, parse_query};
use crate::report::{Finding, sort_findings};
use crate::search::{SearchContext, SearchOptions, search_source, search_source_queries};

#[derive(Debug, Deserialize)]
pub struct RuleFile {
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

#[derive(Deserialize)]
struct PyProject {
    tool: Option<PyProjectTools>,
}

#[derive(Deserialize)]
struct PyProjectTools {
    pyastq: Option<PyProjectPyastq>,
}

#[derive(Deserialize)]
struct PyProjectPyastq {
    #[serde(default)]
    exclude: Vec<String>,
    #[serde(default)]
    rules: Vec<Rule>,
    #[serde(rename = "rules-file")]
    rules_file: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
pub struct Rule {
    pub id: String,
    pub query: String,
    pub message: String,
    #[serde(default = "default_severity")]
    pub severity: String,
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub valid: Vec<String>,
    #[serde(default)]
    pub invalid: Vec<String>,
}

pub struct CompiledRule<'a> {
    pub rule: &'a Rule,
    pub query: Query,
}

pub fn load_rules(path: &Path) -> Result<RuleFile, String> {
    let source = std::fs::read_to_string(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let rules = if path
        .file_name()
        .is_some_and(|name| name == "pyproject.toml")
    {
        parse_pyproject_rules(path, &source)?
            .ok_or_else(|| format!("{} does not contain [tool.pyastq]", path.display()))?
    } else {
        toml::from_str(&source)
            .map_err(|error| format!("invalid rule file {}: {error}", path.display()))?
    };
    validate_rules(&rules)?;
    Ok(rules)
}

pub fn discover_rules(start: &Path) -> Result<(PathBuf, RuleFile), String> {
    let start = if start.is_file() {
        start.parent().unwrap_or_else(|| Path::new("."))
    } else {
        start
    };
    let mut directory = start.canonicalize().map_err(|error| {
        format!(
            "could not resolve configuration search path {}: {error}",
            start.display()
        )
    })?;

    loop {
        let candidate = directory.join("pyproject.toml");
        if candidate.is_file() {
            let source = std::fs::read_to_string(&candidate)
                .map_err(|error| format!("could not read {}: {error}", candidate.display()))?;
            if let Some(rules) = parse_pyproject_rules(&candidate, &source)? {
                validate_rules(&rules)?;
                return Ok((candidate, rules));
            }
        }
        if !directory.pop() {
            break;
        }
    }

    Err(format!(
        "no [tool.pyastq] configuration found from {}; pass --rules <path>",
        start.display()
    ))
}

fn parse_pyproject_rules(path: &Path, source: &str) -> Result<Option<RuleFile>, String> {
    let pyproject: PyProject = toml::from_str(source)
        .map_err(|error| format!("invalid pyproject.toml {}: {error}", path.display()))?;
    let Some(configuration) = pyproject.tool.and_then(|tool| tool.pyastq) else {
        return Ok(None);
    };
    let mut rules = RuleFile {
        exclude: configuration.exclude,
        rules: configuration.rules,
    };

    if let Some(rules_file) = configuration.rules_file {
        let rules_path = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(rules_file);
        if rules_path
            .file_name()
            .is_some_and(|name| name == "pyproject.toml")
        {
            return Err(format!(
                "{}: rules-file must reference a standalone rule file",
                path.display()
            ));
        }
        let source = std::fs::read_to_string(&rules_path)
            .map_err(|error| format!("could not read {}: {error}", rules_path.display()))?;
        let external: RuleFile = toml::from_str(&source)
            .map_err(|error| format!("invalid rule file {}: {error}", rules_path.display()))?;
        rules.exclude.extend(external.exclude);
        rules.rules.splice(0..0, external.rules);
    }

    Ok(Some(rules))
}

pub fn check(
    root: &Path,
    rule_file: &RuleFile,
    base_options: &SearchOptions,
) -> Result<Vec<Finding>, String> {
    let compiled = compile_rules(rule_file)?;
    let global_excludes: Vec<_> = base_options
        .excludes
        .iter()
        .chain(&rule_file.exclude)
        .cloned()
        .collect();
    let collection_filter = FileFilter::new(
        root,
        std::slice::from_ref(&base_options.includes),
        &global_excludes,
        base_options.changed_only,
    )?;
    let files = collect_python_files(root, &collection_filter)?;
    let rule_filters = compiled
        .iter()
        .map(|compiled_rule| {
            let include_groups = vec![
                base_options.includes.clone(),
                compiled_rule.rule.include.clone(),
            ];
            let excludes: Vec<_> = global_excludes
                .iter()
                .chain(&compiled_rule.rule.exclude)
                .cloned()
                .collect();
            FileFilter::new(root, &include_groups, &excludes, false)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let result_keys: Vec<_> = compiled
        .iter()
        .map(|compiled_rule| rule_result_key(root, compiled_rule, base_options, &global_excludes))
        .collect();
    let cache_enabled =
        base_options.use_cache && !base_options.changed_only && base_options.max_matches.is_none();
    let mut cache = cache_enabled.then(|| SearchCache::load(root));
    let mut current_files = vec![BTreeSet::new(); compiled.len()];
    let mut findings_by_rule = vec![Vec::new(); compiled.len()];
    let base = if root.is_file() {
        root.parent().unwrap_or_else(|| Path::new("."))
    } else {
        root
    };

    for path in files {
        let relative = path.strip_prefix(base).unwrap_or(&path);
        let source = std::fs::read_to_string(&path)
            .map_err(|error| format!("could not read {}: {error}", path.display()))?;
        let hash = content_hash(source.as_bytes());
        let file_key = cache
            .as_ref()
            .map(|cache| cache.file_key(&path))
            .unwrap_or_default();
        let applicable: Vec<_> = rule_filters
            .iter()
            .enumerate()
            .filter_map(|(index, filter)| filter.accepts(relative).then_some(index))
            .collect();
        let mut missing = Vec::new();

        for &index in &applicable {
            if cache.is_some() {
                current_files[index].insert(file_key.clone());
            }
            match cache
                .as_ref()
                .and_then(|cache| cache.findings(&file_key, &hash, &result_keys[index]))
            {
                Some(findings) => findings_by_rule[index].extend(findings),
                None => missing.push(index),
            }
        }

        if missing.is_empty() {
            continue;
        }

        let queries: Vec<_> = missing
            .iter()
            .map(|&index| {
                let rule = compiled[index].rule;
                (
                    &compiled[index].query,
                    SearchContext {
                        rule_id: Some(&rule.id),
                        message: Some(&rule.message),
                        severity: Some(&rule.severity),
                    },
                )
            })
            .collect();
        let searched = search_source_queries(&path, &source, &queries)?;
        for (index, findings) in missing.into_iter().zip(searched) {
            if let Some(cache) = &mut cache {
                cache.store(
                    &file_key,
                    &hash,
                    result_keys[index].clone(),
                    findings.clone(),
                );
            }
            findings_by_rule[index].extend(findings);
        }
    }

    if let Some(cache) = &mut cache {
        for (result_key, files) in result_keys.iter().zip(&current_files) {
            cache.retain_result_files(result_key, files);
        }
        let _ = cache.save();
    }

    let mut findings: Vec<_> = findings_by_rule.into_iter().flatten().collect();
    sort_findings(&mut findings);
    if let Some(maximum) = base_options.max_matches {
        findings.truncate(maximum);
    }
    Ok(findings)
}

fn rule_result_key(
    root: &Path,
    compiled_rule: &CompiledRule<'_>,
    options: &SearchOptions,
    global_excludes: &[String],
) -> String {
    format!(
        "resolver-v1|check|{}|{}|{}|{}|root={}|include={:?}|rule_include={:?}|exclude={:?}|rule_exclude={:?}",
        compiled_rule.rule.id,
        compiled_rule.rule.query,
        compiled_rule.rule.message,
        compiled_rule.rule.severity,
        root.display(),
        options.includes,
        compiled_rule.rule.include,
        global_excludes,
        compiled_rule.rule.exclude
    )
}

pub fn test_rules(rule_file: &RuleFile) -> Result<Vec<String>, String> {
    let compiled = compile_rules(rule_file)?;
    let mut failures = Vec::new();

    for compiled_rule in compiled {
        for (index, source) in compiled_rule.rule.valid.iter().enumerate() {
            let findings = test_source(&compiled_rule, source)?;
            if !findings.is_empty() {
                failures.push(format!(
                    "{} valid example {} unexpectedly matched",
                    compiled_rule.rule.id,
                    index + 1
                ));
            }
        }
        for (index, source) in compiled_rule.rule.invalid.iter().enumerate() {
            let findings = test_source(&compiled_rule, source)?;
            if findings.is_empty() {
                failures.push(format!(
                    "{} invalid example {} did not match",
                    compiled_rule.rule.id,
                    index + 1
                ));
            }
        }
    }
    Ok(failures)
}

fn test_source(compiled_rule: &CompiledRule<'_>, source: &str) -> Result<Vec<Finding>, String> {
    search_source(
        Path::new("<rule-test>.py"),
        source,
        &compiled_rule.query,
        SearchContext {
            rule_id: Some(&compiled_rule.rule.id),
            message: Some(&compiled_rule.rule.message),
            severity: Some(&compiled_rule.rule.severity),
        },
    )
}

fn compile_rules(rule_file: &RuleFile) -> Result<Vec<CompiledRule<'_>>, String> {
    rule_file
        .rules
        .iter()
        .map(|rule| {
            parse_query(&rule.query)
                .map(|query| CompiledRule { rule, query })
                .map_err(|error| format!("rule `{}`: {error}", rule.id))
        })
        .collect()
}

fn validate_rules(rule_file: &RuleFile) -> Result<(), String> {
    if rule_file.rules.is_empty() {
        return Err("rule configuration must define at least one rule".to_owned());
    }
    let mut ids = std::collections::HashSet::new();
    for rule in &rule_file.rules {
        if rule.id.trim().is_empty() {
            return Err("rule IDs cannot be empty".to_owned());
        }
        if !ids.insert(&rule.id) {
            return Err(format!("duplicate rule ID `{}`", rule.id));
        }
        if !matches!(rule.severity.as_str(), "info" | "warning" | "error") {
            return Err(format!(
                "rule `{}` has invalid severity `{}`; expected info, warning, or error",
                rule.id, rule.severity
            ));
        }
    }
    Ok(())
}

fn default_severity() -> String {
    "warning".to_owned()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{RuleFile, check, discover_rules, load_rules, test_rules};
    use crate::search::SearchOptions;

    static TEMPORARY_DIRECTORY_ID: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn validates_rule_examples() {
        let rules: RuleFile = toml::from_str(
            r#"
                [[rules]]
                id = "no-eval"
                query = "call:eval"
                message = "Avoid eval"
                valid = ["parse(value)"]
                invalid = ["eval(value)"]
            "#,
        )
        .unwrap();

        assert!(test_rules(&rules).unwrap().is_empty());
    }

    #[test]
    fn caches_one_file_hash_with_results_for_each_rule() {
        let directory = temporary_directory("rule-cache");
        std::fs::write(directory.join("example.py"), "eval(value)\nprint(value)\n").unwrap();
        let rules: RuleFile = toml::from_str(
            r#"
                [[rules]]
                id = "no-eval"
                query = "call:eval"
                message = "Avoid eval"

                [[rules]]
                id = "no-print"
                query = "call:print"
                message = "Avoid print"
            "#,
        )
        .unwrap();

        let first = check(&directory, &rules, &SearchOptions::default()).unwrap();
        assert_eq!(first.len(), 2);
        let second = check(&directory, &rules, &SearchOptions::default()).unwrap();
        assert_eq!(second.len(), 2);

        let cache: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(directory.join(".pyastq-cache.json")).unwrap(),
        )
        .unwrap();
        let cached_file = &cache["files"]["example.py"];
        assert!(cached_file["hash"].is_string());
        assert_eq!(cached_file["results"].as_object().unwrap().len(), 2);

        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn returns_findings_in_location_order_instead_of_rule_order() {
        let directory = temporary_directory("finding-order");
        std::fs::write(
            directory.join("example.py"),
            "print('first')\neval('second')\n",
        )
        .unwrap();
        let rules: RuleFile = toml::from_str(
            r#"
                [[rules]]
                id = "no-eval"
                query = "call:eval"
                message = "Avoid eval"

                [[rules]]
                id = "no-print"
                query = "call:print"
                message = "Avoid print"
            "#,
        )
        .unwrap();

        let findings = check(&directory, &rules, &SearchOptions::default()).unwrap();
        assert_eq!(
            findings
                .iter()
                .map(|finding| (finding.line, finding.rule_id.as_deref()))
                .collect::<Vec<_>>(),
            [(1, Some("no-print")), (2, Some("no-eval"))]
        );

        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn loads_and_discovers_tool_pyastq_from_pyproject() {
        let directory = temporary_directory("pyproject");
        let nested = directory.join("src/package");
        std::fs::create_dir_all(&nested).unwrap();
        let pyproject = directory.join("pyproject.toml");
        std::fs::write(
            &pyproject,
            r#"
                [project]
                name = "example"
                version = "0.1.0"

                [tool.pyastq]
                exclude = ["generated/**"]

                [[tool.pyastq.rules]]
                id = "no-eval"
                query = "call:eval"
                message = "Avoid eval"
                severity = "error"
            "#,
        )
        .unwrap();

        let explicit = load_rules(&pyproject).unwrap();
        assert_eq!(explicit.rules.len(), 1);
        assert_eq!(explicit.exclude, ["generated/**"]);

        let (discovered_path, discovered) = discover_rules(&nested).unwrap();
        assert_eq!(discovered_path, pyproject.canonicalize().unwrap());
        assert_eq!(discovered.rules[0].id, "no-eval");

        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn loads_relative_rules_file_and_merges_inline_rules() {
        let directory = temporary_directory("external-rules");
        std::fs::create_dir_all(directory.join("config")).unwrap();
        std::fs::write(
            directory.join("config/strict.toml"),
            r#"
                exclude = ["vendor/**"]

                [[rules]]
                id = "no-eval"
                query = "call:eval"
                message = "Avoid eval"
            "#,
        )
        .unwrap();
        let pyproject = directory.join("pyproject.toml");
        std::fs::write(
            &pyproject,
            r#"
                [tool.pyastq]
                rules-file = "config/strict.toml"
                exclude = ["generated/**"]

                [[tool.pyastq.rules]]
                id = "no-print"
                query = "call:print"
                message = "Avoid print"
            "#,
        )
        .unwrap();

        let rules = load_rules(&pyproject).unwrap();
        assert_eq!(rules.exclude, ["generated/**", "vendor/**"]);
        assert_eq!(rules.rules.len(), 2);
        assert_eq!(rules.rules[0].id, "no-eval");
        assert_eq!(rules.rules[1].id, "no-print");

        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rejects_duplicate_ids_across_external_and_inline_rules() {
        let directory = temporary_directory("duplicate-external-rules");
        std::fs::write(
            directory.join("rules.toml"),
            r#"
                [[rules]]
                id = "no-eval"
                query = "call:eval"
                message = "Avoid eval"
            "#,
        )
        .unwrap();
        let pyproject = directory.join("pyproject.toml");
        std::fs::write(
            &pyproject,
            r#"
                [tool.pyastq]
                rules-file = "rules.toml"

                [[tool.pyastq.rules]]
                id = "no-eval"
                query = "call:eval"
                message = "Avoid eval again"
            "#,
        )
        .unwrap();

        let error = load_rules(&pyproject).unwrap_err();
        assert!(error.contains("duplicate rule ID `no-eval`"));

        std::fs::remove_dir_all(directory).unwrap();
    }

    fn temporary_directory(label: &str) -> std::path::PathBuf {
        let id = TEMPORARY_DIRECTORY_ID.fetch_add(1, Ordering::Relaxed);
        let directory =
            std::env::temp_dir().join(format!("pyastq-{label}-{}-{id}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        directory
    }
}
