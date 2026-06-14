use std::collections::BTreeMap;
use std::path::Path;

use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::cache::content_hash;

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum OutputFormat {
    Text,
    Json,
    Jsonl,
    Sarif,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Finding {
    pub path: String,
    pub line: usize,
    pub column: usize,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
}

pub fn print_findings(
    findings: &[Finding],
    format: OutputFormat,
    root: &Path,
) -> Result<(), String> {
    match format {
        OutputFormat::Text => {
            for finding in findings {
                let location = format!("{}:{}:{}", finding.path, finding.line, finding.column);
                match finding.rule_id.as_deref() {
                    Some(rule_id) => println!(
                        "{}: [{}/{}] {}: {}",
                        location,
                        finding.severity.as_deref().unwrap_or("warning"),
                        rule_id,
                        finding.message.as_deref().unwrap_or("rule matched"),
                        finding.text
                    ),
                    None => println!("{}: {}", location, finding.text),
                }
            }
            Ok(())
        }
        OutputFormat::Json => {
            let output = serde_json::to_string_pretty(findings)
                .map_err(|error| format!("could not encode JSON: {error}"))?;
            println!("{output}");
            Ok(())
        }
        OutputFormat::Jsonl => {
            for finding in findings {
                let output = serde_json::to_string(finding)
                    .map_err(|error| format!("could not encode JSON: {error}"))?;
                println!("{output}");
            }
            Ok(())
        }
        OutputFormat::Sarif => {
            let output = serde_json::to_string_pretty(&sarif_log(findings, root))
                .map_err(|error| format!("could not encode SARIF: {error}"))?;
            println!("{output}");
            Ok(())
        }
    }
}

fn sarif_log(findings: &[Finding], root: &Path) -> Value {
    let mut rules = BTreeMap::new();
    for finding in findings {
        let id = finding.rule_id.as_deref().unwrap_or("pyastq/find");
        rules.entry(id).or_insert_with(|| {
            json!({
                "id": id,
                "shortDescription": {
                    "text": finding.message.as_deref().unwrap_or("Structural query matched")
                },
                "defaultConfiguration": {
                    "level": sarif_level(finding.severity.as_deref())
                }
            })
        });
    }
    let rule_indices: BTreeMap<_, _> = rules
        .keys()
        .enumerate()
        .map(|(index, id)| (*id, index))
        .collect();
    let results: Vec<_> = findings
        .iter()
        .map(|finding| {
            let rule_id = finding.rule_id.as_deref().unwrap_or("pyastq/find");
            let uri = artifact_uri(&finding.path, root);
            let message = finding
                .message
                .as_deref()
                .unwrap_or("Structural query matched");
            let fingerprint = content_hash(
                format!("{rule_id}\0{uri}\0{}\0{}", finding.line, finding.text).as_bytes(),
            );
            json!({
                "ruleId": rule_id,
                "ruleIndex": rule_indices[rule_id],
                "level": sarif_level(finding.severity.as_deref()),
                "message": { "text": message },
                "locations": [{
                    "physicalLocation": {
                        "artifactLocation": { "uri": uri },
                        "region": {
                            "startLine": finding.line,
                            "startColumn": finding.column,
                            "endLine": finding.line,
                            "endColumn": finding.column + finding.text.chars().count().max(1),
                            "snippet": { "text": finding.text }
                        }
                    }
                }],
                "partialFingerprints": {
                    "pyastq/v1": fingerprint
                }
            })
        })
        .collect();

    json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": {
                "driver": {
                    "name": "pyastq",
                    "version": env!("CARGO_PKG_VERSION"),
                    "rules": rules.into_values().collect::<Vec<_>>()
                }
            },
            "results": results
        }]
    })
}

fn sarif_level(severity: Option<&str>) -> &'static str {
    match severity {
        Some("error") => "error",
        Some("info") => "note",
        _ => "warning",
    }
}

fn artifact_uri(path: &str, root: &Path) -> String {
    let path = Path::new(path);
    let base = if root.is_file() {
        root.parent().unwrap_or_else(|| Path::new("."))
    } else {
        root
    };
    let relative = path.strip_prefix(base).unwrap_or(path);
    percent_encode_path(&relative.to_string_lossy().replace('\\', "/"))
}

fn percent_encode_path(path: &str) -> String {
    let mut encoded = String::new();
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~' | b'/') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{Finding, sarif_log};

    #[test]
    fn emits_sarif_2_1_0_with_rules_and_locations() {
        let log = sarif_log(
            &[Finding {
                path: "src/example.py".to_owned(),
                line: 4,
                column: 8,
                text: "eval(value)".to_owned(),
                rule_id: Some("no-eval".to_owned()),
                message: Some("Avoid eval".to_owned()),
                severity: Some("error".to_owned()),
            }],
            Path::new("."),
        );

        assert_eq!(log["version"], "2.1.0");
        assert_eq!(
            log["runs"][0]["tool"]["driver"]["rules"][0]["id"],
            "no-eval"
        );
        assert_eq!(log["runs"][0]["results"][0]["level"], "error");
        assert_eq!(
            log["runs"][0]["results"][0]["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
            "src/example.py"
        );
    }
}
