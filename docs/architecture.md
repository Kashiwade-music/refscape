# Crate構成と依存ゲート

## 責務

| crate | 責務 | 許可する内部依存 |
|---|---|---|
| `refscape-model` | UI非依存のソース位置・シンボル・カード・接続・領域・座標・ズーム・セッション・テーマのモデルと不変条件 | なし |
| `refscape-application` | プロジェクト開始、定義展開、カード操作、セッション保存・復元などの操作と外部機能のtrait | `model` |
| `refscape-lsp` | LSPクライアント、サーバー管理、言語設定、プロトコル型と独自モデルの変換 | `application`, `model` |
| `refscape-storage` | セッション・設定・テーマの保存形式、読み書き、バージョン移行 | `application`, `model` |
| `refscape-ui` | GPUIによるCanvas・コードカード・接続線・入力・ハイライト・テーマの描画 | `application`, `model` |
| `refscape-app` | 実行バイナリ `refscape`、具体的なアダプターとビューの生成・接続、起動・終了処理 | 上記5 crate |
| `xtask` | 構造検査、依存グラフの生成、開発用ゲート | 製品への依存なし |

表の依存先は `refscape-` を省略したもの。許可する依存先の上限を示す。
現時点の製品crateはドキュメントと空のエントリーポイントだけで、機能の実装は含まない。

`application` が `LanguageService` や `SessionRepository` といったtraitを定義し、
`lsp` と `storage` がそれを実装する。`app` が具体的な実装を生成して渡す。
`application` から具体的なアダプターへ依存しない。

GPUIの型は `ui` と起動に必要な `app` に閉じ込め、LSPの型は `lsp` の境界で独自モデルに変換する。
Canvasの状態や配置規則はUI非依存とし、ファイルの保存形式と移行処理は `storage` が管理する。
テーマの意味的な定義は `model`、保存は `storage`、描画用の型への変換は `ui` が担当する。
コードの構造解析はREADMEの方針に従いLSP等の公式機能を利用し、未対応の機能を推測で補わない。

## WorkspaceとGPUI

製品workspaceはルートの `Cargo.toml` に定義する。内部依存はルートの
`[workspace.dependencies]` に `path` で登録し、各crateは `workspace = true` で継承する。

`xtask` は製品workspaceから除外し、`xtask/Cargo.toml` に独自の `[workspace]` を持つ。
製品の循環や壊れたmanifestで製品の依存解決が失敗しても、検査器を起動できる構成とする。
ルートと `xtask/` の両方の `Cargo.lock` をバージョン管理し、ゲートは `--locked` で実行する。

GPUIは2026-10-01に確認したZedの `main` の最新コミット
[`40180d9c40e2d20eb63d388bff920818f2910b53`](https://github.com/zed-industries/zed/commit/40180d9c40e2d20eb63d388bff920818f2910b53)
を共通依存の `rev` に固定する。同コミットのtoolchainに合わせ、Rustは `1.98.1` に固定する。

空実装ではGPUIを有効にしないため、製品のビルドと構造検査にZedの取得は不要。
UI実装を始めるときに `refscape-ui/Cargo.toml` の `[dependencies]` へ
`gpui.workspace = true` を追加し、ルートのlockfileを更新する。
更新時も最新コミットを確認して完全なSHAに固定し、常時追従するbranch指定にはしない。

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

外部依存についてはGPUI系の直接依存を `ui` / `app`、`lsp-types` の直接依存を `lsp` に限定する。
GPUI本体はZedの完全なコミットSHAで固定した共通依存の継承のみ許可する。
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

構造検査に通った実際の内部依存宣言から [dependency-graph.md](dependency-graph.md) を生成する。
許可リストをそのまま図示せず、manifestで宣言されている依存だけをMermaidの矢印にする。
矢印は依存するcrateから依存先へ向かい、ラベルに依存種別・optional・target条件を表示する。
外部依存は省略し、独立workspaceの `xtask` は製品と接続のないノードとして表示する。
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

1. 全内部依存の循環・依存方向・workspace構成を検査。
2. `docs/dependency-graph.md` を自動生成・更新。
3. 製品とxtaskの `cargo fmt --check`。
4. 製品とxtaskの `cargo clippy --all-targets --all-features --locked -- -D warnings`。
5. 製品とxtaskの `cargo test --all-features --locked`（製品は `--workspace`、doc-testも含む）。

ルートの `cargo fmt --all` や `cargo test --workspace` は独立workspaceのxtaskを含まない。
ゲートでは両workspaceを明示的に検査する。手動でフォーマットする場合も両方を実行する。

```sh
cargo fmt --all
cargo fmt --manifest-path xtask/Cargo.toml --all
```

xtaskの公開コマンドは `cargo xtask gate` のみとする。
依存宣言を変更したらゲートを実行し、生成された図を含めて変更を確認する。
