use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::report::Finding;

const CACHE_VERSION: u32 = 2;
const CACHE_FILE: &str = ".pyastq-cache.json";

#[derive(Default, Deserialize, Serialize)]
struct CacheData {
    version: u32,
    files: BTreeMap<String, CachedFile>,
}

#[derive(Deserialize, Serialize)]
struct CachedFile {
    hash: String,
    results: BTreeMap<String, Vec<Finding>>,
}

pub struct SearchCache {
    path: PathBuf,
    base: PathBuf,
    data: CacheData,
}

impl SearchCache {
    pub fn load(root: &Path) -> Self {
        let base = if root.is_file() {
            root.parent().unwrap_or_else(|| Path::new("."))
        } else {
            root
        };
        let path = base.join(CACHE_FILE);
        let data = std::fs::read_to_string(&path)
            .ok()
            .and_then(|source| serde_json::from_str::<CacheData>(&source).ok())
            .filter(|data| data.version == CACHE_VERSION)
            .unwrap_or_else(|| CacheData {
                version: CACHE_VERSION,
                files: BTreeMap::new(),
            });
        Self {
            path,
            base: base.to_owned(),
            data,
        }
    }

    pub fn file_key(&self, path: &Path) -> String {
        path.strip_prefix(&self.base)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    }

    pub fn findings(&self, file: &str, hash: &str, result_key: &str) -> Option<Vec<Finding>> {
        let cached = self.data.files.get(file)?;
        if cached.hash != hash {
            return None;
        }
        cached.results.get(result_key).cloned()
    }

    pub fn store(&mut self, file: &str, hash: &str, result_key: String, findings: Vec<Finding>) {
        let cached = self
            .data
            .files
            .entry(file.to_owned())
            .or_insert_with(|| CachedFile {
                hash: hash.to_owned(),
                results: BTreeMap::new(),
            });
        if cached.hash != hash {
            cached.hash = hash.to_owned();
            cached.results.clear();
        }
        cached.results.insert(result_key, findings);
    }

    pub fn retain_result_files(&mut self, result_key: &str, files: &BTreeSet<String>) {
        self.data.files.retain(|file, cached| {
            if !files.contains(file) {
                cached.results.remove(result_key);
            }
            !cached.results.is_empty()
        });
    }

    pub fn save(&self) -> Result<(), String> {
        let source = serde_json::to_vec_pretty(&self.data)
            .map_err(|error| format!("could not encode cache: {error}"))?;
        let temporary = self.path.with_extension("json.tmp");
        std::fs::write(&temporary, source)
            .map_err(|error| format!("could not write {}: {error}", temporary.display()))?;
        std::fs::rename(&temporary, &self.path)
            .map_err(|error| format!("could not replace {}: {error}", self.path.display()))
    }
}

pub fn content_hash(source: &[u8]) -> String {
    let first = fnv1a(source, 0xcbf29ce484222325);
    let second = fnv1a(source, 0x84222325cbf29ce4);
    format!("{first:016x}{second:016x}")
}

fn fnv1a(source: &[u8], seed: u64) -> u64 {
    source.iter().fold(seed, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

#[cfg(test)]
mod tests {
    use super::content_hash;

    #[test]
    fn content_hash_is_stable_and_content_sensitive() {
        assert_eq!(content_hash(b"same"), content_hash(b"same"));
        assert_ne!(content_hash(b"first"), content_hash(b"second"));
    }
}
