use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use super::{Cli, Command, EXIT_OK, OutputArgs, SearchArgs, execute};
use crate::report::OutputFormat;

static TEMPORARY_DIRECTORY_ID: AtomicU64 = AtomicU64::new(0);

#[test]
fn check_reports_invalid_python() {
    let directory = temporary_directory();
    let source = directory.join("invalid.py");
    let rules = write_rules(&directory);
    std::fs::write(&source, "def broken(:\n    pass\n").unwrap();

    let error = execute(check_command(source.clone(), rules)).unwrap_err();
    assert_eq!(
        error,
        format!(
            "{}:1:12: invalid Python syntax: def broken(:",
            source.display()
        )
    );

    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn check_accepts_a_bare_name_as_valid_python() {
    let directory = temporary_directory();
    let source = directory.join("valid.py");
    let rules = write_rules(&directory);
    std::fs::write(&source, "a\n").unwrap();

    assert_eq!(execute(check_command(source, rules)).unwrap(), EXIT_OK);

    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn num_workers_must_be_positive() {
    assert_eq!(
        super::parse_num_workers("0").unwrap_err(),
        "num-workers must be a positive integer"
    );
    assert_eq!(super::parse_num_workers("4").unwrap(), 4);
}

fn check_command(path: PathBuf, rules: PathBuf) -> Cli {
    Cli {
        command: Command::Check {
            path,
            rules: Some(rules),
            search: SearchArgs::default(),
            output: OutputArgs {
                format: OutputFormat::Text,
                quiet: true,
            },
        },
    }
}

fn write_rules(directory: &std::path::Path) -> PathBuf {
    let rules = directory.join("pyastq.toml");
    std::fs::write(
        &rules,
        r#"
[[rules]]
id = "no-eval"
query = "call:eval"
message = "Avoid eval."
"#,
    )
    .unwrap();
    rules
}

fn temporary_directory() -> PathBuf {
    let id = TEMPORARY_DIRECTORY_ID.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!("pyastq-cli-{}-{id}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    directory
}
