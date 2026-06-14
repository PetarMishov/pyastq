use std::collections::BTreeSet;
use std::path::Path;

use tree_sitter::{Node, Parser};

use crate::cache::{SearchCache, content_hash};
use crate::files::{FileFilter, collect_python_files};
use crate::query::{Query, matches_query};
use crate::report::Finding;

pub struct SearchOptions {
    pub includes: Vec<String>,
    pub required_includes: Vec<Vec<String>>,
    pub excludes: Vec<String>,
    pub changed_only: bool,
    pub max_matches: Option<usize>,
    pub use_cache: bool,
    pub cache_key: Option<String>,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            includes: Vec::new(),
            required_includes: Vec::new(),
            excludes: Vec::new(),
            changed_only: false,
            max_matches: None,
            use_cache: true,
            cache_key: None,
        }
    }
}

pub struct SearchContext<'a> {
    pub rule_id: Option<&'a str>,
    pub message: Option<&'a str>,
    pub severity: Option<&'a str>,
}

pub fn search_path(
    root: &Path,
    query: &Query,
    options: &SearchOptions,
    context: SearchContext<'_>,
) -> Result<Vec<Finding>, String> {
    let mut include_groups = vec![options.includes.clone()];
    include_groups.extend(options.required_includes.clone());
    let filter = FileFilter::new(
        root,
        &include_groups,
        &options.excludes,
        options.changed_only,
    )?;
    let files = collect_python_files(root, &filter)?;
    let mut parser = python_parser()?;
    let mut findings = Vec::new();
    let cache_enabled = options.use_cache && !options.changed_only && options.max_matches.is_none();
    let result_key = options.cache_key.as_ref().map(|key| {
        format!(
            "{key}|root={}|include={:?}|required={:?}|exclude={:?}",
            root.display(),
            options.includes,
            options.required_includes,
            options.excludes
        )
    });
    let mut cache = cache_enabled.then(|| SearchCache::load(root));
    let mut current_files = BTreeSet::new();

    for path in files {
        let source = std::fs::read_to_string(&path)
            .map_err(|error| format!("could not read {}: {error}", path.display()))?;
        let hash = content_hash(source.as_bytes());
        let file_key = cache
            .as_ref()
            .map(|cache| cache.file_key(&path))
            .unwrap_or_default();
        let file_findings = cache
            .as_ref()
            .zip(result_key.as_deref())
            .and_then(|(cache, result_key)| cache.findings(&file_key, &hash, result_key))
            .map(Ok)
            .unwrap_or_else(|| {
                search_source_with_parser(&mut parser, &path, &source, query, &context)
            })?;

        if let (Some(cache), Some(result_key)) = (&mut cache, result_key.as_ref()) {
            current_files.insert(file_key.clone());
            cache.store(&file_key, &hash, result_key.clone(), file_findings.clone());
        }
        findings.extend(file_findings);

        if options
            .max_matches
            .is_some_and(|maximum| findings.len() >= maximum)
        {
            findings.truncate(options.max_matches.unwrap_or(findings.len()));
            break;
        }
    }

    if let (Some(cache), Some(result_key)) = (&mut cache, result_key) {
        cache.retain_result_files(&result_key, &current_files);
        let _ = cache.save();
    }

    Ok(findings)
}

pub fn search_source(
    path: &Path,
    source: &str,
    query: &Query,
    context: SearchContext<'_>,
) -> Result<Vec<Finding>, String> {
    search_source_with_parser(&mut python_parser()?, path, source, query, &context)
}

pub fn search_source_queries(
    path: &Path,
    source: &str,
    queries: &[(&Query, SearchContext<'_>)],
) -> Result<Vec<Vec<Finding>>, String> {
    let mut parser = python_parser()?;
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| format!("could not parse {}", path.display()))?;

    queries
        .iter()
        .map(|(query, context)| {
            if context
                .rule_id
                .is_some_and(|rule_id| file_is_suppressed(source, rule_id))
            {
                return Ok(Vec::new());
            }
            let mut findings = Vec::new();
            collect_matches(
                tree.root_node(),
                path,
                source,
                query,
                context,
                &mut findings,
            )?;
            Ok(findings)
        })
        .collect()
}

fn search_source_with_parser(
    parser: &mut Parser,
    path: &Path,
    source: &str,
    query: &Query,
    context: &SearchContext<'_>,
) -> Result<Vec<Finding>, String> {
    if context
        .rule_id
        .is_some_and(|rule_id| file_is_suppressed(source, rule_id))
    {
        return Ok(Vec::new());
    }

    let tree = parser
        .parse(source, None)
        .ok_or_else(|| format!("could not parse {}", path.display()))?;
    let mut findings = Vec::new();
    collect_matches(
        tree.root_node(),
        path,
        source,
        query,
        context,
        &mut findings,
    )?;
    Ok(findings)
}

fn collect_matches(
    node: Node,
    path: &Path,
    source: &str,
    query: &Query,
    context: &SearchContext<'_>,
    findings: &mut Vec<Finding>,
) -> Result<(), String> {
    if matches_query(node, source.as_bytes(), query) {
        let position = node.start_position();
        let line = position.row + 1;
        if !context
            .rule_id
            .is_some_and(|rule_id| line_is_suppressed(source, line, rule_id))
        {
            let text = node
                .utf8_text(source.as_bytes())
                .map_err(|error| format!("invalid UTF-8 in {}: {error}", path.display()))?;
            findings.push(Finding {
                path: path.display().to_string(),
                line,
                column: position.column + 1,
                text: text.lines().next().unwrap_or("").to_owned(),
                rule_id: context.rule_id.map(str::to_owned),
                message: context.message.map(str::to_owned),
                severity: context.severity.map(str::to_owned),
            });
        }
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_matches(child, path, source, query, context, findings)?;
    }
    Ok(())
}

fn python_parser() -> Result<Parser, String> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .map_err(|error| format!("failed to load Python grammar: {error}"))?;
    Ok(parser)
}

fn file_is_suppressed(source: &str, rule_id: &str) -> bool {
    source
        .lines()
        .any(|line| directive_matches(line, "pyastq: ignore-file", rule_id))
}

fn line_is_suppressed(source: &str, line_number: usize, rule_id: &str) -> bool {
    let lines: Vec<_> = source.lines().collect();
    let current = lines.get(line_number.saturating_sub(1)).copied();
    let previous = lines.get(line_number.saturating_sub(2)).copied();

    current
        .into_iter()
        .chain(previous)
        .any(|line| directive_matches(line, "pyastq: ignore", rule_id))
}

fn directive_matches(line: &str, directive: &str, rule_id: &str) -> bool {
    let Some((_, arguments)) = line.split_once(directive) else {
        return false;
    };
    let arguments = arguments.trim();
    arguments.is_empty()
        || arguments
            .split([',', ' ', '\t'])
            .any(|candidate| candidate == rule_id)
}
