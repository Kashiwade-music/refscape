use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    path::PathBuf,
    sync::Arc,
};

/// Environment captured once at the composition boundary.
#[derive(Clone, Debug, Default)]
pub struct EnvironmentSnapshot {
    values: Arc<BTreeMap<OsString, OsString>>,
}
impl EnvironmentSnapshot {
    pub fn capture() -> Self {
        Self {
            values: Arc::new(std::env::vars_os().collect()),
        }
    }
    pub fn from_values(values: impl IntoIterator<Item = (OsString, OsString)>) -> Self {
        Self {
            values: Arc::new(values.into_iter().collect()),
        }
    }
    pub fn get(&self, key: impl AsRef<OsStr>) -> Option<&OsStr> {
        let key = key.as_ref();
        self.values
            .get(key)
            .or_else(|| {
                #[cfg(windows)]
                {
                    self.values
                        .iter()
                        .find(|(name, _)| {
                            name.to_string_lossy()
                                .eq_ignore_ascii_case(&key.to_string_lossy())
                        })
                        .map(|(_, value)| value)
                }
                #[cfg(not(windows))]
                {
                    None
                }
            })
            .map(OsString::as_os_str)
    }
    pub fn configured_executable(
        &self,
        variable: &str,
        fallback: &str,
    ) -> crate::resolver::ConfiguredExecutable {
        match self.get(variable) {
            Some(path) => crate::resolver::ConfiguredExecutable {
                path: path.into(),
                origin: crate::resolver::LaunchOrigin::Environment,
            },
            None => crate::resolver::ConfiguredExecutable::default_name(fallback),
        }
    }
    pub fn path(&self) -> Vec<PathBuf> {
        self.get("PATH")
            .map(|value| std::env::split_paths(value).collect())
            .unwrap_or_default()
    }
    pub fn values(&self) -> &BTreeMap<OsString, OsString> {
        &self.values
    }
}
