//! Project selection, build settings, and session persistence.
use super::*;

/// A successful backend switch must update its save destination even when enumeration fails.
pub(super) fn project_open_output<L: LanguageService, R: SessionRepository>(
    explorer: &mut Explorer<L, R>,
    session_path: PathBuf,
    message: Option<String>,
) -> Output {
    let mut output = Output {
        reset: true,
        protect_session: message.is_some(),
        message,
        session_path: Some(session_path),
        opened_project: true,
        ..Default::default()
    };
    match explorer.files() {
        Ok(files) => output.files = Some(files),
        Err(error) => {
            output.files = Some(vec![]);
            output.error = true;
            output.protect_session = true;
            let error = format!("Project opened; source file listing failed: {error}");
            output.message = Some(match output.message.take() {
                Some(previous) => format!("{previous}\n{error}"),
                None => error,
            });
        }
    }
    output
}

impl<L: LanguageService + 'static, R: SessionRepository + 'static> ExplorerView<L, R> {
    pub(super) fn open_project_with_session(
        &mut self,
        path: PathBuf,
        session_path: PathBuf,
        cx: &mut Context<Self>,
    ) {
        self.open_project_with_session_options(
            path,
            session_path,
            self.project.launch_options.clone(),
            cx,
        );
    }

    pub(super) fn open_project_with_session_options(
        &mut self,
        path: PathBuf,
        session_path: PathBuf,
        options: ProjectOptions,
        cx: &mut Context<Self>,
    ) {
        if self.requests.busy || self.requests.closing {
            return;
        }
        self.project.pending = Some((path.clone(), session_path.clone()));
        let previous = if self.project.autosave && !self.session.project_root.as_os_str().is_empty()
        {
            Some(self.project.session_path.clone())
        } else {
            None
        };
        self.project.symbols.clear();
        self.search.query.clear();
        self.search.selection = 0..0;
        self.search.marked = None;
        self.run_job(
            "Starting language service and indexing project…",
            Box::new(move |explorer| {
                if let Some(previous) = previous {
                    explorer.save_session(&previous)?;
                }
                let mut message = None;
                let active_theme = explorer.session().theme.clone();
                if session_path.is_file() {
                    if let Err(error) =
                        explorer.load_project_session(&session_path, &path, &options)
                    {
                        explorer.open_project(&path, &options)?;
                        message = Some(format!("Project opened; session restore failed: {error}"));
                    }
                    if active_theme != Theme::dark() && active_theme != Theme::light() {
                        explorer.set_theme(active_theme)?;
                    }
                } else {
                    explorer.open_project(&path, &options)?;
                }
                Ok(project_open_output(explorer, session_path, message))
            }),
            cx,
        );
    }

    pub(super) fn pick_project(&mut self, cx: &mut Context<Self>) {
        if self.requests.busy || self.requests.closing {
            return;
        }
        let picker = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open Rust or C/C++ source folder".into()),
        });
        cx.spawn(async move |view, cx| match picker.await {
            Ok(Ok(Some(paths))) => {
                if let Some(path) = paths.into_iter().next() {
                    let _ = view.update(cx, |view, cx| {
                        let session_path = if view.session.project_root.as_os_str().is_empty()
                            && !view.project.session_path.as_os_str().is_empty()
                        {
                            view.project.session_path.clone()
                        } else {
                            path.join(".refscape/session.json")
                        };
                        view.open_project_with_session(path, session_path, cx);
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
        if self.requests.busy || self.requests.closing {
            return;
        }
        if self.project.pending.is_none() && self.session.project_root.as_os_str().is_empty() {
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
                    let _ = view.update(cx, |view, cx| {
                        view.select_compilation_database(path, cx);
                    });
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
        let project = self.project.pending.clone().or_else(|| {
            (!self.session.project_root.as_os_str().is_empty()).then(|| {
                (
                    self.session.project_root.clone(),
                    self.project.session_path.clone(),
                )
            })
        });
        if let Some((root, session_path)) = project {
            self.open_project_with_session_options(
                root,
                session_path,
                ProjectOptions {
                    language: ProjectLanguage::Cpp,
                    compilation_database: Some(database),
                },
                cx,
            );
        }
    }

    pub(super) fn save(&mut self, cx: &mut Context<Self>) {
        let path = self.project.session_path.clone();
        self.run_job(
            "Saving session…",
            Box::new(move |explorer| {
                explorer.save_session(&path)?;
                Ok(Output {
                    message: Some(format!("Session saved: {}", display_path(&path))),
                    session_path: Some(path),
                    ..Default::default()
                })
            }),
            cx,
        );
    }

    pub(super) fn pick_session(&mut self, save: bool, cx: &mut Context<Self>) {
        if self.requests.busy || self.requests.closing {
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
                    let _ = view.update(cx, |view, cx| {
                        view.run_job(
                            "Saving session…",
                            Box::new(move |explorer| {
                                explorer.save_session(&path)?;
                                Ok(Output {
                                    session_path: Some(path),
                                    message: Some("Session saved.".into()),
                                    ..Default::default()
                                })
                            }),
                            cx,
                        );
                    });
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
                        let _ = view.update(cx, |view, cx| {
                            view.open_session(path, cx);
                        });
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
        self.run_job(
            "Reading session settings…",
            Box::new(move |explorer| {
                let root = explorer.session_project_root(&path)?;
                Ok(Output {
                    restore_session: Some((root, path)),
                    ..Default::default()
                })
            }),
            cx,
        );
    }

    pub(super) fn restore_session(&mut self, root: PathBuf, path: PathBuf, cx: &mut Context<Self>) {
        let previous = if self.project.autosave
            && !self.session.project_root.as_os_str().is_empty()
            && self.project.session_path != path
        {
            Some(self.project.session_path.clone())
        } else {
            None
        };
        let options = self.project.launch_options.clone();
        self.run_job(
            "Restoring session…",
            Box::new(move |explorer| {
                if let Some(previous) = previous {
                    explorer.save_session(&previous)?;
                }
                explorer.load_project_session(&path, &root, &options)?;
                Ok(project_open_output(explorer, path, None))
            }),
            cx,
        );
    }
}
