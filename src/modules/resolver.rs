//! Module path resolution and validation for Modus.

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    FileNotFound {
        path: PathBuf,
        imported_from: Option<PathBuf>,
    },
    InvalidExtension {
        path: PathBuf,
    },
    IoError {
        path: PathBuf,
        message: String,
    },
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FileNotFound {
                path,
                imported_from,
            } => {
                if let Some(from) = imported_from {
                    write!(
                        f,
                        "Module file '{}' imported from '{}' not found",
                        path.display(),
                        from.display()
                    )
                } else {
                    write!(f, "Module file '{}' not found", path.display())
                }
            }
            Self::InvalidExtension { path } => {
                write!(
                    f,
                    "Invalid module file extension for '{}'. Modus source files must have '.mds' or '.mdsi' extension.",
                    path.display()
                )
            }
            Self::IoError { path, message } => {
                write!(
                    f,
                    "Failed to read module file '{}': {}",
                    path.display(),
                    message
                )
            }
        }
    }
}

impl std::error::Error for ResolveError {}

/// Resolves an import path string relative to the importing file's directory.
pub fn resolve_module_path(
    import_specifier: &str,
    importing_file: Option<&Path>,
) -> Result<PathBuf, ResolveError> {
    let raw_path = PathBuf::from(import_specifier);

    // Determine target path relative to importing file's parent directory if relative
    let target = if raw_path.is_relative() {
        if let Some(parent) = importing_file.and_then(|p| p.parent()) {
            parent.join(&raw_path)
        } else {
            raw_path
        }
    } else {
        raw_path
    };

    // Normalize extension: if extension missing, try .mds
    let target = if target.extension().is_none() {
        target.with_extension("mds")
    } else {
        target
    };

    // Verify extension is .mds or .mdsi
    let ext = target.extension().and_then(|e| e.to_str()).unwrap_or("");
    if ext != "mds" && ext != "mdsi" {
        return Err(ResolveError::InvalidExtension { path: target });
    }

    // Canonicalize path
    fs::canonicalize(&target).map_err(|_| ResolveError::FileNotFound {
        path: target,
        imported_from: importing_file.map(|p| p.to_path_buf()),
    })
}
