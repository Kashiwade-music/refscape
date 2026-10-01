# Refscape
A native spatial code explorer for navigating Rust through references and definitions.

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

なお、コードの構造解析はヒューリスティックには行わず、各言語の公式が提供する機能（LSP等）を用いて実施する。

## 起動

Windowsを優先したRust/Cargoプロジェクト向けの実装。ビルドにはRustup、MSVC C++ Build Tools、
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

解析には `rust-analyzer` を使用し、ソースの構造を正規表現などで推測しない。
初回解析にはプロジェクトの依存取得・インデックス作成が必要になる。
プロジェクトが独自のRust toolchainを指定する場合、そのtoolchainにも
`rust-analyzer` と `rust-src` をインストールする。
実行ファイルは `--rust-analyzer PATH` または `REFSCAPE_RUST_ANALYZER` 環境変数で指定できる。

## 操作

左のファイル一覧からファイルカードを開くか、検索欄にシンボル名を入力してEnterで検索し、
結果をクリックしてカードを配置する。コード上のクリックは元のファイル位置をrust-analyzerに渡し、
結果を右側のカードとして展開する。同じシンボルのカード・同じ位置の接続は再利用する。
接続元のコード単語に下線を引き、その下線の右端から定義・参照の矢印を伸ばす。
下線と接続位置はパン・ズームやカード移動に追従する。コードを省略するズーム階層では接続線も省略する。

| 操作 | 動作 |
|---|---|
| コードを左クリック | 定義を展開 |
| コードを右クリック | 参照元を展開 |
| カードのヘッダーをドラッグ | カードを移動 |
| 空のCanvasをドラッグ／ホイール | パン |
| Shift + ホイール | 横方向にパン |
| Ctrl + ホイール／`+`・`-` | 拡大・縮小 |
| `0`／Fit | 全カードを画面に収める |
| Ctrl + F／Ctrl + P | 検索欄にフォーカス |
| Ctrl + S | 現在のセッションを保存 |
| Ctrl + Shift + S | 名前を付けて保存 |
| Ctrl + O | 保存済みセッションを開く |
| Ctrl + Shift + O | プロジェクトを選択 |
| カードの `×`／選択してDelete | カードとその接続を削除 |
| Themeボタン | テーマを切り替える |

65%以上のズームではソースコード、35～65%ではカードの概要、35%未満では領域を表示する。
領域はCargo metadataに基づくworkspaceのcrate（Cargo package単位）と、
ソースファイルごとにカードを包む。
解析中もCanvasを移動でき、解析・保存のエラーは下部のステータス欄に表示する。

## セッションとテーマ

既定の保存先はプロジェクト内の `.refscape/session.json`。起動時に保存済みセッションを復元し、
通常のウィンドウ終了時や別のプロジェクト・セッションへ切り替える前にも保存する。
Ctrl + SやSaveボタンで途中保存できる。
コードカードの本文・位置・接続・領域・パン・ズーム・テーマをJSONで保存する。
保存は同じディレクトリの一時ファイルへの書き込み後に置き換え、
不正なデータや未対応の保存形式バージョンはエラーとして扱う。
自動復元に失敗した既存ファイルは終了時に上書きせず、Save asで別の保存先を選べる。
セッションはソースのスナップショットを含む。外部でソースを変更した後は、
該当カードを削除してファイルまたはシンボルを再配置し、最新の本文を読み直す。

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
- 多言語対応（C++/Python/TypeScript/React）
  - まずはRustから

コード編集機能は持たない。言語解析はRustの `rust-analyzer` が対象。
Windows以外のプラットフォームと他言語は将来の対応範囲。
GPUIが使う描画バックエンドに対応したGPUドライバーが必要。
マクロ展開の仮想URIなど、実ファイルに対応しないソースへの移動は未対応で、
解析バックエンドのエラーを画面に表示する。

## 依存ライブラリ

- 描画はGPUIを使用。crates.io版ではなく、確認時点のZedの最新Gitコミットに固定する。

## 開発

製品は6 crateのCargo workspaceで構成し、開発用の `xtask` は独立したworkspaceとする。
各crateの責務・依存方向・検査ルールは [アーキテクチャ](docs/architecture.md) を参照。
現在の依存宣言は [依存グラフ](docs/dependency-graph.md) にMermaidで出力する。

Rust toolchainは `rust-toolchain.toml` に固定している。リポジトリのルートで次を実行する。

```sh
# 構造検査、依存グラフの自動生成・更新、format、clippy、test
cargo xtask gate

# 起動
cargo run --locked

# 実際のrust-analyzerで定義・参照・ハイライトを検証
cargo test -p refscape-language --locked --test rust_analyzer -- --ignored

# 実プロジェクトのカード展開・重複防止・名前付きセッション復元を検証
cargo test -p refscape-app --locked --test workflow -- --ignored

# ウィンドウを開かず、解析要求・Canvas操作・セッション復元を検証
cargo run --locked -- --check "C:\code\my-project"
```

`cargo xtask gate` が構造検査に成功すると、`docs/dependency-graph.md` を自動生成・更新する。
実サーバーのテストは `rust-analyzer` が外部toolchain componentのため通常はignoreされる。

実GPUによる描画確認用のexampleも用意する。`main` を持つプロジェクトで、
ライト／ダークのPNGを出力できる。GPUとデスクトップ環境が必要。

```powershell
cargo run -p refscape-app --locked --example render --features visual-tests -- examples/demo target/refscape-dark.png
cargo run -p refscape-app --locked --example render --features visual-tests -- examples/demo target/refscape-light.png light
```
