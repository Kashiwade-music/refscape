# refscape
A spatial code explorer for navigating Rust through calls, references, and definitions.

## 概要

Refscape は、コード本文そのものを Infinite Canvas 上に配置して読む Native コードリーディングアプリ。\
IDEのようにファイルやタブを行き来するのではなく、関数・型・実装などをカードとして空間上に展開し、コード同士の関係を視覚的に辿る。

## 基本UIとユーザー体験

各カードには実際のソースコードを表示する。

```rs
fn main() {
    let config = load_config();
    run(config);
}
```

`load_config()` をクリックして展開すると、その定義が別カードとして近くに追加される。

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

## 将来構想

- マルチプラットフォーム（Windows/Linux/macOS）
- 多言語対応（C++/Python/TypeScript/React）


