//! Cargo owns the authoritative package boundaries and target source locations.
use refscape_model::ProjectCrate;
use serde::Deserialize;
use std::{
    collections::BTreeSet,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
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
    pub(crate) fn discover(root: &Path, timeout: Duration) -> Result<Self, String> {
        let mut command = Command::new("cargo");
        command
            .args(["metadata", "--no-deps", "--format-version", "1"])
            .arg("--manifest-path")
            .arg(root.join("Cargo.toml"))
            .current_dir(root)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command
            .spawn()
            .map_err(|error| format!("cannot run cargo metadata: {error}"))?;
        let mut stdout = child.stdout.take().ok_or("missing cargo metadata stdout")?;
        let mut stderr = child.stderr.take().ok_or("missing cargo metadata stderr")?;
        let (out_sender, out_receiver) = mpsc::channel();
        let (err_sender, err_receiver) = mpsc::channel();
        thread::spawn(move || {
            let mut bytes = vec![];
            let result = (&mut stdout)
                .take(64 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map(|_| bytes);
            let _ = out_sender.send(result);
        });
        thread::spawn(move || {
            let mut bytes = vec![];
            let _ = (&mut stderr).take(1024 * 1024).read_to_end(&mut bytes);
            let _ = err_sender.send(bytes);
        });
        let deadline = Instant::now() + timeout;
        let status = loop {
            match child.try_wait().map_err(|error| error.to_string())? {
                Some(status) => break status,
                None if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("cargo metadata timed out while loading package boundaries".into());
                }
                None => thread::sleep(Duration::from_millis(10)),
            }
        };
        let bytes = out_receiver
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|error| format!("cannot read cargo metadata: {error}"))?
            .map_err(|error| format!("cannot read cargo metadata: {error}"))?;
        if !status.success() {
            let errors = err_receiver
                .recv_timeout(Duration::from_millis(100))
                .unwrap_or_default();
            return Err(format!(
                "cargo metadata failed: {}",
                String::from_utf8_lossy(&errors).trim()
            ));
        }
        if bytes.len() > 64 * 1024 * 1024 {
            return Err("cargo metadata exceeds 64 MiB".into());
        }
        Self::parse(&bytes)
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
