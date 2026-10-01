use super::*;

struct Language {
    target: Symbol,
    fail: bool,
    code: String,
    additional: Vec<Symbol>,
    crates: Vec<ProjectCrate>,
    options: ProjectOptions,
    open_count: usize,
}
impl LanguageService for Language {
    fn open_project(&mut self, _: &Path, options: &ProjectOptions) -> Result<()> {
        self.options = options.clone();
        self.open_count += 1;
        Ok(())
    }
    fn project_options(&self) -> ProjectOptions {
        self.options.clone()
    }
    fn files(&mut self) -> Result<Vec<PathBuf>> {
        Ok(vec![self.target.path.clone()])
    }
    fn symbols(&mut self, _: &Path) -> Result<Vec<Symbol>> {
        Ok(vec![self.target.clone()])
    }
    fn project_crates(&mut self) -> Result<Vec<ProjectCrate>> {
        Ok(self.crates.clone())
    }
    fn source(&mut self, symbol: &Symbol) -> Result<SourceDocument> {
        if self.fail {
            return Err("backend unavailable".into());
        }
        Ok(SourceDocument {
            expanded: Vec::new(),
            folded: Vec::new(),
            context: Vec::new(),
            code_start: None,
            symbol: symbol.clone(),
            code: self.code.clone(),
            tokens: Vec::new(),
        })
    }
    fn definitions(&mut self, _: &Path, _: Position) -> Result<Vec<Symbol>> {
        let mut targets = vec![self.target.clone(), self.target.clone()];
        targets.extend(self.additional.clone());
        Ok(targets)
    }
    fn references(&mut self, _: &Path, _: Position) -> Result<Vec<Symbol>> {
        Ok(vec![self.target.clone()])
    }
}
struct Repository;
impl SessionRepository for Repository {
    fn save(&self, _: &Path, _: &Session) -> Result<()> {
        Ok(())
    }
    fn load(&self, _: &Path) -> Result<Session> {
        Err("no session".into())
    }
}
fn symbol(name: &str) -> Symbol {
    Symbol {
        id: name.into(),
        name: name.into(),
        kind: "function".into(),
        path: PathBuf::from(format!("/project/{name}.rs")),
        range: SourceRange {
            start: Position::default(),
            end: Position::new(0, 14),
        },
        selection_range: SourceRange {
            start: Position::new(0, 3),
            end: Position::new(0, 9),
        },
        children: Vec::new(),
    }
}
fn explorer() -> Explorer<Language, Repository> {
    let mut explorer = Explorer::new(
        Language {
            target: symbol("target"),
            fail: false,
            code: "fn target() {}".into(),
            additional: Vec::new(),
            crates: Vec::new(),
            options: ProjectOptions::default(),
            open_count: 0,
        },
        Repository,
    );
    explorer
        .open_project(
            &std::env::current_dir().unwrap(),
            &ProjectOptions::default(),
        )
        .unwrap();
    explorer
}

mod layout;
mod navigation;
mod project;
mod regions;
