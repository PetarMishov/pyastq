use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::query::{QueryVariables, is_variable_name, parse_query_with_variables};
use crate::report::{OutputFormat, print_findings};
use crate::rules::{check, discover_rules, load_rules, test_rules};
use crate::search::{SearchContext, SearchOptions, search_path};

const EXIT_OK: i32 = 0;
const EXIT_FINDINGS: i32 = 1;
const EXIT_ERROR: i32 = 2;

#[derive(Parser)]
#[command(
    name = "pyastq",
    version,
    about = "Structural search and rules for Python"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Search Python files with one structural query.
    Find {
        path: PathBuf,
        query: String,
        /// Query template variable, as KEY=VALUE. May be repeated.
        #[arg(long = "var", value_name = "KEY=VALUE", value_parser = parse_variable_assignment)]
        variables: Vec<(String, String)>,
        #[arg(long)]
        fail_on_match: bool,
        #[command(flatten)]
        search: SearchArgs,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Run configured rules, discovering [tool.pyastq] when --rules is omitted.
    Check {
        path: PathBuf,
        #[arg(long, short = 'r')]
        rules: Option<PathBuf>,
        #[command(flatten)]
        search: SearchArgs,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Test configured valid and invalid rule examples.
    TestRules {
        #[arg(long, short = 'r')]
        rules: Option<PathBuf>,
    },
}

#[derive(Args, Default)]
struct SearchArgs {
    /// Include matching relative paths. May be repeated.
    #[arg(long)]
    include: Vec<String>,
    /// Exclude matching relative paths. May be repeated.
    #[arg(long)]
    exclude: Vec<String>,
    /// Search only Git files changed relative to HEAD.
    #[arg(long)]
    changed: bool,
    /// Stop after this many matches.
    #[arg(long)]
    max_matches: Option<usize>,
    /// Disable the on-disk file hash cache.
    #[arg(long)]
    no_cache: bool,
    /// Number of files to process concurrently.
    #[arg(long, default_value_t = 1, value_parser = parse_num_workers)]
    num_workers: usize,
}

#[derive(Args)]
struct OutputArgs {
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,
    /// Do not print individual findings.
    #[arg(long, short)]
    quiet: bool,
}

pub fn run() -> i32 {
    match execute(Cli::parse()) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error}");
            EXIT_ERROR
        }
    }
}

fn execute(cli: Cli) -> Result<i32, String> {
    match cli.command {
        Command::Find {
            path,
            query,
            variables,
            fail_on_match,
            search,
            output,
        } => {
            let variables = collect_variables(variables)?;
            let cache_key = format!("find|{query}|vars={variables:?}");
            let query = parse_query_with_variables(&query, &variables)?;
            let mut search: SearchOptions = search.into();
            search.cache_key = Some(cache_key);
            let findings = search_path(
                &path,
                &query,
                &search,
                SearchContext {
                    rule_id: None,
                    message: None,
                    severity: None,
                },
            )?;
            if !output.quiet {
                print_findings(&findings, output.format, &path)?;
            }
            Ok(if fail_on_match && !findings.is_empty() {
                EXIT_FINDINGS
            } else {
                EXIT_OK
            })
        }
        Command::Check {
            path,
            rules,
            search,
            output,
        } => {
            let rule_file = match rules {
                Some(rules) => load_rules(&rules)?,
                None => discover_rules(&path)?.1,
            };
            let findings = check(&path, &rule_file, &search.into())?;
            if !output.quiet {
                print_findings(&findings, output.format, &path)?;
            }
            Ok(if findings.is_empty() {
                EXIT_OK
            } else {
                EXIT_FINDINGS
            })
        }
        Command::TestRules { rules } => {
            let rule_file = match rules {
                Some(rules) => load_rules(&rules)?,
                None => {
                    discover_rules(&std::env::current_dir().map_err(|error| {
                        format!("could not determine current directory: {error}")
                    })?)?
                    .1
                }
            };
            let failures = test_rules(&rule_file)?;
            for failure in &failures {
                eprintln!("{failure}");
            }
            if failures.is_empty() {
                println!("all rule examples passed");
                Ok(EXIT_OK)
            } else {
                Ok(EXIT_FINDINGS)
            }
        }
    }
}

impl From<SearchArgs> for SearchOptions {
    fn from(value: SearchArgs) -> Self {
        Self {
            includes: value.include,
            required_includes: Vec::new(),
            excludes: value.exclude,
            changed_only: value.changed,
            max_matches: value.max_matches,
            use_cache: !value.no_cache,
            cache_key: None,
            num_workers: value.num_workers,
        }
    }
}

fn parse_num_workers(value: &str) -> Result<usize, String> {
    match value.parse::<usize>() {
        Ok(workers) if workers > 0 => Ok(workers),
        _ => Err("num-workers must be a positive integer".to_owned()),
    }
}

fn parse_variable_assignment(value: &str) -> Result<(String, String), String> {
    let (name, value) = value
        .split_once('=')
        .ok_or_else(|| "variables must use KEY=VALUE".to_owned())?;
    if !is_variable_name(name) {
        return Err(format!("invalid variable name `{name}`"));
    }
    Ok((name.to_owned(), value.to_owned()))
}

fn collect_variables(assignments: Vec<(String, String)>) -> Result<QueryVariables, String> {
    let mut variables = QueryVariables::new();
    for (name, value) in assignments {
        if variables.insert(name.clone(), value).is_some() {
            return Err(format!("duplicate variable `{name}`"));
        }
    }
    Ok(variables)
}

#[cfg(test)]
mod tests;
