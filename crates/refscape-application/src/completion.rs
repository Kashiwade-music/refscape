use super::*;
impl ApplicationController {
    fn adopt_metadata(
        &mut self,
        metadata: refscape_analysis::AnalysisMetadata,
        t: &mut Transition,
    ) -> Result<()> {
        if metadata.catalog_revision <= self.metadata_revision {
            return Ok(());
        }
        let cards = crate::editing::layout_cards(&self.snapshot.cards)?;
        let regions = refscape_canvas::regions::build_regions(
            &cards,
            &self.snapshot.project_root,
            &metadata.crates,
        );
        let options = metadata.options.to_open_options();
        let changed =
            self.snapshot.project_options != options || self.crates.as_ref() != &metadata.crates;
        let snapshot = Arc::make_mut(&mut self.snapshot);
        snapshot.project_options = options;
        snapshot.regions = Arc::new(regions);
        self.project_state = ProjectState::Open {
            epoch: self.basis.project,
            options: metadata.options,
        };
        self.crates = Arc::new(metadata.crates);
        self.jobs.catalog_files = metadata.files.len();
        self.metadata_revision = metadata.catalog_revision;
        if changed {
            self.presentation_changed();
        }
        t.events.push(ViewEvent::Files(metadata.files));
        Ok(())
    }
    pub fn complete(&mut self, completion: Completion) -> Transition {
        let mut t = Transition::default();
        match completion {
            Completion::ProjectPrepared {
                context,
                request,
                result,
            } => {
                let accepted = self.jobs.records.remove(&context.id).is_some_and(|record| {
                    record.class == JobClass::Switch
                        && record.context.project == context.project
                        && !context.cancel.is_cancelled()
                });
                if !accepted {
                    if result.is_ok() && context.project != self.basis.project {
                        t.effects.push(Effect::DisposeProject {
                            epoch: context.project,
                        });
                    }
                    return t;
                }
                match *result {
                    Ok(project) => {
                        self.project_state = ProjectState::Open {
                            epoch: context.project,
                            options: project.options,
                        };
                        self.metadata_revision = 0;
                        self.jobs.catalog_files = project.files.len();
                        let old = self.basis.project;
                        self.snapshot = Arc::new(project.snapshot);
                        self.canvas = CanvasStore::index(&self.snapshot);
                        self.basis = EditBasis {
                            project: context.project,
                            ..Default::default()
                        };
                        self.revision = u64::from(project.refreshed);
                        self.saved_revision = 0;
                        self.presentation = PresentationRevision::default();
                        self.undo = None;
                        self.pending_project = None;
                        self.crates = Arc::new(project.crates);
                        self.selected = None;
                        self.interaction += 1;
                        self.pending_plan = None;
                        self.destination = if project.destination.as_os_str().is_empty() {
                            SaveDestination::Unset
                        } else {
                            match project.protection {
                                Some(reason) => SaveDestination::Protected {
                                    path: project.destination,
                                    reason,
                                },
                                None => SaveDestination::Writable(project.destination),
                            }
                        };
                        t.effects.push(Effect::DisposeProject { epoch: old });
                        t.events.push(ViewEvent::Reset);
                        t.events.push(ViewEvent::Files(project.files));
                        t.events.push(ViewEvent::ClearInspection);
                        if let SaveDestination::Protected { reason, .. } = &self.destination {
                            self.status_event(reason.clone(), project.listing_failed, &mut t);
                        } else {
                            self.ready(&mut t);
                        }
                    }
                    Err(error) => {
                        self.pending_project = Some(request);
                        self.fail(error, &mut t);
                    }
                }
            }
            Completion::AnalysisQueried {
                context,
                basis,
                result,
                metadata,
            } => {
                let Some(record) = self.jobs.records.remove(&context.id) else {
                    return t;
                };
                if context.project != self.basis.project
                    || basis.project != self.basis.project
                    || context.cancel.is_cancelled()
                {
                    return t;
                }
                if result.is_ok()
                    && let Some(metadata) = metadata
                    && let Err(error) = self.adopt_metadata(*metadata, &mut t)
                {
                    self.fail(error, &mut t);
                    return t;
                }
                match result {
                    Ok(AnalysisReply::Unchanged) => {}
                    Ok(AnalysisReply::Files(files)) => {
                        t.events.push(ViewEvent::Files(files));
                        self.ready(&mut t);
                    }
                    Ok(AnalysisReply::Symbols(symbols)) | Ok(AnalysisReply::Search(symbols)) => {
                        t.events.push(ViewEvent::Symbols(symbols));
                        self.ready(&mut t);
                    }
                    Ok(AnalysisReply::Edit { edit, symbols }) => {
                        if basis.content != self.basis.content {
                            return t;
                        }
                        if let Some(symbols) = symbols {
                            t.events.push(ViewEvent::Symbols(symbols));
                        }
                        if self.dragging {
                            if record.class == JobClass::Reload && self.pending_plan.is_some() {
                                return t;
                            }
                            self.pending_plan = Some((edit, record.class));
                        } else {
                            self.plan(edit, record.class, &mut t);
                        }
                    }
                    Ok(AnalysisReply::Hover {
                        card,
                        source,
                        position: _,
                        value,
                    }) => {
                        if self
                            .canvas
                            .card(&self.snapshot, &card)
                            .is_some_and(|current| {
                                current.source.snapshot().revision == source.snapshot().revision
                            })
                        {
                            t.events.push(ViewEvent::Hover(value));
                        }
                    }
                    Ok(AnalysisReply::Inspection {
                        card,
                        source,
                        value,
                    }) => {
                        if self
                            .canvas
                            .card(&self.snapshot, &card)
                            .is_some_and(|current| {
                                current.source.snapshot().revision == source.snapshot().revision
                            })
                            && let Some(value) = value
                        {
                            t.events.push(ViewEvent::Inspection(value));
                        }
                    }
                    Err(error) => {
                        if !matches!(error.kind, ErrorKind::Cancelled | ErrorKind::Stale) {
                            self.fail(error, &mut t);
                        }
                    }
                }
            }
            Completion::CanvasPlanned {
                context,
                basis,
                edit,
                interaction,
                result,
            } => {
                let Some(record) = self.jobs.records.remove(&context.id) else {
                    return t;
                };
                if context.project != self.basis.project || context.cancel.is_cancelled() {
                    return t;
                }
                if record.class == JobClass::Reload && basis.content != self.basis.content {
                    return t;
                }
                if self.dragging {
                    if record.class == JobClass::Reload && self.pending_plan.is_some() {
                        return t;
                    }
                    self.pending_plan = Some((edit, record.class));
                    return t;
                }
                let stale = basis != self.basis
                    || (record.class == JobClass::Arrange && interaction != self.interaction);
                if stale {
                    if basis.project == self.basis.project && basis.content == self.basis.content {
                        let edit = if record.class == JobClass::Arrange {
                            PreparedEdit::Arrange {
                                selected: self.selected.clone(),
                            }
                        } else {
                            edit
                        };
                        self.plan(edit, record.class, &mut t);
                    }
                    return t;
                }
                match result {
                    Ok(patch) => {
                        let state = Arc::make_mut(&mut self.snapshot);
                        state.cards = patch.cards;
                        state.connections = patch.connections;
                        state.regions = patch.regions;
                        if patch.topology {
                            self.basis.topology.0 += 1;
                        }
                        if patch.content {
                            self.basis.content.0 += 1;
                        }
                        if patch.geometry {
                            self.basis.geometry.0 += 1;
                        }
                        if patch.topology || patch.content || patch.geometry {
                            self.revision += 1;
                            self.undo = None;
                        }
                        if patch.topology || patch.content {
                            self.canvas = CanvasStore::index(&self.snapshot);
                        }
                        if record.class == JobClass::Reload && (patch.geometry || patch.topology) {
                            self.cancel(JobClass::Hover, &mut t);
                            self.cancel(JobClass::Inspection, &mut t);
                            t.events.push(ViewEvent::ClearInspection);
                            t.events.push(ViewEvent::SourceReloaded);
                        }
                        if patch.invalidate_undo {
                            self.undo = None;
                        }
                        if let Some(positions) = patch.undo {
                            self.undo = Some(LayoutUndo {
                                basis: self.basis,
                                positions,
                            });
                        }
                        t.events.push(ViewEvent::Canvas(patch.outcome));
                        self.ready(&mut t);
                    }
                    Err(error) => {
                        if !matches!(error.kind, ErrorKind::Cancelled | ErrorKind::Stale) {
                            self.fail(error, &mut t);
                        }
                    }
                }
            }
            Completion::SessionWritten {
                context,
                path,
                epoch,
                revision,
                result,
            } => {
                if self.jobs.records.remove(&context.id).is_none() {
                    return t;
                }
                let Some((active_path, action)) = self.active_saves.remove(&context.id) else {
                    return t;
                };
                if active_path != path {
                    self.fail("Save completion destination mismatch", &mut t);
                    return t;
                }
                match result {
                    Ok(()) => {
                        if epoch == self.basis.project.0
                            && (!matches!(action, SaveAction::Manual)
                                || self.latest_manual_save == Some(context.id))
                        {
                            if revision == self.revision {
                                self.saved_revision = revision;
                            }
                            if matches!(action, SaveAction::Manual) {
                                self.destination = SaveDestination::Writable(path.clone());
                            }
                        }
                        match action {
                            SaveAction::Switch(request) => self.prepare(*request, &mut t),
                            SaveAction::Close => {
                                t.effects.push(Effect::DisposeProject {
                                    epoch: self.basis.project,
                                });
                                t.effects.push(Effect::CloseWindow);
                                t.events.push(ViewEvent::CloseWindow);
                            }
                            SaveAction::Manual => self.ready(&mut t),
                        }
                    }
                    Err(error) => {
                        if matches!(action, SaveAction::Close) {
                            self.closing = false;
                        }
                        self.fail(format!("Session save failed: {error}. Use Save as to choose another location."),&mut t);
                    }
                }
                self.pump_save(&path, &mut t);
            }
            Completion::Cancelled { .. }
            | Completion::Disposed { .. }
            | Completion::WindowClosed => {}
        }
        if !self.jobs.busy() && !self.dragging {
            if let Some(edit) = self.pending_drop.take() {
                self.plan(edit, JobClass::Edit, &mut t);
            } else if let Some((edit, class)) = self.pending_plan.take() {
                self.plan(edit, class, &mut t);
            }
        }
        t
    }
}

#[cfg(test)]
mod save_order_tests {
    use super::*;
    #[test]
    fn older_queued_write_cannot_restore_an_outdated_destination() {
        let mut controller = ApplicationController::new();
        controller.project_state = ProjectState::Open {
            epoch: ProjectEpoch(1),
            options: refscape_model::ResolvedProjectOptions::try_from(
                refscape_model::ProjectOpenOptions {
                    language: refscape_model::ProjectLanguage::Rust,
                    compilation_database: None,
                },
            )
            .unwrap(),
        };
        controller.basis.project = ProjectEpoch(1);
        let mut transition = Transition::default();
        controller
            .save(Some("old.json".into()), SaveAction::Manual, &mut transition)
            .unwrap();
        controller
            .save(Some("new.json".into()), SaveAction::Manual, &mut transition)
            .unwrap();
        for effect in transition.effects.into_iter().rev() {
            let Effect::WriteSession {
                context,
                path,
                snapshot,
            } = effect
            else {
                panic!("expected write")
            };
            controller.complete(Completion::SessionWritten {
                context,
                path,
                epoch: snapshot.epoch,
                revision: snapshot.revision,
                result: Ok(()),
            });
        }
        assert_eq!(
            controller.destination,
            SaveDestination::Writable("new.json".into())
        );
    }
}
