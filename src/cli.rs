use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::query::parse_query;
use crate::report::{OutputFormat, print_findings};
use crate::rules::{check, load_rules, test_rules};
use crate::search::{SearchContext, SearchOptions, search_path};

const EXIT_OK: i32 = 0;
const EXIT_FINDINGS: i32 = 1;
const EXIT_ERROR: i32 = 2;

#[derive(Parser)]
#[command(
    name = "past",
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
        #[arg(long)]
        fail_on_match: bool,
        #[command(flatten)]
        search: SearchArgs,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Run all rules from a TOML rule file.
    Check {
        path: PathBuf,
        #[arg(long, short = 'r')]
        rules: PathBuf,
        #[command(flatten)]
        search: SearchArgs,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Run valid and invalid examples embedded in a rule file.
    TestRules {
        #[arg(long, short = 'r')]
        rules: PathBuf,
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
            fail_on_match,
            search,
            output,
        } => {
            let query = parse_query(&query)?;
            let findings = search_path(
                &path,
                &query,
                &search.into(),
                SearchContext {
                    rule_id: None,
                    message: None,
                    severity: None,
                },
            )?;
            if !output.quiet {
                print_findings(&findings, output.format)?;
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
            let rule_file = load_rules(&rules)?;
            let findings = check(&path, &rule_file, &search.into())?;
            if !output.quiet {
                print_findings(&findings, output.format)?;
            }
            Ok(if findings.is_empty() {
                EXIT_OK
            } else {
                EXIT_FINDINGS
            })
        }
        Command::TestRules { rules } => {
            let rule_file = load_rules(&rules)?;
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
        }
    }
}
