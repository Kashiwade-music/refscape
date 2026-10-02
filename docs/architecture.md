# Crate構成と依存ゲート

製品は14 crate、`xtask`は製品に依存しない独立workspaceで構成する。下表はdev・build・optional・target指定を含む内部依存の上限であり、`xtask/src/architecture.rs`の明示POLICYで検査する。

| crate | 責務 | 許可する内部依存（`refscape-`省略） |
|---|---|---|
| model | immutable source、projection、fold状態、identity、座標、operation、theme | なし |
| analysis | 解析factory、成立済みsession、capability、要求契約 | model |
| canvas | sourceを持たないgeometry入力から純粋な配置deltaを計画 | model |
| application | 単一controller、command/effect/completion、保存・project寿命、worker executor | model, analysis, canvas |
| lsp | 常時RPC dispatcher、bounded pipe、document snapshot、process supervisor | model, analysis |
| language-support | walker、catalog、launch resolver、共通query・opened runtime | model, analysis, lsp |
| language-rust | Cargo metadata、Rust profileとfactory | model, analysis, lsp, language-support |
| language-cpp | compilation database、C++ profileとfactory | model, analysis, lsp, language-support |
| language-typescript | TS/JS profileとfactory | model, analysis, lsp, language-support |
| language-python | Python profileとfactory | model, analysis, lsp, language-support |
| language | 言語選択registry、起動設定のdecision table | model, analysis, language-support, language-rust, language-cpp, language-typescript, language-python |
| storage | v1専用DTO、検証済みimport、borrowed streaming export、原子的置換 | model, application |
| ui | GPUI描画・入力、controller driver、可視行shape/cache | model, application |
| app | CLI正規化、環境採取、具体adapterの生成・接続 | model, analysis, application, language, storage, ui |

現在の全依存宣言は[生成依存図](dependency-graph.md)に記載する。GPUIはui、JSON-RPC/LSPのwire処理はlspに閉じる。言語crate間の依存、全依存種の循環、`mod.rs`、1000 code lines超過を拒否する。

`ApplicationController`だけが可変project/canvas正本を持つ。UIはintentをcommandとして渡し、workerはimmutable snapshotと解析sessionを使ってeffectを実行する。headless checkも同じcontrollerを駆動する。completionはjob/project/revisionを照合して採用し、旧projectの遅着結果を混入させない。I/O中にcontrollerをlockしない。

カードはArc source snapshotとfold状態を持ち、projection・UTF-16 index・寸法を共有する。canvas plannerはID・矩形・edge・順序のみを受け取る。saved sourceはimport後にディスク更新へ自動追従しない。serializationはstorageだけが所有し、v1のID・保存順・source・themeを保持する。未接続Settings APIは撤去した。

LSPは共通Tokio runtimeでreader/writer/dispatcher/supervisorを稼働させる。operationの絶対期限とcancelをRPC・file/process処理へ通し、server refreshをcache epochへ反映する。queueとcacheには上限があり、process終了をUI threadで待たない。各言語のwarmup順とsearch merge優先順はprofileで保持する。コード構造の解析はLSP等の公式機能を使い、独自構文推測で補わない。

言語追加ではprofile/factoryを実装し、registryとPOLICYへ登録する。application/UIへ言語別委譲を追加しない。factoryは完全準備済みcandidateを返し、失敗時に旧projectを交換しない。一覧取得のみの失敗では新projectを採用し、一覧を空にして保存先を保護する。

| コマンド | 検査対象 |
|---|---|
| `cargo xtask graph` | 依存図を明示更新する |
| `cargo xtask gate` | source/POLICY/生成図check、fmt、strict clippy、外部server不要の全test |

通常gateは文書を書き換えず、生成図との差を失敗にする。依存宣言を変更したら`cargo xtask graph`を実行し、生成図もレビューする。製品とxtaskは独立workspaceのため、format・clippy・testは両方を検査する。実サーバーのテストは外部ツールが必要なため通常gateではignoreされる。言語別の実行方法とGPU描画exampleはREADMEの開発手順を参照する。
