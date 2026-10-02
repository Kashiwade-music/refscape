//! Real basedpyright coverage. Set REFSCAPE_PYRIGHT or install basedpyright first.
mod support;
use refscape_language_python::Python;
use refscape_model::{Position, ProjectLanguage, ProjectOpenOptions, Symbol};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

fn demo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/python-demo")
        .canonicalize()
        .unwrap()
}

fn backend() -> support::Opened<Python> {
    support::Opened::new(Python::default()).with_timeout(Duration::from_secs(45))
}

fn find<'a>(symbols: &'a [Symbol], name: &str) -> &'a Symbol {
    fn search<'a>(symbols: &'a [Symbol], name: &str) -> Option<&'a Symbol> {
        symbols.iter().find_map(|symbol| {
            if symbol.name == name {
                Some(symbol)
            } else {
                search(&symbol.children, name)
            }
        })
    }
    search(symbols, name).unwrap_or_else(|| panic!("missing symbol {name}: {symbols:?}"))
}

fn position(path: &Path, needle: &str) -> Position {
    let text = fs::read_to_string(path).unwrap();
    let (line, text) = text
        .lines()
        .enumerate()
        .filter(|(_, text)| text.contains(needle))
        .last()
        .unwrap();
    let byte = text.find(needle).unwrap();
    Position::new(line as u32, text[..byte].encode_utf16().count() as u32)
}

#[test]
#[ignore = "requires basedpyright-langserver or REFSCAPE_PYRIGHT"]
fn real_server_provides_semantic_variables_types_and_nested_context() {
    let root = demo();
    let mut language = backend();
    language
        .open_project(&root, &ProjectOpenOptions::default())
        .unwrap();
    assert_eq!(language.project_options().language, ProjectLanguage::Python);
    let model = root.join("model.py");
    let pipeline = root.join("pipeline.py");
    let symbols = language.symbols(&model).unwrap();
    let increment = language.source(find(&symbols, "increment")).unwrap();
    increment.validate().unwrap();
    assert_eq!(
        increment
            .context
            .iter()
            .map(|context| context.code.as_str())
            .collect::<Vec<_>>(),
        ["class Counter:"]
    );
    assert!(
        increment
            .folded
            .iter()
            .any(|gap| gap.code.contains("def __init__"))
    );
    assert!(increment.code.starts_with("    def increment("));
    assert!(!increment.tokens.is_empty());
    let symbols = language.symbols(&pipeline).unwrap();
    let annotate = language.source(find(&symbols, "annotate")).unwrap();
    annotate.validate().unwrap();
    assert_eq!(
        annotate
            .context
            .iter()
            .map(|context| context.code.as_str())
            .collect::<Vec<_>>(),
        ["def summarize(counter: Counter, config: Config) -> str:"]
    );
    let run = language.source(find(&symbols, "run")).unwrap();
    run.validate().unwrap();
    let counter = position(&pipeline, "counter, config)");
    assert!(
        refscape_model::CardSource::try_from(run.clone())
            .unwrap()
            .variable_token(counter)
            .is_some(),
        "{:?}",
        run.tokens
    );
    assert!(
        language
            .type_definitions(&pipeline, counter)
            .unwrap()
            .iter()
            .any(|symbol| symbol.name == "Counter" && symbol.path.ends_with("model.py"))
    );
    assert!(
        language
            .document_highlights(&pipeline, counter)
            .unwrap()
            .len()
            >= 2
    );
    assert!(language.hover(&pipeline, counter).unwrap().is_some());
}

#[test]
#[ignore = "requires basedpyright-langserver or REFSCAPE_PYRIGHT"]
fn real_server_searches_and_finds_callers_in_previously_unopened_files() {
    let root = demo();
    let model = root.join("model.py");
    let main = root.join("main.py");
    let mut language = backend();
    language
        .open_project(&root, &ProjectOpenOptions::default())
        .unwrap();
    let references = language
        .references(&model, position(&model, "load_config()"))
        .unwrap();
    assert!(
        references
            .iter()
            .any(|symbol| symbol.path.ends_with("main.py")),
        "{references:?}"
    );
    assert!(
        references
            .iter()
            .any(|symbol| symbol.path.ends_with("pipeline.py")),
        "{references:?}"
    );
    assert!(
        language
            .definitions(&main, position(&main, "load_config()"))
            .unwrap()
            .iter()
            .any(|symbol| symbol.name == "load_config" && symbol.path.ends_with("model.py"))
    );
    let found = language.search("ANNOTATE").unwrap();
    assert!(
        found
            .iter()
            .any(|symbol| symbol.name == "annotate" && symbol.path.ends_with("pipeline.py")),
        "{found:?}"
    );
    let mut identifiers = std::collections::BTreeSet::new();
    assert!(found.iter().all(|symbol| identifiers.insert(&symbol.id)));
    let all = language.search("").unwrap();
    assert!(all.iter().any(|symbol| symbol.name == "Counter"));
    assert!(all.iter().any(|symbol| symbol.name == "annotate"));
}

#[test]
#[ignore = "requires basedpyright-langserver or REFSCAPE_PYRIGHT"]
fn real_server_analyzes_unconfigured_python_stubs_unicode_and_external_edits() {
    let root = std::env::temp_dir().join(format!(
        "refscape-python-inferred-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    let model = root.join("model.py");
    let app = root.join("app.py");
    let stub = root.join("contract.pyi");
    fs::write(
        &model,
        "def scale(value: int) -> int:\n    return value * 2\n",
    )
    .unwrap();
    fs::write(&app, "from model import scale\nfrom contract import describe\nlabel = '猫😀'; result = scale(2)\nmessage = describe(result)\n").unwrap();
    fs::write(&stub, "def describe(value: int) -> str: ...\n").unwrap();
    let mut language = backend();
    language
        .open_project(&root, &ProjectOpenOptions::default())
        .unwrap();
    assert_eq!(language.files().unwrap().len(), 3);
    assert!(
        language
            .definitions(&app, position(&app, "scale(2)"))
            .unwrap()
            .iter()
            .any(|symbol| symbol.name == "scale")
    );
    assert!(
        language
            .definitions(&app, position(&app, "describe(result)"))
            .unwrap()
            .iter()
            .any(|symbol| symbol.path.ends_with("contract.pyi"))
    );
    let symbols = language.symbols(&model).unwrap();
    let source = language.source(find(&symbols, "scale")).unwrap();
    source.validate().unwrap();
    assert!(source.code.contains("value * 2"));
    assert!(!source.tokens.is_empty());
    fs::write(
        &model,
        "def scale(value: int) -> int:\n    return value * 3\n",
    )
    .unwrap();
    let symbols = language.symbols(&model).unwrap();
    assert!(
        language
            .source(find(&symbols, "scale"))
            .unwrap()
            .code
            .contains("value * 3")
    );
    drop(language);
    assert!(
        root.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("refscape-python-inferred-")
    );
    fs::remove_dir_all(root).unwrap();
}
