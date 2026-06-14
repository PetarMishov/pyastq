use std::path::Path;

use serde::Deserialize;

use crate::query::{Query, parse_query};
use crate::report::Finding;
use crate::search::{SearchContext, SearchOptions, search_path, search_source};

#[derive(Debug, Deserialize)]
pub struct RuleFile {
    #[serde(default)]
    pub exclude: Vec<String>,
    pub rules: Vec<Rule>,
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
    let rules: RuleFile = toml::from_str(&source)
        .map_err(|error| format!("invalid rule file {}: {error}", path.display()))?;
    validate_rules(&rules)?;
    Ok(rules)
}

pub fn check(
    root: &Path,
    rule_file: &RuleFile,
    base_options: &SearchOptions,
) -> Result<Vec<Finding>, String> {
    let compiled = compile_rules(rule_file)?;
    let mut findings = Vec::new();

    for compiled_rule in compiled {
        let options = SearchOptions {
            includes: base_options.includes.clone(),
            required_includes: if compiled_rule.rule.include.is_empty() {
                Vec::new()
            } else {
                vec![compiled_rule.rule.include.clone()]
            },
            excludes: base_options
                .excludes
                .iter()
                .chain(&rule_file.exclude)
                .chain(&compiled_rule.rule.exclude)
                .cloned()
                .collect(),
            changed_only: base_options.changed_only,
            max_matches: base_options
                .max_matches
                .map(|maximum| maximum.saturating_sub(findings.len())),
        };

        findings.extend(search_path(
            root,
            &compiled_rule.query,
            &options,
            SearchContext {
                rule_id: Some(&compiled_rule.rule.id),
                message: Some(&compiled_rule.rule.message),
                severity: Some(&compiled_rule.rule.severity),
            },
        )?);

        if base_options
            .max_matches
            .is_some_and(|maximum| findings.len() >= maximum)
        {
            findings.truncate(base_options.max_matches.unwrap_or(findings.len()));
            break;
        }
    }
    Ok(findings)
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
    use super::{RuleFile, test_rules};

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
}
