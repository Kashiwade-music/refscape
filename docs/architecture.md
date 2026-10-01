# Crate構成と依存ゲート

## 責務

| crate | 責務 | 許可する内部依存 |
|---|---|---|
| `refscape-model` | UI非依存のソース位置・シンボル・カード・接続・領域・座標・ズーム・セッション・テーマのモデルと不変条件 | なし |
| `refscape-canvas` | カード寸法、衝突回避、列の圧縮、接続元の配置基準、領域構築 | `model` |
| `refscape-application` | プロジェクト開始、定義展開、カード操作、セッション保存・復元などの操作と外部機能のtrait | `canvas`, `model` |
| `refscape-lsp` | JSON-RPC通信、文書状態とキャッシュ、共通LSP要求、応答と独自モデルの変換 | `model` |
| `refscape-language-rust` | Cargoプロジェクト情報、Rustファイル検出、rust-analyzerの起動設定・固有通知、言語サービスの実装 | `application`, `lsp`, `model` |
| `refscape-language-cpp` | C/C++ファイル検出、compilation database、clangdの起動設定、言語サービスの実装 | `application`, `lsp`, `model` |
| `refscape-language` | プロジェクトの言語選択、選択した言語サービスへの委譲 | `application`, `language-rust`, `language-cpp`, `model` |
| `refscape-storage` | セッション・設定・テーマの保存形式、読み書き、バージョン検査と原子的書き込み | `application`, `model` |
| `refscape-ui` | GPUIによる描画・入力、背景処理と結果反映、一時的な表示状態、ネイティブウィンドウの起動・終了 | `application`, `canvas`, `model` |
| `refscape-app` | 実行バイナリ `refscape`、CLIと具体的なアダプターの生成・接続 | `application`, `language`, `model`, `storage`, `ui` |
| `xtask` | 構造検査、依存グラフの生成、開発用ゲート | 製品への依存なし |

表の依存先は `refscape-` を省略したもの。許可する依存先の上限を示す。
製品は10 crateでRust/C/C++プロジェクトの解析、Canvas操作、バージョン付き永続化、GPUIの画面を実装する。

`application::ports` が `LanguageService` や `SessionRepository` といったtraitを定義し、
言語ごとのcrateと `storage` がそれを実装する。`language` は言語選択とサービスへの委譲を行い、
`app` が選択サービスと保存の実装を生成して `application::explorer::Explorer` に渡す。
`application` から具体的なアダプターへ依存しない。

GPUIの型・直接依存は `ui` に閉じ込める。`app` はUI非依存の引数を `refscape_ui::runtime::run` に渡して起動する。
共通LSP応答は `lsp` の境界で独自モデルに変換し、サーバー固有の設定や通知は各言語crateが扱う。
LSPは言語バックエンドの一つとし、将来はコンパイラAPIなどの公式機能を使うバックエンドも追加できる。
言語crate同士の依存、および言語crateから選択層 `language` への依存は認めない。
`lsp` は具体的な言語や `LanguageService` を知らず、`canvas` は `Explorer` やGPUIの型を受け取らない。
Canvasの状態は `model`、配置規則は `canvas`、ファイルの保存形式は `storage` が管理する。
テーマの意味的な定義は `model`、保存は `storage`、描画用の型への変換は `ui` が担当する。
コードの構造解析はREADMEの方針に従いLSP等の公式機能を利用し、未対応の機能を推測で補わない。

## 言語を追加する手順

1. `crates/refscape-language-<name>` を作り、ルートworkspaceと `[workspace.dependencies]` に登録する。
2. `application::ports::LanguageService` を実装し、プロジェクトの検出・設定検証・解析プロセスの生成をそのcrateに置く。
3. LSPを使う場合は `lsp` の共通セッションを利用し、サーバー固有の初期化設定・通知を言語側で定義する。
4. `language` の選択処理に新しい候補を登録し、必要な言語設定を `model` に追加する。混在プロジェクトの優先順位と明示指定も選択層で決める。
5. `xtask/src/architecture.rs` の `POLICY` に新crateと選択層からの依存を登録し、構造検査のfixtureとこの責務表を更新する。
6. 新crateに言語固有の単体・実サーバーテストを追加し、`app` から選択層を通した操作も検証する。`cargo xtask gate` で依存図を生成して確認する。

共通LSPを利用しない公式コンパイラAPIの実装も同じ言語サービスの境界で接続できる。
下位crateのテストから選択層や `app` へ依存しない。結合テストは `app/tests` に置き、
言語間で共有する通信・応答変換だけを `lsp` に置く。旧APIへの再エクスポートや引数変換の互換ラッパーは設けない。

## データと操作の境界

ソース位置はLSPと同じ0始まりの行・UTF-16列で保持し、UTF-8本文の切り出し時に変換する。
シンボルの範囲とsemantic tokenは元のファイルに対する絶対位置を持つため、
カード上のクリックは抜粋の開始位置を加えて定義・参照要求に変換する。
カード本文にはファイルやシンボルの実際のソースを保存する。

`Explorer` は定義・参照の展開、同じシンボルの重複防止、接続の追加、
カードの移動・削除、カーソルを基準にしたズームを扱う。
`canvas` は新しいカードの寸法をソース全体を読める大きさにし、既存カードとの衝突を避けて配置する。
カードの非表示・削除後は、カード左端の位置から残った列を判定し、列と列内の読み順を保って配置を再計算する。
列内の最大幅に合わせて横方向の空きを詰め、幅の広いカードによって隣の列が同じ列にまとめられることを防ぐ。
縦方向の間隔には、描画と共有するファイル領域の見出しと余白を含める。
複数カードを同時に閉じる操作は一括で再配置する。パン・ズームは保持する。
閉じたカードから辿れる子孫も削除するが、他の枝から繋がる子孫は保持する。循環は訪問済み集合で処理する。
展開・再配置では接続元の絶対行からカード内のコード行の高さを求め、子カードの配置基準にする。
crate領域にはCargo metadataのworkspaceパッケージと所属ディレクトリを用い、
ファイル領域と合わせて表示する。workspace外のソースとC/C++のソースはプロジェクト領域にまとめる。
セッション保存前にはモデルの不変条件を検証する。
ソースは保存時点のスナップショットであり、読み込み時に内容を推測で再構築しない。

`LanguageBackend` はプロジェクトの種類と `ProjectOptions` に基づいてRust/C/C++の解析を選ぶ。
`RustAnalyzer` と `Clangd` はプロジェクトごとにLSPプロセスを起動し、document symbol、workspace symbol、
definition、typeDefinition、references、documentHighlight、semantic tokenを独自モデルへ変換する。
変数・引数・フィールドはsemantic tokenで判別し、クリック時には型定義を展開する。
documentHighlightで取得した宣言・使用範囲は同じファイルの全カードに表示する。
選択と型情報は一時的なUI状態とし、型定義への接続はセッションに保存する。
hoverはplain textを要求し、型・ドキュメントを返す。UIはホバーを遅延してバックグラウンドで取得し、
単語から説明へポインターを移すための猶予を設け、説明内のスクロールはCanvasに伝播させない。
単語と説明から離れたときは遅延して閉じ、Canvas操作では古い要求・説明を直ちに破棄する。
JSON-RPCのフレーミング、要求ID、サーバーからの要求、タイムアウト、終了処理は
`lsp` にまとめる。言語固有の初期化設定や通知の扱いは言語crateから渡す。
外部ファイルの更新時には解析キャッシュを無効にする。
仮想URIのマクロ展開など、ファイル上のソースに変換できない応答は明示的なエラーにする。

ソースルートとコンパイル設定の場所は独立して保持する。
`ProjectOptions` は言語と任意のcompilation databaseのパスを持つUI非依存モデルであり、
検出・形式の検証・clangdの起動引数への変換は `language-cpp` が担う。
C/C++のファイル一覧はソースツリーのソース・ヘッダーとcompilation databaseのソースを用い、
コードの構造と関係の解析はclangdに委ねる。Cargoのパッケージ境界を推測で代用しない。
clangdの索引はバックグラウンドで作成し、Rust専用の解析完了通知は待たない。
保存済みセッションはバックエンド起動前に対象プロジェクトを検証し、保存された解析設定を復元する。
明示された設定は保存設定より優先し、読み込みに失敗したセッションは自動保存で上書きしない。

永続化はバージョン1のJSON形式で、セッション・テーマ・設定のバージョンを個別に検査する。
現在のバージョン以外を読み込む移行処理はまだ持たず、未対応バージョンは拒否する。
セッションの解析設定と設定ファイルのclangd実行パスを保持する。
書き込みは同じディレクトリの一時ファイルを同期してからrenameし、以前のファイルを先に削除しない。

## WorkspaceとGPUI

製品workspaceはルートの `Cargo.toml` に定義する。内部依存はルートの
`[workspace.dependencies]` に `path` で登録し、各crateは `workspace = true` で継承する。
この表は共通の依存定義であり、各crateが明示的に継承した依存だけが追加される。
外部依存は複数crateで共有する `serde` / `serde_json` を共通定義に置き、
単一crate専用のライブラリはそのcrateのmanifestに直接定義する。

`xtask` は製品workspaceから除外し、`xtask/Cargo.toml` に独自の `[workspace]` を持つ。
製品の循環や壊れたmanifestで製品の依存解決が失敗しても、検査器を起動できる構成とする。
ルートと `xtask/` の両方の `Cargo.lock` をバージョン管理し、ゲートは `--locked` で実行する。

GPUIは2026-10-01に確認したZedの `main` の最新コミット
[`f8c2cc844057540ca1eac7de4f19f50d7597dead`](https://github.com/zed-industries/zed/commit/f8c2cc844057540ca1eac7de4f19f50d7597dead)
を `refscape-ui/Cargo.toml` の `rev` に固定する。同コミットのtoolchainに合わせ、Rustは `1.98.1` に固定する。

`gpui` とプラットフォーム起動用の `gpui_platform` は両方とも `refscape-ui` 内に定義し、
同じ完全なコミットSHAを使う。製品のビルドにはZedのGit取得とGPUIのOS固有依存が必要。
更新時も最新コミットを確認して完全なSHAに固定し、常時追従するbranch指定にはしない。

## ファイルとモジュールの規約

- 1ソースファイルは `max_file_lines`（1000行）以下とする。空行・コメント・文字列リテラルを除去した後にコードが残る行だけを数える。末尾の改行の有無やLF・CRLFで上限は変わらない。
- 子を持つRustモジュールは `<name>.rs` と `<name>/` の組で配置する。`mod.rs` は使わない。

`xtask/src/source_rules.rs` の `MAX_FILE_LINES` を行数上限の正本とする。
`cargo xtask gate` の最初に、Git管理下のファイルとignoreされていない未追跡ファイルを検査する。
行数検査は製品・xtask・example・テストのRust/C/C++ソース（`.rs`、`.c`、`.h`、`.cc`、`.cpp`、`.cxx`、`.hh`、`.hpp`、`.hxx`）に適用する。
ドキュメント・設定・自動生成するlockfile・依存グラフ・バイナリ資産は対象外とする。
Gitのignore対象の未追跡ファイル（`target/` など）と作業ツリーで削除したファイルは検査しない。

行コメント・docコメント・ブロックコメント（Rustでは入れ子も含む）と、通常文字列・複数行文字列・raw文字列・byte/C文字列を除外する。
文字列と同じ行にある代入・呼び出し・区切り記号などはコードとして残るため、その行は数える。
文字リテラル・ライフタイムはコードとして数える。閉じていないコメント・文字列や読み込み失敗は検査失敗とする。

Rustファイルを含む子ディレクトリには、同名の親 `.rs` ファイルが必要。
Cargoのソース・ターゲット用ディレクトリ（`src/`、`src/bin/`、`tests/`、`examples/`、`benches/`）と、
そのターゲット用ディレクトリ直下で `main.rs` / `lib.rs` を持つ個別ターゲットのルートは例外とする。
Rustファイルを含まない資産ディレクトリには親 `.rs` を要求しない。
既存の `mod.rs` 配置はこの対応検査で先に拒否せず、後続のClippyに診断を任せる。

`mod.rs` 禁止はClippyの `clippy::mod_module_files` を `deny` にして検査する。
製品はルートの `[workspace.lints.clippy]` を各crateが継承し、独立workspaceの `xtask` と
`examples/demo` はそれぞれ `[lints.clippy]` に設定する。ゲートで3 workspaceのClippyを実行する。
コンパイル対象のRustモジュールに対して適用され、既存lintの独自再実装は持たない。

全違反をパス・行数または必要な親ファイルとともに表示し、非ゼロ終了する。
既存の違反にも適用し、免除リストや自動修正は設けない。

## 構造検査

```sh
cargo xtask gate
```

検査の正本は `xtask/src/architecture.rs` の `POLICY`。
新しいcrateはworkspaceの登録と同時に責務を決め、この許可リストを明示的に更新する。
未登録のworkspace member、必須crateの欠落、所定のディレクトリ以外のmemberを拒否する。

`cargo metadata --format-version 1 --no-deps` の `packages[].dependencies` から、
すべての内部依存宣言を集めた有向グラフを作る。現在有効な依存だけを表す `resolve` は使わない。
依存種別やfeature、実行OSによる除外は行わず、次をすべて検査する。

- `dependencies`, `dev-dependencies`, `build-dependencies`
- 無効になっているoptional依存
- OS・target固有依存（実行中のOS以外も含む）
- `package` による別名依存

全宣言を合成したグラフで自己依存と循環を拒否する。
同時に有効にならないtarget間の循環も禁止する、意図的に厳しいルールとする。
さらに `POLICY` にない内部依存も拒否するため、非循環でも `ui → storage` は通らない。

依存先は正規化したmanifestのパスとCargoのpackage IDで識別する。
内部crateをregistry・gitから取得する宣言や、`workspace = true` を使わない直接path依存は拒否する。
未登録のローカルpath依存や製品から `xtask` への依存も許可しない。
`xtask` 自身も独立した単一packageのworkspaceであることと、製品・ローカルcrateに依存しないことを検査する。

外部依存についてはGPUI系の直接依存を `ui`、`lsp-types` の直接依存を `lsp` に限定する。
現在の共通LSP実装はJSONを使い、`lsp-types` を直接依存に持たない。
GPUI系はUI内に直接定義したZedの完全なコミットSHAのみ許可し、全宣言のSHA一致も検査する。
GPUI系を `workspace.dependencies` に戻す宣言も、未使用・別名を含め拒否する。
検査の保証対象は製品workspaceの内部依存宣言であり、外部ライブラリ全体の循環検査ではない。
metadata取得、manifest解析、依存先識別に失敗した場合も検査失敗とし、成功として扱わない。

循環があれば経路と依存種別を表示して非ゼロ終了する。

```text
dependency cycle:
  refscape-model
    --dev--> refscape-storage
    --normal--> refscape-model
```

Cargo自体が許容する一部のdev依存の循環も、このゲートでは禁止する。
検査器のテストは一時ディレクトリに実際のCargo workspaceを作り、Cargo metadata経由で検証する。

## 依存グラフ

```sh
cargo xtask gate
```

構造検査に通った実際の依存宣言から [dependency-graph.md](dependency-graph.md) を生成する。
許可リストをそのまま図示せず、manifestで宣言されている依存だけをMermaidの矢印にする。
矢印は依存するcrateから依存先へ向かい、ラベルに依存種別・optional・target条件を表示する。
内部依存の章では独立workspaceの `xtask` を製品と接続のないノードとして表示する。
`Direct external dependencies` 章では製品各crateと `xtask` の直接外部依存をMermaidで表示する。
normal・dev・build・optional・target条件に加え、別名は `alias`、workspace継承は `workspace` と表示する。
推移依存と未使用の共通定義は含めず、同じpackage名は一つの外部ノードにまとめる。
順序は固定し、日時やローカルの絶対パスを出力しない。生成ファイルは手動編集しない。

構造検査に成功したら、ゲートが生成ファイルを自動で作成・更新する。
古い図や欠落したファイルを理由に失敗することはなく、改行はLFで出力する。
構造検査に失敗した場合は非ゼロ終了し、既存のグラフを上書きしない。
後続のformat・clippy・testで失敗した場合も、生成済みのグラフは残る。

## ローカルのゲート

```sh
cargo xtask gate
```

以下を順番に実行し、最初の失敗で非ゼロ終了する。

1. `max_file_lines`（1000行以下）、`<name>.rs` と `<name>/` の対応を検査。
2. 全内部依存の循環・依存方向・workspace構成を検査。
3. `docs/dependency-graph.md` を自動生成・更新。
4. 製品とxtaskの `cargo fmt --check`。
5. 製品・xtask・Rust exampleの `cargo clippy --all-targets --all-features --locked -- -D warnings`（`mod_module_files` による `mod.rs` 禁止を含む）。
6. 製品とxtaskの `cargo test --all-features --locked`（製品は `--workspace`、doc-testも含む）。

ルートの `cargo fmt --all` や `cargo test --workspace` は独立workspaceのxtaskを含まない。
ゲートでは両workspaceを明示的に検査する。手動でフォーマットする場合も両方を実行する。

```sh
cargo fmt --all
cargo fmt --manifest-path xtask/Cargo.toml --all
```

xtaskの公開コマンドは `cargo xtask gate` のみとする。
依存宣言を変更したらゲートを実行し、生成された図を含めて変更を確認する。

通常のゲートは外部のrust-analyzer・clangdが必要なテストをignoreするので、
ローカルで実行する場合はREADMEの追加テストコマンドを使う。
