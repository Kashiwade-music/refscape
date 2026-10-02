use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
};

#[derive(Clone, Copy)]
pub struct WalkPolicy {
    pub extensions: &'static [&'static str],
    pub excluded: &'static [&'static str],
    pub case_insensitive: bool,
    pub symlink_files: bool,
    pub exclude_virtual_environments: bool,
    pub canonical_paths: bool,
}
impl WalkPolicy {
    fn accepts(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|s| s.to_str())
            .is_some_and(|extension| {
                self.extensions.iter().any(|expected| {
                    if self.case_insensitive {
                        extension.eq_ignore_ascii_case(expected)
                    } else {
                        extension == *expected
                    }
                })
            })
    }
}

/// A single directory traversal retains independent language exclusion masks.
/// Excluding a path for one profile never excludes it for another profile.
pub struct ProjectProbe {
    pub catalogs: Vec<Vec<PathBuf>>,
}
impl ProjectProbe {
    pub fn scan(root: &Path, policies: &[WalkPolicy]) -> Result<Self, String> {
        Self::scan_with_context(
            root,
            policies,
            &refscape_model::OperationContext::detached(std::time::Duration::from_secs(120)),
        )
        .map_err(|error| error.to_string())
    }
    pub fn scan_with_context(
        root: &Path,
        policies: &[WalkPolicy],
        context: &refscape_model::OperationContext,
    ) -> Result<Self, refscape_model::RefscapeError> {
        context.check()?;
        let mut pending = vec![(root.to_path_buf(), (0..policies.len()).collect::<Vec<_>>())];
        let mut visited = vec![BTreeSet::new(); policies.len()];
        let mut catalogs = vec![BTreeSet::new(); policies.len()];
        while let Some((directory, active)) = pending.pop() {
            context.check()?;
            let identity = directory.canonicalize().map_err(|error| {
                refscape_model::RefscapeError::new(
                    refscape_model::ErrorKind::Io,
                    format!("cannot list {}: {error}", directory.display()),
                )
            })?;
            let active: Vec<_> = active
                .into_iter()
                .filter(|index| visited[*index].insert(identity.clone()))
                .collect();
            if active.is_empty() {
                continue;
            }
            for entry in fs::read_dir(&directory).map_err(|error| {
                refscape_model::RefscapeError::new(
                    refscape_model::ErrorKind::Io,
                    format!("cannot list {}: {error}", directory.display()),
                )
            })? {
                context.check()?;
                let entry = entry.map_err(refscape_model::RefscapeError::from)?;
                let kind = entry
                    .file_type()
                    .map_err(refscape_model::RefscapeError::from)?;
                let path = entry.path();
                if kind.is_dir() {
                    let next = active
                        .iter()
                        .copied()
                        .filter(|index| {
                            let policy = policies[*index];
                            !policy
                                .excluded
                                .iter()
                                .any(|excluded| entry.file_name() == *excluded)
                                && !(policy.exclude_virtual_environments
                                    && path.join("pyvenv.cfg").is_file())
                        })
                        .collect::<Vec<_>>();
                    if !next.is_empty() {
                        pending.push((path, next));
                    }
                } else {
                    for index in &active {
                        let policy = policies[*index];
                        if (kind.is_file()
                            || policy.symlink_files && kind.is_symlink() && path.is_file())
                            && policy.accepts(&path)
                        {
                            catalogs[*index].insert(if policy.canonical_paths {
                                path.canonicalize().map_err(|error| {
                                    refscape_model::RefscapeError::new(
                                        refscape_model::ErrorKind::Io,
                                        format!("cannot resolve {}: {error}", path.display()),
                                    )
                                })?
                            } else {
                                path.clone()
                            });
                        }
                    }
                }
            }
        }
        Ok(Self {
            catalogs: catalogs
                .into_iter()
                .map(|files| files.into_iter().collect())
                .collect(),
        })
    }
}
/// A probe can stop at the first accepted regular file; full catalogs sort only once.
pub fn walk(root: &Path, policy: WalkPolicy, first_only: bool) -> Result<Vec<PathBuf>, String> {
    walk_with_context(
        root,
        policy,
        first_only,
        &refscape_model::OperationContext::detached(std::time::Duration::from_secs(120)),
    )
    .map_err(|error| error.to_string())
}
pub fn walk_with_context(
    root: &Path,
    policy: WalkPolicy,
    first_only: bool,
    context: &refscape_model::OperationContext,
) -> Result<Vec<PathBuf>, refscape_model::RefscapeError> {
    let mut pending = vec![root.to_path_buf()];
    let mut visited = BTreeSet::new();
    let mut output = BTreeSet::new();
    while let Some(directory) = pending.pop() {
        context.check()?;
        let identity = directory.canonicalize().map_err(|e| {
            refscape_model::RefscapeError::new(
                refscape_model::ErrorKind::Io,
                format!("cannot list {}: {e}", directory.display()),
            )
        })?;
        if !visited.insert(identity) {
            continue;
        }
        for entry in fs::read_dir(&directory).map_err(|e| {
            refscape_model::RefscapeError::new(
                refscape_model::ErrorKind::Io,
                format!("cannot list {}: {e}", directory.display()),
            )
        })? {
            context.check()?;
            let entry = entry.map_err(refscape_model::RefscapeError::from)?;
            let kind = entry
                .file_type()
                .map_err(refscape_model::RefscapeError::from)?;
            let path = entry.path();
            if kind.is_dir() {
                if !policy
                    .excluded
                    .iter()
                    .any(|excluded| entry.file_name() == *excluded)
                    && !(policy.exclude_virtual_environments && path.join("pyvenv.cfg").is_file())
                {
                    pending.push(path);
                }
            } else if (kind.is_file()
                || policy.symlink_files && kind.is_symlink() && path.is_file())
                && policy.accepts(&path)
            {
                output.insert(if policy.canonical_paths {
                    path.canonicalize().map_err(|e| {
                        refscape_model::RefscapeError::new(
                            refscape_model::ErrorKind::Io,
                            format!("cannot resolve {}: {e}", path.display()),
                        )
                    })?
                } else {
                    path
                });
                if first_only {
                    return Ok(output.into_iter().collect());
                }
            }
        }
    }
    Ok(output.into_iter().collect())
}

#[derive(Clone, Debug, Default)]
pub struct CatalogSnapshot {
    pub revision: u64,
    pub ordered_files: Vec<PathBuf>,
    fingerprints: BTreeMap<PathBuf, u64>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CatalogDelta {
    pub added: Vec<PathBuf>,
    pub changed: Vec<PathBuf>,
    pub removed: Vec<PathBuf>,
}
impl CatalogSnapshot {
    /// Hash bytes as well as metadata: same-size same-mtime external edits are visible.
    pub fn refresh(&self, files: Vec<PathBuf>) -> Result<(Self, CatalogDelta), String> {
        self.refresh_with_context(
            files,
            &refscape_model::OperationContext::detached(std::time::Duration::from_secs(120)),
        )
        .map_err(|error| error.to_string())
    }
    pub fn refresh_with_context(
        &self,
        mut files: Vec<PathBuf>,
        context: &refscape_model::OperationContext,
    ) -> Result<(Self, CatalogDelta), refscape_model::RefscapeError> {
        context.check()?;
        files.sort();
        files.dedup();
        let mut fingerprints = BTreeMap::new();
        for path in &files {
            context.check()?;
            let bytes = fs::read(path).map_err(|e| {
                refscape_model::RefscapeError::new(
                    refscape_model::ErrorKind::Io,
                    format!("cannot read {}: {e}", path.display()),
                )
            })?;
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            bytes.hash(&mut hasher);
            fingerprints.insert(path.clone(), hasher.finish());
        }
        let mut delta = CatalogDelta::default();
        for (path, fingerprint) in &fingerprints {
            match self.fingerprints.get(path) {
                None => delta.added.push(path.clone()),
                Some(old) if old != fingerprint => delta.changed.push(path.clone()),
                _ => {}
            }
        }
        delta.removed = self
            .fingerprints
            .keys()
            .filter(|path| !fingerprints.contains_key(*path))
            .cloned()
            .collect();
        let dirty =
            !delta.added.is_empty() || !delta.changed.is_empty() || !delta.removed.is_empty();
        context.check()?;
        Ok((
            Self {
                revision: self.revision.checked_add(u64::from(dirty)).ok_or_else(|| {
                    refscape_model::RefscapeError::new(
                        refscape_model::ErrorKind::InternalInvariant,
                        "Catalog revision overflow",
                    )
                })?,
                ordered_files: files,
                fingerprints,
            },
            delta,
        ))
    }
}
