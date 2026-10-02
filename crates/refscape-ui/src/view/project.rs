//! Native pickers return typed commands; lifecycle belongs to the application.
use super::ExplorerView;
use gpui::{Context, PathPromptOptions};
use refscape_application::Command;
use refscape_model::ProjectOpenOptions;
use std::path::PathBuf;
impl ExplorerView {
    pub(super) fn open_project_with_session_options(
        &mut self,
        root: PathBuf,
        destination: PathBuf,
        options: ProjectOpenOptions,
        cx: &mut Context<Self>,
    ) {
        self.command(
            Command::OpenProject {
                root,
                options,
                destination,
            },
            cx,
        );
    }
    pub(super) fn pick_project(&mut self, cx: &mut Context<Self>) {
        if self.controller.busy() || self.controller.closing() {
            return;
        }
        let picker = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open Rust, C/C++, TypeScript/React, or Python source folder".into()),
        });
        cx.spawn(async move |view, cx| match picker.await {
            Ok(Ok(Some(paths))) => {
                if let Some(root) = paths.into_iter().next() {
                    let _ = view.update(cx, |view, cx| {
                        let destination = if view
                            .controller
                            .snapshot()
                            .project_root
                            .as_os_str()
                            .is_empty()
                            && !view.project.session_path.as_os_str().is_empty()
                        {
                            view.project.session_path.clone()
                        } else {
                            root.join(".refscape/session.json")
                        };
                        view.open_project_with_session_options(
                            root,
                            destination,
                            view.project.launch_options.clone(),
                            cx,
                        );
                    });
                }
            }
            Ok(Ok(None)) => {}
            result => {
                let _ = view.update(cx, |view, cx| {
                    view.requests.status = format!("Project picker failed: {result:?}");
                    view.requests.error = true;
                    cx.notify();
                });
            }
        })
        .detach();
    }
    pub(super) fn pick_compilation_database(&mut self, cx: &mut Context<Self>) {
        if self.controller.busy() || self.controller.closing() {
            return;
        }
        if self
            .controller
            .snapshot()
            .project_root
            .as_os_str()
            .is_empty()
            && self.controller.pending_project().is_none()
        {
            self.requests.status = "Open a source folder before selecting build settings.".into();
            cx.notify();
            return;
        }
        let picker = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Select compile_commands.json".into()),
        });
        cx.spawn(async move |view, cx| match picker.await {
            Ok(Ok(Some(paths))) => {
                if let Some(path) = paths.into_iter().next() {
                    let _ = view.update(cx, |view, cx| view.select_compilation_database(path, cx));
                }
            }
            Ok(Ok(None)) => {}
            result => {
                let _ = view.update(cx, |view, cx| {
                    view.requests.status = format!("Build settings picker failed: {result:?}");
                    view.requests.error = true;
                    cx.notify();
                });
            }
        })
        .detach();
    }
    pub(super) fn select_compilation_database(
        &mut self,
        database: PathBuf,
        cx: &mut Context<Self>,
    ) {
        self.command(Command::SetCompilationDatabase(database), cx);
    }
    pub(super) fn save(&mut self, cx: &mut Context<Self>) {
        self.command(Command::Save, cx);
    }
    pub(super) fn pick_session(&mut self, save: bool, cx: &mut Context<Self>) {
        if self.controller.busy() || self.controller.closing() {
            return;
        }
        if save {
            let picker = cx.prompt_for_new_path(
                self.project
                    .session_path
                    .parent()
                    .unwrap_or(std::path::Path::new(".")),
                Some("refscape-session.json"),
            );
            cx.spawn(async move |view, cx| match picker.await {
                Ok(Ok(Some(path))) => {
                    let _ = view.update(cx, |view, cx| view.command(Command::SaveAs(path), cx));
                }
                Ok(Ok(None)) => {}
                result => {
                    let _ = view.update(cx, |view, cx| {
                        view.requests.status = format!("Session picker failed: {result:?}");
                        view.requests.error = true;
                        cx.notify();
                    });
                }
            })
            .detach();
        } else {
            let picker = cx.prompt_for_paths(PathPromptOptions {
                files: true,
                directories: false,
                multiple: false,
                prompt: Some("Open Refscape session".into()),
            });
            cx.spawn(async move |view, cx| match picker.await {
                Ok(Ok(Some(paths))) => {
                    if let Some(path) = paths.into_iter().next() {
                        let _ = view.update(cx, |view, cx| view.open_session(path, cx));
                    }
                }
                Ok(Ok(None)) => {}
                result => {
                    let _ = view.update(cx, |view, cx| {
                        view.requests.status = format!("Session picker failed: {result:?}");
                        view.requests.error = true;
                        cx.notify();
                    });
                }
            })
            .detach();
        }
    }
    pub(super) fn open_session(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.command(
            Command::OpenSession {
                path,
                expected_root: None,
                overrides: self.project.launch_options.clone(),
            },
            cx,
        );
    }
}
