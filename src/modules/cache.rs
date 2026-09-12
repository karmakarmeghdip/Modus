//! Incremental compilation caching and early-cutoff fingerprinting for Modus modules.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModuleCacheEntry {
    pub source_hash: u64,
    pub interface_hash: u64,
    pub build_fingerprint: String,
    pub object_file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CacheManifest {
    pub entries: HashMap<String, ModuleCacheEntry>,
}

/// Computes a hash of the module's raw source code.
pub fn hash_source(source: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    hasher.finish()
}

/// Computes the build fingerprint for a module based on its own source hash and the
/// interface hashes of all its direct dependencies.
/// Early-cutoff invariant: If a dependency's body changed but its interface hash did not,
/// the downstream module's fingerprint remains identical.
pub fn compute_fingerprint(source_hash: u64, dep_interface_hashes: &[u64]) -> String {
    let mut hasher = DefaultHasher::new();
    source_hash.hash(&mut hasher);
    for h in dep_interface_hashes {
        h.hash(&mut hasher);
    }
    // Compiler ABI tag
    "modus_abi_v1".hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Manages the on-disk `.modus-cache/` store.
pub struct CacheStore {
    pub root_dir: PathBuf,
    pub objects_dir: PathBuf,
    pub manifest_file: PathBuf,
    pub manifest: CacheManifest,
}

impl CacheStore {
    pub fn new(cache_root: &Path) -> Self {
        let root_dir = cache_root.to_path_buf();
        let objects_dir = root_dir.join("objects");
        let manifest_file = root_dir.join("manifest.json");

        let manifest = if manifest_file.exists() {
            if let Ok(data) = std::fs::read_to_string(&manifest_file) {
                serde_json::from_str(&data).unwrap_or_default()
            } else {
                CacheManifest::default()
            }
        } else {
            CacheManifest::default()
        };

        Self {
            root_dir,
            objects_dir,
            manifest_file,
            manifest,
        }
    }

    /// Checks if a valid cached object file exists for the module with this fingerprint.
    pub fn is_cached(&self, module_path: &Path, fingerprint: &str) -> Option<PathBuf> {
        let key = module_path.to_string_lossy().to_string();
        let entry = self.manifest.entries.get(&key)?;
        if entry.build_fingerprint == fingerprint {
            let obj_path = self.objects_dir.join(&entry.object_file);
            if obj_path.exists() {
                return Some(obj_path);
            }
        }
        None
    }

    /// Stores a freshly compiled object file into the cache and updates the manifest.
    pub fn store_object(
        &mut self,
        module_path: &Path,
        source_hash: u64,
        interface_hash: u64,
        fingerprint: &str,
        obj_file_temp: &Path,
    ) -> Result<PathBuf, std::io::Error> {
        std::fs::create_dir_all(&self.objects_dir)?;
        let obj_filename = format!("{fingerprint}.o");
        let target_path = self.objects_dir.join(&obj_filename);
        std::fs::copy(obj_file_temp, &target_path)?;

        let key = module_path.to_string_lossy().to_string();
        self.manifest.entries.insert(
            key,
            ModuleCacheEntry {
                source_hash,
                interface_hash,
                build_fingerprint: fingerprint.to_string(),
                object_file: obj_filename,
            },
        );

        self.save_manifest()?;
        Ok(target_path)
    }

    /// Persists the cache manifest to disk.
    pub fn save_manifest(&self) -> Result<(), std::io::Error> {
        std::fs::create_dir_all(&self.root_dir)?;
        let json = serde_json::to_string_pretty(&self.manifest).map_err(std::io::Error::other)?;
        std::fs::write(&self.manifest_file, json)?;
        Ok(())
    }

    /// Purges all cached files and resets the manifest.
    pub fn clean(&mut self) -> Result<(), std::io::Error> {
        if self.root_dir.exists() {
            std::fs::remove_dir_all(&self.root_dir)?;
        }
        self.manifest = CacheManifest::default();
        Ok(())
    }
}
