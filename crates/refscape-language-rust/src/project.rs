//! Cargo owns authoritative package and target boundaries.
use refscape_model::ProjectCrate;
use serde::Deserialize;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    workspace_members: Vec<String>,
}

#[derive(Deserialize)]
struct Package {
    id: String,
    name: String,
    manifest_path: PathBuf,
    targets: Vec<Target>,
}

#[derive(Deserialize)]
struct Target {
    src_path: PathBuf,
}

pub(crate) struct Project {
    pub(crate) crates: Vec<ProjectCrate>,
    pub(crate) targets: Vec<PathBuf>,
}

impl Project {
    pub(crate) fn discover(
        root: &Path,
        context: &refscape_model::OperationContext,
        environment: &refscape_language_support::EnvironmentSnapshot,
    ) -> Result<Self, refscape_model::RefscapeError> {
        let launch = refscape_language_support::resolver::resolve(
            root,
            &refscape_language_support::resolver::ConfiguredExecutable::default_name("cargo"),
            refscape_language_support::resolver::ServerKind::Native,
            environment,
        )?;
        let mut command = launch.command();
        command
            .args(["metadata", "--no-deps", "--format-version", "1"])
            .arg("--manifest-path")
            .arg(root.join("Cargo.toml"))
            .current_dir(root);
        let output = refscape_language_support::process::run(&mut command, context)?;
        if !output.success {
            return Err(refscape_model::RefscapeError::new(
                refscape_model::ErrorKind::BackendUnavailable,
                format!(
                    "cargo metadata failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            ));
        }
        Self::parse(&output.stdout).map_err(|e| {
            refscape_model::RefscapeError::new(refscape_model::ErrorKind::InvalidData, e)
        })
    }
    fn parse(bytes: &[u8]) -> Result<Self, String> {
        let metadata: Metadata = serde_json::from_slice(bytes)
            .map_err(|error| format!("invalid cargo metadata: {error}"))?;
        let members = metadata
            .workspace_members
            .into_iter()
            .collect::<BTreeSet<_>>();
        let mut crates = vec![];
        let mut targets = vec![];
        for package in metadata.packages {
            if !members.contains(&package.id) {
                continue;
            }
            let root = package
                .manifest_path
                .parent()
                .ok_or("Cargo package manifest has no parent directory")?
                .canonicalize()
                .map_err(|error| {
                    format!("cannot locate Cargo package {}: {error}", package.name)
                })?;
            crates.push(ProjectCrate {
                id: package.id,
                name: package.name,
                root,
            });
            for target in package.targets {
                let path = target.src_path.canonicalize().map_err(|error| {
                    format!(
                        "cannot locate Cargo source {}: {error}",
                        target.src_path.display()
                    )
                })?;
                targets.push(path);
            }
        }
        crates.sort_by(|left, right| (&left.name, &left.id).cmp(&(&right.name, &right.id)));
        targets.sort();
        targets.dedup();
        Ok(Self { crates, targets })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn metadata_package_ids_remain_opaque_and_only_workspace_members_are_kept() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs");
        let metadata = json!({"workspace_members":["opaque package identifier"],"packages":[
            {"id":"opaque package identifier","name":"actual-name","manifest_path":manifest,"targets":[{"src_path":source}]},
            {"id":"dependency","name":"other","manifest_path":"missing/Cargo.toml","targets":[]}
        ]});
        let project = Project::parse(&serde_json::to_vec(&metadata).unwrap()).unwrap();
        assert_eq!(project.crates.len(), 1);
        assert_eq!(project.crates[0].id, "opaque package identifier");
        assert_eq!(
            project.crates[0].root,
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .canonicalize()
                .unwrap()
        );
        assert_eq!(project.targets, [source.canonicalize().unwrap()]);
    }
}
