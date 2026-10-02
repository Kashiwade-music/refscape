//! Real tsserver coverage. Install dependencies in examples/typescript-demo first.
mod support;
use refscape_language_typescript::TypeScript;
use refscape_model::{Position, ProjectLanguage, ProjectOpenOptions, Symbol};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

fn demo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/typescript-demo")
        .canonicalize()
        .unwrap()
}
fn backend() -> support::Opened<TypeScript> {
    let executable = std::env::var_os("REFSCAPE_TYPESCRIPT_LANGUAGE_SERVER")
        .map(PathBuf::from)
        .unwrap_or_else(|| demo().join("node_modules/typescript-language-server/lib/cli.mjs"));
    support::Opened::new(TypeScript::new(executable)).with_timeout(Duration::from_secs(45))
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
        .filter(|(_, line)| line.contains(needle))
        .last()
        .unwrap();
    let byte = text.find(needle).unwrap();
    Position::new(line as u32, text[..byte].encode_utf16().count() as u32)
}

#[test]
#[ignore = "requires Node.js and npm install in examples/typescript-demo"]
fn real_server_preserves_namespace_class_context_and_folded_source() {
    let root = demo();
    let mut language = backend();
    language
        .open_project(&root, &ProjectOpenOptions::default())
        .unwrap();
    assert_eq!(
        language.project_options().language,
        ProjectLanguage::TypeScript
    );
    let path = root.join("src/model.ts");
    let symbols = language.symbols(&path).unwrap();
    let method = find(&symbols, "increment");
    let source = language.source(method).unwrap();
    source.validate().unwrap();
    assert_eq!(
        source
            .context
            .iter()
            .map(|context| context.code.as_str())
            .collect::<Vec<_>>(),
        ["export namespace Counters {", "  export class Controller {"]
    );
    assert!(
        source.code.starts_with("    increment(counter: Counter)"),
        "{}",
        source.code
    );
    assert!(
        source
            .folded
            .iter()
            .any(|gap| gap.code.contains("reset(counter:"))
    );
    assert!(!source.tokens.is_empty());
    assert_eq!(source.code_start, Some(Position::new(10, 0)));
    assert!(
        language
            .definitions(&path, position(&path, "Counter {"))
            .unwrap()
            .iter()
            .any(|symbol| symbol.name == "Counter")
    );
    let found = language.search("CounterView").unwrap();
    assert!(
        found
            .iter()
            .any(|symbol| symbol.path.ends_with("src/CounterView.tsx"))
    );
    assert!(
        found
            .iter()
            .any(|symbol| symbol.path.ends_with("mobile/src/CounterView.native.tsx"))
    );
}

#[test]
#[ignore = "requires Node.js and npm install in examples/typescript-demo"]
fn real_server_resolves_react_jsx_props_aliases_hooks_and_native_module_suffixes() {
    let root = demo();
    let mut language = backend();
    language
        .open_project(&root, &ProjectOpenOptions::default())
        .unwrap();
    for (file, target) in [
        ("src/App.tsx", "src/CounterView.tsx"),
        ("src/Legacy.jsx", "src/CounterView.tsx"),
        ("mobile/src/App.tsx", "mobile/src/CounterView.native.tsx"),
        ("mobile/src/Legacy.jsx", "mobile/src/CounterView.native.tsx"),
    ] {
        let path = root.join(file);
        let definitions = language
            .definitions(&path, position(&path, "CounterView counter"))
            .unwrap();
        assert!(
            definitions
                .iter()
                .any(|symbol| symbol.path.ends_with(target) && symbol.name == "CounterView"),
            "{file}: {definitions:?}"
        );
        let symbols = language.symbols(&path).unwrap();
        let source = language.source(symbols.first().unwrap()).unwrap();
        source.validate().unwrap();
        assert!(!source.tokens.is_empty(), "{file}");
        assert!(
            language
                .hover(&path, position(&path, "CounterView counter"))
                .unwrap()
                .is_some()
        );
    }
    let app = root.join("src/App.tsx");
    let hook = language
        .definitions(&app, position(&app, "useState(initialCounter)"))
        .unwrap();
    assert!(
        hook.iter()
            .any(|symbol| symbol.path.ends_with("@types/react/index.d.ts")),
        "{hook:?}"
    );
    let alias = language
        .definitions(&app, position(&app, "initialCounter)"))
        .unwrap();
    assert!(
        alias
            .iter()
            .any(|symbol| symbol.path.ends_with("src/model.ts") && symbol.name == "initialCounter")
    );
    let view = root.join("src/CounterView.tsx");
    let types = language
        .type_definitions(&view, position(&view, "counter.value"))
        .unwrap();
    assert!(
        types.iter().any(|symbol| symbol.name == "Counter"),
        "{types:?}"
    );
    assert!(
        language
            .document_highlights(&view, position(&view, "counter.value"))
            .unwrap()
            .len()
            >= 2
    );
    let references = language
        .references(&view, position(&view, "CounterView({"))
        .unwrap();
    assert!(
        references
            .iter()
            .any(|symbol| symbol.path.ends_with("src/App.tsx"))
    );
    assert!(
        references
            .iter()
            .any(|symbol| symbol.path.ends_with("src/Legacy.jsx"))
    );
    let native = root.join("mobile/src/CounterView.native.tsx");
    let native_types = language
        .definitions(&native, position(&native, "Pressable onPress"))
        .unwrap();
    assert!(
        native_types
            .iter()
            .any(|symbol| symbol.path.to_string_lossy().contains("react-native")),
        "{native_types:?}"
    );
}

#[test]
#[ignore = "requires Node.js and npm install in examples/typescript-demo"]
fn real_server_analyzes_unconfigured_typescript_and_javascript_and_external_edits() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "refscape-ts-inferred-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir(&root).unwrap();
    let model = root.join("model.ts");
    let app = root.join("app.js");
    fs::write(
        &model,
        "export function scale(value: number): number { return value * 2; }\n",
    )
    .unwrap();
    fs::write(
        &app,
        "import { scale } from './model';\nconst label = '猫😀'; export const result = scale(2);\n",
    )
    .unwrap();
    let mut language = backend();
    language
        .open_project(&root, &ProjectOpenOptions::default())
        .unwrap();
    let definitions = language
        .definitions(&app, position(&app, "scale(2)"))
        .unwrap();
    assert!(definitions.iter().any(|symbol| symbol.name == "scale"));
    let symbols = language.symbols(&model).unwrap();
    assert!(
        language
            .source(find(&symbols, "scale"))
            .unwrap()
            .code
            .contains("value * 2")
    );
    fs::write(
        &model,
        "export function scale(value: number): number { return value * 3; }\n",
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
            .starts_with("refscape-ts-inferred-")
    );
    fs::remove_dir_all(root).unwrap();
}
