use super::*;
use refscape_application::test_support::*;
use refscape_model::{OperationContext, ProjectCrate, ResolvedProjectOptions};

#[derive(Clone)]
struct LiveLanguage(Language);
impl AnalysisFactory for LiveLanguage {
    fn prepare(
        &self,
        root: &Path,
        options: &ProjectOpenOptions,
        context: &OperationContext,
    ) -> AnalysisResult<PreparedProject> {
        let mut prepared = self.0.prepare(root, options, context)?;
        prepared.session = Box::new(self.clone());
        Ok(prepared)
    }
}
impl AnalysisSession for LiveLanguage {
    fn project_options(&self) -> ResolvedProjectOptions {
        self.0.project_options()
    }
    fn files(&mut self, context: &OperationContext) -> AnalysisResult<Vec<PathBuf>> {
        self.0.files(context)
    }
    fn symbols(&mut self, path: &Path, context: &OperationContext) -> AnalysisResult<Vec<Symbol>> {
        self.0.symbols(path, context)
    }
    fn source(
        &mut self,
        symbol: &Symbol,
        context: &OperationContext,
    ) -> AnalysisResult<SourceDocument> {
        context.check()?;
        let mut source = self.0.source.clone();
        source.symbol = symbol.clone();
        source.code = std::fs::read_to_string(&symbol.path).unwrap();
        Ok(source)
    }
    fn definitions(
        &mut self,
        path: &Path,
        position: Position,
        context: &OperationContext,
    ) -> AnalysisResult<Vec<NavigationTarget>> {
        self.0.definitions(path, position, context)
    }
    fn references(
        &mut self,
        path: &Path,
        position: Position,
        context: &OperationContext,
    ) -> AnalysisResult<Vec<NavigationTarget>> {
        self.0.references(path, position, context)
    }
    fn project_crates(&mut self, context: &OperationContext) -> AnalysisResult<Vec<ProjectCrate>> {
        self.0.project_crates(context)
    }
    fn search(&mut self, query: &str, context: &OperationContext) -> AnalysisResult<Vec<Symbol>> {
        self.0.search(query, context)
    }
}
struct LiveFile(PathBuf);
impl Drop for LiveFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
#[gpui::test]
fn periodic_disk_changes_refresh_visible_code_without_resetting_selection_or_viewport(
    cx: &mut TestAppContext,
) {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let file = LiveFile(std::env::temp_dir().join(format!("refscape-reload-ui-{unique}.rs")));
    let old = "fn live() {}";
    std::fs::write(&file.0, old).unwrap();
    let source = SourceDocument {
        symbol: Symbol::file(
            file.0.clone(),
            SourceRange {
                start: Position::default(),
                end: Position::new(0, old.len() as u32),
            },
        ),
        code: old.into(),
        code_start: None,
        context: vec![],
        folded: vec![],
        expanded: vec![],
        tokens: vec![],
    };
    let mut fixture = FixtureDriver::new(
        LiveLanguage(Language {
            source: source.clone(),
            requests: Default::default(),
            targets: vec![],
            type_targets: vec![],
            type_requests: Default::default(),
            highlights: vec![],
        }),
        Repository,
    );
    fixture
        .open_project(Path::new("."), &ProjectOpenOptions::default())
        .unwrap();
    let id = fixture
        .add_symbol(source.symbol, Point::new(100.0, 50.0))
        .unwrap();
    fixture.driver.dispatch(Command::RefreshSources);
    fixture.driver.dispatch(Command::Zoom {
        factor: 1.25,
        anchor: Point::new(200.0, 150.0),
    });
    let before = fixture.snapshot().clone();
    let (view, cx) = cx.add_window_view(|window, cx| {
        ExplorerView::from_fixture(
            fixture,
            "session.json".into(),
            vec![],
            None,
            ProjectOpenOptions::default(),
            window,
            cx,
        )
    });
    cx.run_until_parked();
    view.update(cx, |view, _| {
        view.canvas.selected = Some(id.clone());
        view.hover.text = Some("old hover".into());
    });
    let updated = "fn live() {\n    changed();\n}";
    std::fs::write(&file.0, updated).unwrap();
    cx.background_executor
        .advance_clock(std::time::Duration::from_secs(1));
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        let card = &view.controller.snapshot().cards[0];
        assert_eq!(card.source.code.as_ref(), updated);
        assert_eq!(card.position, before.cards[0].position);
        assert_eq!(view.controller.snapshot().viewport, before.viewport);
        assert_eq!(view.canvas.selected.as_deref(), Some(id.as_str()));
        assert!(view.hover.text.is_none());
    });
    let handle = cx.window_handle();
    cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    view.read_with(cx, |view, _| {
        assert!(
            view.canvas.painted[0]
                .rows
                .iter()
                .any(|row| row.code.text.contains("changed"))
        );
    });
}
