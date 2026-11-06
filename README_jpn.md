<p align="center">
  <img src="images/EH3D_PHOENIX_CAD_BANNER.svg" alt="EH3D-PHOENIX-CAD banner" width="100%">
</p>

# <img src="images/brand/icon-64.png" alt="" width="40" height="40" align="absmiddle"> EH3D-PHOENIX-CAD
<p align="center"><a href="README.md">🇺🇸 English</a> | <a href="README_spa.md">🇪🇸 Español</a> | <a href="README_fra.md">🇫🇷 Français</a> | <a href="README_ita.md">🇮🇹 Italiano</a> | <a href="README_deu.md">🇩🇪 Deutsch</a> | <a href="README_zho.md">🇨🇳 中文</a> | 🇯🇵 <b>日本語</b> | <a href="README_por.md">🇵🇹 Português</a></p>

### EH3D-PHOENIX-CAD：筐体、シャーシ、ヒートシンクの設計に特化したパラメトリック機械 CADとして計画中。筐体の最初のモデルがライブラリとして存在し、画面で一度も開かれていないウィンドウが付いていますが、機械ではまだ何も実行していません。

<p align="center">
  <img src="https://img.shields.io/badge/License-GPL%203.0--or--later-blue.svg" alt="GPL 3.0 or later">
  <img src="https://img.shields.io/badge/Stack-Markdown%20%2B%20Python-blue.svg" alt="Markdown and Python">
  <img src="https://img.shields.io/badge/Languages-8-00E5FF.svg" alt="Languages">
  <img src="https://img.shields.io/badge/Tests-343-2EA043.svg" alt="Tests">
  <img src="https://img.shields.io/badge/Version-0.0.3-0f6cbd.svg" alt="Version">
  <img src="https://img.shields.io/badge/Maturity-scaffolding-FFB020.svg" alt="Maturity">
</p>

---

**正直さの確認 - 今日実際に動くもの：** Rust ライブラリで、パラメトリックな筐体を扱います（独自の範囲を持つ整数ナノメートルの長さ、面の穴（丸穴、皿穴、座ぐり穴、角穴、長穴）、丸めた縁と面取りした縁、変数・定数・他のパラメータに対する単位付きの式で書けるパラメータ（厳密な分数、検査付きのオーバーフロー）、衝突を報告してモデルを変えない構造化された変更操作、基板の取り付け穴の下に立てる支柱とそこにねじ止めする蓋、プロジェクトファイルの機構ドメインに保存した筐体のレシピ、OpenSCAD 形式のスクリプト、基板の外形に対する収まりの検査）。ウィンドウ（egui、`--features gui` でビルド）がモデルのツリー、各項目のプロパティ、3D ビュー、平面ビューを表示し、`check` コマンドはプロジェクトを読んで寸法、収まり、各手順の状態を表示します。ウィンドウのロジックは画面なしでテストされています。独自のテストが 340 件あり、コンピューター上で通っています。試したのは自身のテストが作ったモデルだけで、ウィンドウを画面で開いたことも機械での実行もなく、書き出す STEP ファイルは構造だけを確認しており、ヒートシンク、スケッチ、アセンブリは計画のみです。

---

> **安全性とセキュリティの現状：** このリポジトリは最初の段階で、安全装置ではありません。ここに書かれたことが、機械とそのコントローラーの上限、インターロック、非常停止の代わりになることはありません。

## 📋 ひと目で

| | |
|---|---|
| バージョン | `0.0.3` |
| 成熟度 | 足場 |
| テスト | 343 |
| 技術スタック | Markdown, Python 3.10+ |
| 実機での動作 | まだハードウェアでは実行していません |

**移動先：** [概要](#-概要) · [アーキテクチャ](#-アーキテクチャ) · [クイックスタート](#-クイックスタート) · [テスト](#-テスト) · [リポジトリの構成](#-リポジトリの構成) · [関連プロジェクト](#-関連プロジェクト)

## 🚦 プロジェクトの状況

各行はこのリポジトリの一部分で、信号は今日どこまで進んでいるかを示します。計画ではなく、リポジトリが示せるもの（テスト、ビルド、実行）に基づいて書かれています。

🟢 完了・検証済み · 🟡 動作する（既知の制限あり） · 🟠 一部のみ、または未検証 · 🔴 未実装または動作しない

| 領域 | 状態 | ここでの意味 |
|---|---|---|
| リポジトリ チェック | 🟡 | リポジトリ チェック、画像のチェック、基盤の 3 件のテストは CI で実行され、通っています。 |
| テスト | 🟡 | Rust ライブラリの 340 件と基盤の 3 件のテストがコンピューター上で通っています。使うのはテスト自身が作ったモデルだけです。 |
| 実機 | 🟠 | どの機械やボードでも実行していません。ここにあるものは、ハードウェア上で試していません。 |
| 計画中の機能 | 🟠 | 一部完了：穴・支柱・ねじ止めの蓋を持つパラメトリックな筐体、OpenSCAD 形式のスクリプト、収まりの検査、プロジェクトファイル内の筐体のレシピと、構造だけを確認した最初の STEP 書き出しがあります。ヒートシンク、スケッチ、アセンブリは未着手です。 |
| 要件 | 🟠 | 6 件の要件のうち 4 件がプロトタイプ（筐体、スクリプト、変更操作、機械用の範囲）、2 件は計画のみで、検証済みのものはありません。 |

## 🎯 概要

* **計画中：** EH3D-PHOENIX-CAD は、筐体、シャーシ、ヒートシンクの設計に特化したパラメトリック機械 CAD を目指し、ウィンドウ付きのライブラリ、コマンドラインインターフェース、必要に応じてサーバータスクの形をとります。ライブラリは最初のパラメトリック筐体、ツリー・プロパティ・3D ビュー・平面ビューを表示するウィンドウ（画面で一度も開かれていません）、`check` コマンドとともに存在しますが、サーバータスク、STEP 出力、ヒートシンクはまだありません。

## 🧩 EH3D での役割

EH3D-PHOENIX-CAD は EH3D Fabrication Suite の PHOENIX ファミリーの一部です。何をするか、まだ何をしないか、要件、他のリポジトリとの関係は `docs/` フォルダーに書かれています。

## 🏛️ アーキテクチャ

<p align="center">
  <img src="images/architecture.svg" alt="EH3D-PHOENIX-CAD のアーキテクチャ：誰が使い、何を持ち、何に依存するか" width="100%">
</p>

このリポジトリの各部分の関係：誰が使い、何を持ち、何に依存するか。図は `docs/architecture.json` から描かれ、コードと照合されます。

## 🚀 クイックスタート

Python 3.10 以降が必要で、インストールするものはありません。

```bash
python -I tools/ci_validate.py
python -I tools/make_architecture.py --check
python -I tools/make_structure.py --check --readmes
python -I -m unittest discover -s tools -p "test_*.py"
```

プログラムを試すには（Rust が必要。ウィンドウには `--features gui` が必要）：

```bash
cargo run -p eh3d-phoenix-cad -- check crates/eh3d-phoenix-cad/examples/box.phoenixproj
cargo run --release -p eh3d-phoenix-cad --features gui -- crates/eh3d-phoenix-cad/examples/box.phoenixproj
cargo run --release -p eh3d-phoenix-cad --features gui -- gui crates/eh3d-phoenix-cad/examples/box.phoenixproj --shot box.png
```

## 🧪 テスト

Rust ライブラリには 340 件のテスト（`cargo test`）、基盤には 3 件のテスト（`python -I -m unittest discover -s tools -p "test_*.py"`）があり、すべてコンピューター上で通っています。CI はリポジトリ チェック、画像のチェック、基盤のテストを実行し、Rust のコードについては書式チェック、警告をエラーとする clippy、Linux と Windows でのテストを実行し、ウィンドウも clippy（`--features gui`）で検査します。

## 🏗️ ビルドとリリース

バージョンは `VERSION` ファイルと `eh3d.project.json` にあり、各バージョンには [CHANGELOG.md](CHANGELOG.md) の項目があります。ビルドするものはなく、リリースは GitHub で公開されます。

## 📂 リポジトリの構成

<p align="center">
  <img src="images/repository-structure.svg" alt="EH3D-PHOENIX-CAD のリポジトリ構成" width="100%">
</p>

<details>
<summary>ツリーのテキスト版</summary>

```text
EH3D-PHOENIX-CAD/
├── docs/
│   ├── architecture.json structure.json         2 つの図のデータ
│   ├── SCOPE.md REQUIREMENTS.md                 計画内容とその要件
│   ├── DEPENDENCIES.md THREAT_MODEL.md          他のリポジトリとの関係と脅威モデル
│   ├── DECISIONS.md STATUS.md                   適用される決定事項と正直な状況
│   ├── NOT_YET.md RULES.md                      後回しにしたアイデアとコードの記述規則
│   ├── ERROR_CODES.md                           診断コード PHOENIX-<領域>-<NNN>
│   ├── EXPRESSIONS.md                           パラメータの式の言語
│   ├── PROJECT_DOMAIN.md                        プロジェクトファイル内の筐体のレシピ
│   └── adr/                                     承認された決定の記録
├── Cargo.toml Cargo.lock                        Rust ワークスペースと固定された依存関係
├── crates/                                      機械モデル：筐体、操作、スクリプト、収まりの検査、ウィンドウ
├── AGENTS.md                                    リポジトリを変更する人のための規則
├── tools/                                       リポジトリ チェック、そのテスト、画像のツール
├── images/                                      バナーとアーキテクチャ・リポジトリ構成の図
├── .github/                                     CI とダッシュボードのワークフロー、Issue と PR のテンプレート
├── eh3d.project.json                            マニフェスト：バージョン、役割、成熟度、ビルドコマンド
├── VERSION                                      リポジトリのバージョン
├── CHANGELOG.md                                 各バージョンの変更点
├── SECURITY.md SUPPORT.md                       脆弱性の報告方法と問い合わせ先
└── CONTRIBUTING.md CODE_OF_CONDUCT.md LICENSE   貢献方法、行動規範、ライセンス
```

</details>

## 🗺️ ロードマップと既知の制限

**現時点で分かっている制限**

* 初期段階のコード：あるのは筐体のモデルだけで、自身のテストが作ったモデルでしか試していません。機械では何も実行しておらず、検証済みの要件はありません。

**次のステップ**

* 基板ツールと一緒にプロジェクト形式の機械ドメインを書き、そこから来る変更操作を書く。そのあとに STEP 出力、ヒートシンク、コマンドラインを、それぞれのテストとともに追加する。

## 🌍 言語

README とテキストは、英語、スペイン語、フランス語、イタリア語、ドイツ語、中国語、日本語、ヨーロッパポルトガル語で提供されています。

## 🔗 関連プロジェクト

* [EH3D-DOCS](https://github.com/JuanenRac/EH3D-DOCS)：スイートの地図、アーキテクチャ、規約、共有チェック
* [EH3D-COMMON](https://github.com/JuanenRac/EH3D-COMMON)：穴が使うオブジェクト識別子（UUID v7）。今後はプロジェクトコンテナと診断クレートも
* 計画中の関係（未構築）（EH3D-PHOENIX-PCB, EH3D-THERMAL-LAB）：[docs/DEPENDENCIES.md](docs/DEPENDENCIES.md) を参照

## 📚 ドキュメント

* [スイートの規約](https://github.com/JuanenRac/EH3D-DOCS/blob/main/docs/CONVENTIONS.md)
* [変更履歴](CHANGELOG.md)
* [範囲、要件、依存関係、脅威モデル、決定事項](docs/SCOPE.md)

## 🤝 コントリビュートとコミュニティ

* プル リクエストの前に `python -I tools/ci_validate.py` と基盤のテストを実行してください。すべて通る必要があります。[CONTRIBUTING.md](CONTRIBUTING.md) と[行動規範](CODE_OF_CONDUCT.md)を参照してください。
* README は 8 言語あります。文章を変更するときは 8 つのファイルすべてを変更し、2 つの図は `docs/architecture.json` と `docs/structure.json` から `tools/make_architecture.py` と `tools/make_structure.py` で描画します。
* 脆弱性は非公開で報告してください：[SECURITY.md](SECURITY.md)。質問、アイデア、報告：electrohobby3d@gmail.com（[SUPPORT.md](SUPPORT.md)）。

## 👤 作者

**JuanenRac (Electro Hobby 3D)** · electrohobby3d@gmail.com

## 📜 ライセンス

ソフトウェア：GPL-3.0-or-later。[LICENSE](LICENSE) を参照。スイートのハードウェア設計には CERN OHL-S を使います。
