use std::path::{Path, PathBuf};
use std::process::Command;

use regex::Regex;

pub struct FileFilter {
    includes: Vec<Vec<Regex>>,
    excludes: Vec<Regex>,
    changed: Option<Vec<PathBuf>>,
}

impl FileFilter {
    pub fn new(
        root: &Path,
        include_groups: &[Vec<String>],
        excludes: &[String],
        changed_only: bool,
    ) -> Result<Self, String> {
        let includes = include_groups
            .iter()
            .map(|patterns| compile_globs(patterns))
            .collect::<Result<_, _>>()?;
        let mut all_excludes = vec![
            "**/.git/**".to_owned(),
            "**/target/**".to_owned(),
            "**/.venv/**".to_owned(),
            "**/venv/**".to_owned(),
            "**/__pycache__/**".to_owned(),
        ];
        all_excludes.extend_from_slice(excludes);

        Ok(Self {
            includes,
            excludes: compile_globs(&all_excludes)?,
            changed: changed_only.then(|| changed_files(root)).transpose()?,
        })
    }

    pub fn accepts(&self, relative_path: &Path) -> bool {
        let path = normalize(relative_path);
        let included = self
            .includes
            .iter()
            .all(|group| group.is_empty() || group.iter().any(|pattern| pattern.is_match(&path)));
        let excluded = self.excludes.iter().any(|pattern| pattern.is_match(&path));
        let changed = self
            .changed
            .as_ref()
            .is_none_or(|changed| changed.iter().any(|candidate| normalize(candidate) == path));
        included && !excluded && changed
    }
}

pub fn collect_python_files(root: &Path, filter: &FileFilter) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    if root.is_file() {
        let base = root.parent().unwrap_or_else(|| Path::new("."));
        collect(base, root, filter, &mut files)?;
    } else {
        collect(root, root, filter, &mut files)?;
    }
    files.sort();
    Ok(files)
}

fn collect(
    root: &Path,
    path: &Path,
    filter: &FileFilter,
    files: &mut Vec<PathBuf>,
) -> Result<(), String> {
    if path.is_file() {
        if path.extension().is_some_and(|extension| extension == "py") {
            let relative = path.strip_prefix(root).unwrap_or(path);
            if filter.accepts(relative) {
                files.push(path.to_owned());
            }
        }
        return Ok(());
    }

    for entry in std::fs::read_dir(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?
    {
        let entry = entry.map_err(|error| format!("could not read directory entry: {error}"))?;
        let entry_path = entry.path();
        let relative = entry_path.strip_prefix(root).unwrap_or(&entry_path);

        if entry_path.is_dir() {
            let directory_path = format!("{}/", normalize(relative));
            if !filter
                .excludes
                .iter()
                .any(|pattern| pattern.is_match(&directory_path))
            {
                collect(root, &entry_path, filter, files)?;
            }
        } else {
            collect(root, &entry_path, filter, files)?;
        }
    }
    Ok(())
}

fn compile_globs(patterns: &[String]) -> Result<Vec<Regex>, String> {
    patterns
        .iter()
        .map(|pattern| {
            Regex::new(&glob_regex(pattern))
                .map_err(|error| format!("invalid path pattern `{pattern}`: {error}"))
        })
        .collect()
}

fn glob_regex(glob: &str) -> String {
    let mut expression = String::from("^");
    let mut characters = glob.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '*' if characters.peek() == Some(&'*') => {
                characters.next();
                if characters.peek() == Some(&'/') {
                    characters.next();
                    expression.push_str("(?:.*/)?");
                } else {
                    expression.push_str(".*");
                }
            }
            '*' => expression.push_str("[^/]*"),
            '?' => expression.push_str("[^/]"),
            character => expression.push_str(&regex::escape(&character.to_string())),
        }
    }
    expression.push('$');
    expression
}

fn changed_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let working_directory = if root.is_file() {
        root.parent().unwrap_or_else(|| Path::new("."))
    } else {
        root
    };
    let repository = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(working_directory)
        .output()
        .map_err(|error| format!("could not run git: {error}"))?;
    if !repository.status.success() {
        return Err("`--changed` requires a Git working tree".to_owned());
    }
    let repository = PathBuf::from(String::from_utf8_lossy(&repository.stdout).trim());

    let output = Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=all"])
        .current_dir(&repository)
        .output()
        .map_err(|error| format!("could not run git: {error}"))?;

    if !output.status.success() {
        return Err("`--changed` requires a Git working tree".to_owned());
    }

    let root = working_directory.canonicalize().map_err(|error| {
        format!(
            "could not resolve search path {}: {error}",
            working_directory.display()
        )
    })?;
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.get(3..))
        .map(|path| path.split(" -> ").last().unwrap_or(path))
        .filter(|path| path.ends_with(".py"))
        .filter_map(|path| {
            repository
                .join(path)
                .strip_prefix(&root)
                .ok()
                .map(PathBuf::from)
        })
        .collect())
}

fn normalize(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::glob_regex;
    use regex::Regex;

    #[test]
    fn supports_double_star_globs() {
        let regex = Regex::new(&glob_regex("src/**/*.py")).unwrap();
        assert!(regex.is_match("src/main.py"));
        assert!(regex.is_match("src/pkg/main.py"));
        assert!(!regex.is_match("tests/main.py"));
    }
}
