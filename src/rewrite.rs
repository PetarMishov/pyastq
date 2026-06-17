use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::query::{MatchCaptures, QueryVariables, is_variable_name};
use crate::report::Finding;
use crate::search::{python_parser, validate_python_with_parser};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum ChangeSafety {
    Safe,
    Unsafe,
}

impl Default for ChangeSafety {
    fn default() -> Self {
        Self::Safe
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Change {
    pub label: String,
    pub replace: String,
    #[serde(default)]
    pub safety: ChangeSafety,
}

#[derive(Default, Debug, Eq, PartialEq)]
pub struct ChangeSummary {
    pub applied: usize,
    pub skipped_unsafe: usize,
}

pub struct ChangeSpec<'a> {
    pub change: &'a Change,
    pub variables: &'a QueryVariables,
}

struct Edit {
    start_byte: usize,
    end_byte: usize,
    replacement: String,
}

pub fn apply_changes<'a, F>(
    findings: &[Finding],
    allow_unsafe: bool,
    mut spec_for: F,
) -> Result<ChangeSummary, String>
where
    F: FnMut(&Finding) -> Option<ChangeSpec<'a>>,
{
    let mut summary = ChangeSummary::default();
    let mut edits_by_path: BTreeMap<PathBuf, Vec<Edit>> = BTreeMap::new();

    for finding in findings {
        let Some(spec) = spec_for(finding) else {
            continue;
        };
        validate_change(spec.change)?;
        if spec.change.safety == ChangeSafety::Unsafe && !allow_unsafe {
            summary.skipped_unsafe += 1;
            continue;
        }
        let replacement = render_template(&spec.change.replace, spec.variables, &finding.captures)?;
        edits_by_path
            .entry(PathBuf::from(&finding.path))
            .or_default()
            .push(Edit {
                start_byte: finding.start_byte,
                end_byte: finding.end_byte,
                replacement,
            });
    }

    for (path, mut edits) in edits_by_path {
        summary.applied += apply_file_edits(&path, &mut edits)?;
    }

    Ok(summary)
}

pub fn render_template(
    template: &str,
    variables: &QueryVariables,
    captures: &MatchCaptures,
) -> Result<String, String> {
    let mut rendered = String::new();
    let mut characters = template.char_indices().peekable();

    while let Some((_, character)) = characters.next() {
        if character != '$' {
            rendered.push(character);
            continue;
        }

        if characters
            .peek()
            .is_some_and(|(_, character)| *character == '$')
        {
            characters.next();
            rendered.push('$');
            continue;
        }

        let Some((_, next)) = characters.peek().copied() else {
            return Err("replacement template ends with `$`".to_owned());
        };

        let name = if next == '{' {
            characters.next();
            let mut name = String::new();
            loop {
                let Some((_, character)) = characters.next() else {
                    return Err("unterminated replacement variable".to_owned());
                };
                if character == '}' {
                    break;
                }
                name.push(character);
            }
            name
        } else {
            let mut name = String::new();
            while let Some((_, character)) = characters.peek().copied() {
                if character == '_' || character.is_ascii_alphanumeric() {
                    characters.next();
                    name.push(character);
                } else {
                    break;
                }
            }
            name
        };

        if !is_variable_name(&name) {
            return Err(format!("invalid replacement variable `${name}`"));
        }
        let value = captures
            .get(&name)
            .or_else(|| variables.get(&name))
            .ok_or_else(|| format!("replacement variable `${name}` is not defined"))?;
        rendered.push_str(value);
    }

    Ok(rendered)
}

pub fn validate_change(change: &Change) -> Result<(), String> {
    if change.label.trim().is_empty() {
        return Err("change labels cannot be empty".to_owned());
    }
    Ok(())
}

fn apply_file_edits(path: &Path, edits: &mut [Edit]) -> Result<usize, String> {
    if edits.is_empty() {
        return Ok(0);
    }
    edits.sort_by_key(|edit| edit.start_byte);
    for pair in edits.windows(2) {
        if pair[0].end_byte > pair[1].start_byte {
            return Err(format!(
                "{} has overlapping changes at bytes {}..{} and {}..{}",
                path.display(),
                pair[0].start_byte,
                pair[0].end_byte,
                pair[1].start_byte,
                pair[1].end_byte
            ));
        }
    }

    let mut source = std::fs::read_to_string(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    for edit in edits.iter().rev() {
        source.replace_range(edit.start_byte..edit.end_byte, &edit.replacement);
    }

    validate_python_with_parser(&mut python_parser()?, path, &source)?;
    std::fs::write(path, source)
        .map_err(|error| format!("could not write {}: {error}", path.display()))?;
    Ok(edits.len())
}

#[cfg(test)]
mod tests {
    use super::render_template;
    use crate::query::{MatchCaptures, QueryVariables};

    #[test]
    fn renders_captures_variables_and_escaped_dollars() {
        let variables = QueryVariables::from([("module".to_owned(), "json".to_owned())]);
        let captures = MatchCaptures::from([("expr".to_owned(), "document".to_owned())]);

        assert_eq!(
            render_template("${module}.loads($expr) $$", &variables, &captures).unwrap(),
            "json.loads(document) $"
        );
    }
}
