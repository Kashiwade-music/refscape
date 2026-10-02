# Refscape
A native spatial code explorer for navigating Rust, C/C++, TypeScript/JavaScript (including React and React Native), and Python through references and definitions.

## 概要

Refscape は、コード本文そのものを Infinite Canvas 上に配置して読む Native コードリーディングアプリ。\
IDEのようにファイルやタブを行き来するのではなく、関数・型・実装などをカードとして空間上に展開し、コード同士の関係を視覚的に辿る。

## 基本UIとユーザー体験

プロジェクトを開いたら、ユーザーはファイル一覧やシンボル検索から、最初に配置するカードを選択する。
各カードには実際のソースコードを表示する。シンタックスハイライトも行う。

```rs
fn main() {
    let config = load_config();
    run(config);
}
```

`load_config()` を左クリックして展開すると、その定義が別カードとして近くに追加される。

```plaintext
┌───────────────────────────────────┐
│ fn main() {                       │       ┌────────────────────────┐
│     let config = load_config(); ●─────────>● fn load_config() {    │
│     run(config);                  │       │       ...              │
│ }                                 │       │   }                    │
└───────────────────────────────────┘       └────────────────────────┘
```

現在のコードを置き換えず、新しいコードを空間に追加していく。

crateやmoduleの単位は、上記コードカード包む領域として表現する。\
ファイルパスはコードカード上部に小さく表示する。\
Zoom Levelで表示階層を切り替える。

なお、コードの構造解析はヒューリスティックには行わず、各言語に対応した解析バックエンド（LSP等）を用いて実施する。
PythonではMicrosoftのPyrightを基に、semantic tokensを提供するbasedpyrightを使用する。

## 起動

Windowsを優先したRust/Cargo、C/C++、TypeScript/JavaScript、Pythonプロジェクト向けの実装。Refscape自体のビルドにはRustup、MSVC C++ Build Tools、
Windows SDKが必要。GPUIのプラットフォーム依存については
[固定コミットのGPUI README](https://github.com/zed-industries/zed/blob/f8c2cc844057540ca1eac7de4f19f50d7597dead/crates/gpui/README.md)
を参照。Rustはリポジトリの `rust-toolchain.toml` で `1.98.1` に固定している。

```powershell
# Rustの公式解析バックエンドと標準ライブラリのソースを追加
rustup component add rust-analyzer rust-src

# 付属のRustデモを開く
cargo run --locked -- examples/demo

# Cargo.tomlのあるディレクトリを開く
cargo run --locked -- "C:\code\my-project"

# プロジェクト選択画面から開始
cargo run --locked

# 保存先を指定してセッションを開く／保存する
cargo run --locked -- "C:\code\my-project" --session "reading.json"

# 保存済みセッションのプロジェクトを開く
cargo run --locked -- --session "reading.json"

# Windowsリリース実行ファイルを作成してデモを開く
cargo build --release --locked
.\target\release\refscape.exe examples/demo
```

Rustの解析には `rust-analyzer`、C/C++の解析には `clangd`、TypeScript/JavaScriptには公式の `tsserver`、PythonにはPyrightベースの `basedpyright` を使用し、ソースの構造を正規表現などで推測しない。
`tsserver` とのLSP通信には [typescript-language-server](https://github.com/typescript-language-server/typescript-language-server) を使用する。
初回解析にはプロジェクトの依存取得・インデックス作成が必要になる。
プロジェクトが独自のRust toolchainを指定する場合、そのtoolchainにも
`rust-analyzer` と `rust-src` をインストールする。
実行ファイルは `--rust-analyzer PATH` または `REFSCAPE_RUST_ANALYZER` 環境変数で指定できる。

### C/C++プロジェクトを開く

LLVMの `clangd` をインストールし、PATHに追加する。
実行ファイルは `--clangd PATH` または `REFSCAPE_CLANGD` 環境変数で指定できる。
Open projectでは、ビルドフォルダではなくソースのルートフォルダを選ぶ。
自動判定はCargo、TypeScript/JavaScript、Python、C/C++の順。React NativeのネイティブC/C++ソースがあってもJS/TSを優先する。
言語を明示する場合は `--language rust` / `--language c` / `--language cpp` を使用する。

```powershell
# 既存のC/C++プロジェクト（コンパイル設定を自動検出）
cargo run --locked -- "C:\code\my-cpp-project"

# ソースとは別の場所にあるコンパイル設定を指定
cargo run --locked -- "C:\code\my-cpp-project" --compile-commands "C:\build\debug\compile_commands.json"

# C/C++を明示して、ウィンドウなしで解析とセッション復元を検証
cargo run --locked -- --check "C:\code\my-cpp-project" --language cpp --compile-commands "C:\build\debug"
```

`compile_commands.json` は、各ソースのincludeパス、マクロ、言語規格などのコンパイル条件を持つ。
ルート、`build/`、`build/` 直下の設定別フォルダ、`out/build/` とその直下から候補を探す。
候補が1件なら自動で選び、複数ある場合はBuild settingsで使用するJSONを選択する。
`--compile-commands` ではJSONファイルか、そのファイルがあるディレクトリを指定できる。
選んだ言語と設定のパスはセッションに保存し、次回の復元で再利用する。
CLIで明示した設定は保存済みの設定より優先する。

設定がなくても簡易解析で開けるが、includeやマクロの不足で定義・型の解析が不完全になり得る。
`compile_flags.txt` や `.clangd` もclangd側で利用する。
ルートに `.clangd` がある場合、明示指定がなければdatabaseの選択をclangdに委ね、既存の設定を優先する。
`compile_commands.json` がない場合、プロジェクト全体のバックグラウンド索引には制限がある。
索引作成中の参照検索・シンボル検索は、索引が進むにつれて結果が増える。
ヘッダーはコンパイル設定の一覧にない場合もファイル一覧に表示し、解析条件はclangdに委ねる。
設定の形式とclangdの挙動は [clangdのプロジェクト設定](https://clangd.llvm.org/installation.html#project-setup) を参照。

CMakeではNinjaまたはMakefile系のgeneratorでJSONを生成できる。
Visual Studio generatorは `CMAKE_EXPORT_COMPILE_COMMANDS` に対応していない。
通常は構成・生成の段階でJSONを作れるが、生成ヘッダーが必要なプロジェクトはビルドも必要。
詳細は [CMakeの公式説明](https://cmake.org/cmake/help/latest/variable/CMAKE_EXPORT_COMPILE_COMMANDS.html) を参照。

```powershell
# Ninjaが利用できる環境で付属デモのコンパイル設定を生成
cmake -S examples/cpp-demo -B examples/cpp-demo/build -G Ninja -DCMAKE_EXPORT_COMPILE_COMMANDS=ON
cargo run --locked -- examples/cpp-demo
```

Refscapeはビルドを自動実行しない。既存の開発環境が生成したコンパイル設定を利用する。

### TypeScript・React・React Nativeプロジェクトを開く

Node.jsと `typescript` / `typescript-language-server` をインストールする。
付属デモで使用するtypescript-language-server 6.0.1はNode.js 22.22.2以上が必要。
既存プロジェクトは通常の開発手順で依存をインストールしてから開く。

```powershell
# PATH上のNode.jsを使い、解析サーバーを追加
npm install -g typescript typescript-language-server
cargo run --locked -- "C:\code\my-react-project"

# 付属のTypeScript / React / React Nativeデモを準備して開く
npm ci --prefix examples/typescript-demo --ignore-scripts
npm run check --prefix examples/typescript-demo
cargo run --locked -- examples/typescript-demo

# 言語やサーバーを明示してウィンドウなしで検証
cargo run --locked -- --check examples/typescript-demo --language typescript
cargo run --locked -- "C:\code\my-project" --typescript-language-server "C:\tools\node_modules\typescript-language-server\lib\cli.mjs"
```

`.ts` / `.tsx` / `.mts` / `.cts`（宣言ファイルを含む）と `.js` / `.jsx` / `.mjs` / `.cjs` を扱う。
TSXとJSXはそれぞれ `typescriptreact` / `javascriptreact` として解析し、Reactのコンポーネント、Hooks、props、
React Nativeのコンポーネントにも同じ定義・参照・型・ハイライト・所属宣言の表示を提供する。
`.native.tsx` / `.ios.tsx` / `.android.tsx` などもファイル一覧に含める。
`node_modules`、`dist`、`build`、`.next`、`.expo`、`Pods`などの依存・生成ディレクトリは一覧から除外する。

既存の `tsconfig.json` / `jsconfig.json`（extends、paths、project references、JSX設定など）はtsserverが読み込む。
設定がないフォルダもtsserverのinferred projectとして開く。Refscapeは設定を生成・変更しない。
React Nativeのプラットフォーム別import解決は、プロジェクトの `moduleSuffixes` 設定に従う。
デモのmobile設定は `[".native", ""]` を使用し、ios/androidを選ぶ場合も対象のtsconfigで設定する。
詳細は [TypeScriptのJSX](https://www.typescriptlang.org/docs/handbook/jsx.html) と
[moduleSuffixes](https://www.typescriptlang.org/tsconfig/#moduleSuffixes) を参照。

既定ではプロジェクトまたは親の `node_modules/typescript-language-server/lib/cli.mjs` を探し、次にPATHを使用する。
`--typescript-language-server PATH` または `REFSCAPE_TYPESCRIPT_LANGUAGE_SERVER` で実行ファイル・npmのcmd shim・`lib/cli.mjs` を指定できる。
Windowsのnpm shimは対応するCLIをNode.jsで直接起動する。Node.jsの指定には `REFSCAPE_NODE` を使用する。
`--language typescript` / `ts` / `javascript` / `js` / `react` / `react-native` は共通のJS/TSバックエンドを選択する。
言語はセッションに保存され、既存のRust・C/C++セッションも引き続き復元できる。
別々の設定を持つパッケージも検索できるよう、シンボル検索・参照検索時に各ソースをサーバーで開く。
解析は開発環境にインストール済みの型定義を利用し、自動の型パッケージ取得は行わない。

### Pythonプロジェクトを開く

Pythonの解析には `basedpyright-langserver --stdio` を使用する。
basedpyrightはMicrosoft Pyrightを基にした解析サーバーで、semantic tokensによるシンボル分類を追加している。
Refscapeはサーバーが返す定義・参照・型・所属宣言を使用し、Pythonソースから宣言を推測しない。
インストール方法とsemantic tokensの詳細は [basedpyrightのインストール](https://docs.basedpyright.com/latest/installation/command-line-and-language-server/) と
[semantic highlighting](https://docs.basedpyright.com/latest/benefits-over-pyright/pylance-features/#semantic-highlighting) を参照。

```powershell
# Pythonの開発環境に解析サーバーを追加
pip install basedpyright
basedpyright --project examples/python-demo
cargo run --locked -- examples/python-demo

# npmを使用する場合
npm install -g basedpyright

# 言語を明示し、ウィンドウなしで解析とセッション復元を検証
cargo run --locked -- --check examples/python-demo --language python

# プロジェクトの仮想環境にあるサーバーを指定
cargo run --locked -- "C:\code\my-python-project" --pyright "C:\code\my-python-project\.venv\Scripts\basedpyright-langserver.exe"
```

`.py` と型スタブの `.pyi` を扱う。class・method・関数・ネストした関数をカードとして表示し、
所属するclassや関数の宣言も元の行番号・インデントで表示する。
`.venv` / `venv` などの仮想環境、`site-packages`、`__pycache__`、`.pytest_cache`、`build` / `dist` などはファイル一覧から除外する。
変数・引数・フィールドのクリックでは同じシンボルの使用箇所をハイライトし、型定義を展開する。
定義・参照検索、ホバー、シンボル検索と共通のCanvas配置・セッション保存／復元も利用できる。
付属デモは追加のPythonパッケージを必要とせず、`main.py` から `load_config` / `run` のファイル間呼び出し、
`Counter.increment` の所属class、`summarize` 内の `annotate` の所属関数、`counter: Counter` の型移動を確認できる。
実行する場合は `python examples/python-demo/main.py` を使用する（Python 3.10以上）。

`pyrightconfig.json` と `pyproject.toml` の `[tool.basedpyright]` / `[tool.pyright]` 設定を解析サーバーが読み込む。
`pythonVersion`、`extraPaths`、`stubPath`、`executionEnvironments` などの既存設定を利用し、Refscapeは設定を生成・変更しない。
basedpyrightはプロジェクト直下の `.venv` にあるPythonを使用し、なければPATH上のPythonを使用する。
別の仮想環境は既存設定の `venvPath` と `venv` で指定できる。
依存パッケージは既存の開発手順で対象の環境にインストールしてから開く。
設定の優先順位・パス解決は [basedpyrightの設定](https://docs.basedpyright.com/latest/configuration/config-files/) と
[Python環境・import解決](https://docs.basedpyright.com/latest/usage/import-resolution/#configuring-your-python-environment) を参照。

`--language python` / `py` はPythonバックエンドを選択する。
`--pyright PATH` または `REFSCAPE_PYRIGHT` でサーバーの実行ファイル・npmのcmd shim・`langserver.index.js` を指定できる。
既定ではプロジェクトまたは親の `.venv` / `venv` と `node_modules/basedpyright` を探し、次にPATH上のサーバーを使用する。
npmのサーバーはNode.jsで起動し、Node.jsの指定には `REFSCAPE_NODE` を使用する。
Microsoftの `pyright-langserver` を明示指定することもできるが、semantic tokensを提供しないため、
シンタックスハイライトと変数・関数のクリック動作の分類には制限がある。
この場合もAlt + 左クリックによる定義移動、参照検索、ホバーを利用できる。
Pythonの言語選択もセッションに保存し、復元時に再利用する。

## 操作

左のファイル一覧からファイルカードを開くか、検索欄にシンボル名を入力してEnterで検索し、
結果をクリックしてカードを配置する。変数・引数・フィールドをクリックすると、同じ値の宣言・使用箇所を
ハイライトし、その型定義を右側のカードとして展開する。ハイライトは同じファイルを表示する全カードに反映し、
同名でも別の変数は含めない。型カードのタイトルには `config → Config` のように関係を表示し、
左側には解析バックエンドが返す型情報を表示する。プリミティブ型など定義位置がない場合もハイライト・型情報は表示する。
関数・型名などのクリックは定義を展開する。Alt + 左クリックで変数の定義元を明示的に展開できる。
検索結果のシンボル、またはコード上の同じ単語をもう一度クリックすると、
対応するカードとその接続線を非表示にする。通常の追加・削除では残ったカードの位置を保つ。
さらにクリックすると再表示する。
カードを閉じると、そのカードから展開した子孫も閉じる。他のカードから接続されている子孫は残す。
新しい子カードは親の右辺から100以上離れた右側で、クリックした単語の右端・表示行の上端に
左上が最も近くなる空き位置へ配置する。上下とさらに右の空きを比較し、既存の兄弟・子孫は動かさない。
別の親から既存カードを参照した場合も位置を保ち、接続を追加する。複数の結果はパス・ソース範囲・ID順に配置する。
配置はworld座標で計算し、パン・ズームには依存しない。カード本体の間に74の余白を保ち、
ファイル・crate・projectの背景領域は障害物にしない。負座標や画面外への配置も許し、自動Fitは行わない。
関数などのカードには、解析バックエンドが返す所属先の `impl`・`mod`・class・namespace などの宣言も、
元の行番号とインデントで表示する。宣言と本文の間の省略箇所は `... (Show NN Lines)` で示す。
所属宣言のシンボルも本文と同じようにクリックして定義・参照・型カードを展開でき、ホバーで説明を表示する。
省略行の左端にある小さな矢印、または `... (Show NN Lines)` 行を左クリックすると、省略区間を同じカード内に展開する。
展開区間の先頭行の左端にある上向き矢印で再び閉じる。開閉ボタン・行番号・コードは別々の列とし、
行番号は右揃えで表示する。展開・折りたたみで列の位置は変えず、操作のポップアップは表示しない。
展開されたコードもクリックでき、
カードの左上を固定して幅・高さを更新し、新たに衝突するカードだけを元の位置に最も近い空きへ移す。
衝突していないカードは動かさず、縮小時も空きを詰めない。接続線は新しい表示行に追従する。
省略区間のソースと展開状態もセッションに保存する。
この配置規則と所属先の宣言表示は、Rust・C/C++・TypeScript/JavaScript・Pythonおよび今後追加するすべての言語に共通の必須仕様とする。
言語ごとのアダプターは解析バックエンドから所属情報を提供し、共通のモデル・配置・描画を利用する。
コードの単語に約400msホバーすると、解析バックエンドが返す型・シグネチャ・ドキュメントを表示する。
単語から説明へマウスを移してスクロールでき、単語と説明の両方から離れると少し待って閉じる。Escではすぐに閉じる。
接続元のコード単語に下線を引き、その下線の右端から定義・参照の矢印を伸ばす。
下線と接続位置はパン・ズームやカード移動に追従する。コードを省略するズーム階層では接続線も省略する。

| 操作 | 動作 |
|---|---|
| 変数・引数・フィールドを左クリック | 同じ変数をハイライトし、型定義を展開 |
| 関数・型名などを左クリック | 定義を展開 |
| Alt + 左クリック | 定義元を展開 |
| コードを右クリック | 参照元を展開 |
| コードにホバー | 型・シグネチャ・ドキュメントを表示 |
| Esc／空のCanvasをクリック | 変数のハイライトを解除 |
| カードのヘッダーをドラッグ | 希望位置をプレビューし、ドロップで対象だけを最も近い空き位置へ移動 |
| 空のCanvasをドラッグ／ホイール | パン |
| Shift + ホイール | 横方向にパン |
| Ctrl + ホイール／`+`・`-` | 拡大・縮小 |
| `0`／Fit | 全カードを画面に収める |
| Arrange | 選択カード（未選択なら保存順の先頭）を根として、子孫を登場順の木に配置する |
| Undo layout | 直前のArrangeによる位置変更を戻す |
| Ctrl + F／Ctrl + P | 検索欄にフォーカス |
| Ctrl + S | 現在のセッションを保存 |
| Ctrl + Shift + S | 名前を付けて保存 |
| Ctrl + O | 保存済みセッションを開く |
| Ctrl + Shift + O | プロジェクトを選択 |
| カードの `×`／選択してDelete | カードと展開した子孫、その接続を削除 |
| Themeボタン | テーマを切り替える |

65%以上のズームではソースコード、35～65%ではカードの概要、35%未満では領域を表示する。
Rustの領域はCargo metadataに基づくworkspaceのcrate（Cargo package単位）と、
ソースファイルごとにカードを包む。C/C++、TypeScript/JavaScript、Pythonはプロジェクトとファイルの領域を表示する。
解析中もCanvasを移動でき、解析・保存のエラーは下部のステータス欄に表示する。

Arrangeは根カードの位置を固定し、そこから接続を辿れる子孫だけを配置する。
配置用の木は、接続元シンボルのソース行・列順に幅優先で辿り、共有カードに最初に到達した親を採用する。
同じ階層では親の枝の上下順を引き継ぎ、各親の子カードはシンボルの登場順に上から下へ並べる。
同じ位置から複数の対象がある場合は、対象のパス・ソース範囲・シンボルID・カードIDで順序を固定する。
実際のカード幅と表示高さから枝の領域を確保し、子は配置上の親の右辺から100px以上右に置く。
木に採用しなかった共有参照・循環・自己参照の接続も残し、確定した位置から描画する。
これらの追加の線は左向きになる場合がある。
子孫以外のカードは固定した障害物として扱い、衝突するときは子孫全体を同じ量だけ上下へずらして順序を保つ。
配置後の広さによる採否判定は行わない。追加・削除後もArrangeを押すまで既存カードの再配置は行わない。
一回分のUndo layoutは位置だけを復元し、追加・削除・寸法変更・ドラッグで無効になる。
ArrangeもUndoもパン・ズームを変えない。

## セッションとテーマ

既定の保存先はプロジェクト内の `.refscape/session.json`。起動時に保存済みセッションを復元し、
通常のウィンドウ終了時や別のプロジェクト・セッションへ切り替える前にも保存する。
Ctrl + SやSaveボタンで途中保存できる。
コードカードの本文・位置・接続・領域・パン・ズーム・テーマをJSONで保存する。
既存のバージョン1セッションも読める。廃止した配置設定は無視し、再保存時には書き出さない。
有効な保存位置を保持し、重複がある場合のみ最小限に修復する。
配置計画、世代、ドラッグプレビューとUndo履歴は保存しない。
保存は同じディレクトリの一時ファイルへの書き込み後に置き換え、
不正なデータや未対応の保存形式バージョンはエラーとして扱う。
自動復元に失敗した既存ファイルは終了時に上書きせず、Save asで別の保存先を選べる。
セッションはソースのスナップショットとファイル内容の識別情報を含む。
復元時に現在のファイルと比較し、変更されたカードだけ最新の本文へ更新して折りたたみ状態をリセットする。
開いているファイルは約1秒間隔でバックグラウンド確認し、外部で保存された変更も同じ方法で反映する。
カードの位置・パン・ズームは保持し、サイズ変更で重複した場合のみ配置を修復する。
更新されたカードの古い位置を指す接続線とホバーは破棄する。削除されたファイル・シンボルのカードも取り除く。

ライトとダークの組み込みテーマに加え、カスタムテーマを読み込める。

```powershell
# 既定テーマを雛形として出力
cargo run --locked -- --export-theme dark my-theme.json

# 編集したテーマを読み込む
cargo run --locked -- "C:\code\my-project" --theme my-theme.json
```

テーマJSONは `version` と `theme` を持つ。`theme.name` と `theme.palette` の
12色を編集して配布でき、各色は `#RRGGBB` 形式を使用する。
セッション内には選択したテーマの内容も保存する。

## 対応範囲

- マルチプラットフォーム（Windows/Linux/macOS）
  - まずはWindowsから
- 多言語対応（Rust/C/C++/TypeScript/JavaScript/React/React Native/Python）
  - その他の言語は今後追加

コード編集機能は持たない。言語解析はRustの `rust-analyzer`、C/C++の `clangd`、TypeScript/JavaScriptの `tsserver`、Pythonの `basedpyright` が対象。
ReactとReact NativeのTSX/JSXコードも対象。Windows以外のプラットフォームとその他の言語は将来の対応範囲。
GPUIが使う描画バックエンドに対応したGPUドライバーが必要。
マクロ展開の仮想URIなど、実ファイルに対応しないソースへの移動は未対応で、
解析バックエンドのエラーを画面に表示する。

## 依存ライブラリ

- 描画はGPUIを使用。crates.io版ではなく、確認時点のZedの最新Gitコミットに固定する。

## 開発

製品は14 crateのCargo workspaceで構成し、開発用の `xtask` は独立したworkspaceとする。
共通LSP処理とCanvas配置を独立させ、Rust、C/C++、TypeScript/JavaScript、Pythonの解析はそれぞれの言語crateが実装する。
今後の言語も専用crateを追加し、選択層へ登録する。言語crate同士の依存と、全種類の内部依存の循環を禁止する。
各crateの責務・依存方向・検査ルールは [アーキテクチャ](docs/architecture.md) を参照。
1ソースファイルは空行・コメント・文字列を除いて `max_file_lines`（1000行）以下とし、子を持つRustモジュールは `<name>.rs` と `<name>/` の組で配置する。`mod.rs` は使わない。
`cargo xtask gate` で検査し、既存の違反も失敗として報告する。
現在の依存宣言は [依存グラフ](docs/dependency-graph.md) にMermaidで出力する。

Rust toolchainは `rust-toolchain.toml` に固定している。リポジトリのルートで次を実行する。

```sh
# 構造検査、生成依存図のcheck、format、clippy、test
cargo xtask gate

# manifest変更後の依存図更新
cargo xtask graph

# 起動
cargo run --locked

# 実際のrust-analyzerで定義・参照・ハイライトを検証
cargo test -p refscape-language-rust --locked --test rust_analyzer -- --ignored

# 実プロジェクトのカード展開・重複防止・名前付きセッション復元を検証
cargo test -p refscape-app --locked --test workflow -- --ignored

# 実際のclangdでC/C++の解析とセッション復元を検証
cargo test -p refscape-language-cpp --locked --test clangd -- --ignored
cargo test -p refscape-app --locked --test cpp_workflow -- --ignored

# TypeScript / React / React Nativeの実解析・Canvas操作・セッション復元
# 先に上記の付属デモ依存をインストールする
cargo test -p refscape-language-typescript --locked --test typescript -- --ignored
cargo test -p refscape-app --locked --test typescript_workflow -- --ignored

# basedpyrightでPythonの実解析・Canvas操作・セッション復元
cargo test -p refscape-language-python --locked --test python -- --ignored
cargo test -p refscape-app --locked --test python_workflow -- --ignored

# 全言語間の切り替えと失敗時のプロジェクト保持
cargo test -p refscape-language --locked --test backends -- --ignored

# ウィンドウを開かず、解析要求・Canvas操作・セッション復元を検証
cargo run --locked -- --check "C:\code\my-project"
```

`cargo xtask graph` が `docs/dependency-graph.md` を更新し、`cargo xtask gate` はその一致を検査する。
実サーバーのテストは外部の `rust-analyzer` / `clangd` / `typescript-language-server` / `basedpyright-langserver` が必要なため通常はignoreされる。

実GPUによる描画確認用のexampleも用意する。`main` を持つプロジェクトで、
ライト／ダークのPNGを出力できる。GPUとデスクトップ環境が必要。

```powershell
cargo run -p refscape-app --locked --example render --features visual-tests -- examples/demo target/refscape-dark.png
cargo run -p refscape-app --locked --example render --features visual-tests -- examples/demo target/refscape-light.png light
cargo run -p refscape-app --locked --example render --features visual-tests -- examples/demo target/refscape-tree.png arrange
```
