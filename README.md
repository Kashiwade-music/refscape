# refscape
A spatial code explorer for navigating Rust through calls, references, and definitions.

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

## 将来構想

- マルチプラットフォーム（Windows/Linux/macOS）
  - まずはWindowsから
- 多言語対応（C++/Python/TypeScript/React）
  - まずはRustから

コード編集機能は将来においても不要。\
カードの位置・接続・ズームを保存し、次回復元したり、セッションとして保存する機能は最初から必要。\
UIテーマはライトモードとダークモードを既定で用意しておき、ユーザーがカスタムテーマを作成・配布できるようにする。

## 依存ライブラリ

- 描画にはGPUIを使うこと。crates.io 空ではなく、zedのGitHubリポジトリの最新版のコミットから取得すること。

## 開発

製品は6 crateのCargo workspaceで構成し、開発用の `xtask` は独立したworkspaceとする。
各crateの責務・依存方向・検査ルールは [アーキテクチャ](docs/architecture.md) を参照。
現在の依存宣言は [依存グラフ](docs/dependency-graph.md) にMermaidで出力する。

Rust toolchainは `rust-toolchain.toml` に固定している。リポジトリのルートで次を実行する。

```sh
# 構造検査、依存グラフの自動生成・更新、format、clippy、test
cargo xtask gate

# 空のエントリーポイントを実行（UIは未実装）
cargo run
```

`cargo xtask gate` が構造検査に成功すると、`docs/dependency-graph.md` を自動生成・更新する。
