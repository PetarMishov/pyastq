use clap::ValueEnum;
use serde::Serialize;

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum OutputFormat {
    Text,
    Json,
    Jsonl,
}

#[derive(Debug, Serialize)]
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

pub fn print_findings(findings: &[Finding], format: OutputFormat) -> Result<(), String> {
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
    }
}
