<p align="center">
  <img src="images/EH3D_PHOENIX_CAD_BANNER.svg" alt="EH3D-PHOENIX-CAD banner" width="100%">
</p>

# <img src="images/brand/icon-64.png" alt="" width="40" height="40" align="absmiddle"> EH3D-PHOENIX-CAD
<p align="center"><a href="README.md">🇺🇸 English</a> | <a href="README_spa.md">🇪🇸 Español</a> | <a href="README_fra.md">🇫🇷 Français</a> | <a href="README_ita.md">🇮🇹 Italiano</a> | <a href="README_deu.md">🇩🇪 Deutsch</a> | <a href="README_zho.md">🇨🇳 中文</a> | 🇯🇵 <b>日本語</b> | <a href="README_por.md">🇵🇹 Português</a></p>

### EH3D-PHOENIX-CAD：筐体、シャーシ、ヒートシンクの設計に特化したパラメトリック機械 CADとして計画中。筐体の最初のモデルがライブラリとして存在し、画面で一度も開かれていないウィンドウが付いていますが、機械ではまだ何も実行していません。

<p align="center">
  <img src="https://img.shields.io/badge/License-GPL%203.0--or--later-blue.svg" alt="GPL 3.0 or later">
  <img src="https://img.shields.io/badge/Stack-Rust%20%2B%20Markdown%20%2B%20Python-blue.svg" alt="Rust, Markdown and Python">
  <img src="https://img.shields.io/badge/Languages-8-00E5FF.svg" alt="Languages">
  <img src="https://img.shields.io/badge/Tests-1802-2EA043.svg" alt="Tests">
  <img src="https://img.shields.io/badge/Version-0.1.5-0f6cbd.svg" alt="Version">
  <img src="https://img.shields.io/badge/Maturity-scaffolding-FFB020.svg" alt="Maturity">
</p>

---

**正直さの確認 - 今日実際に動くもの：** Rust ライブラリで、パラメトリックな筐体を扱います（独自の範囲を持つ整数ナノメートルの長さ、面の穴（丸穴、皿穴、座ぐり穴、角穴、長穴）、丸めた縁と面取りした縁、変数・定数・他のパラメータに対する単位付きの式で書けるパラメータ（厳密な分数、検査付きのオーバーフロー）、衝突を報告してモデルを変えない構造化された変更操作、基板の取り付け穴の下に立てる支柱とそこにねじ止めする蓋、プロジェクトファイルの機構ドメインに保存した筐体のレシピ、OpenSCAD 形式のスクリプト、基板の外形に対する収まりの検査）。ウィンドウ（egui、`--features gui` でビルド）がモデルのツリー、各項目のプロパティ、3D ビュー、平面ビューを表示し（穴やポストをマウスでドラッグし、距離や角度を測定できます。スクリプト、STEP ファイル、適合レポートはメニューからファイルに書き出せ、専用のチェックボックスを入れない限り既存のファイルには上書きしません。レポートメニューでは、切断計画、穴リスト、部品間の隙間、配置、質量と重心、スケッチの図面を 8 言語のテキストで表示できます）、`check` コマンドはプロジェクトを読んで寸法、収まり、各手順の状態を表示します。ウィンドウのロジックは画面なしでテストされています。独自のテストが 1802 件あり、コンピューター上で通っています。試したのは自身のテストが作ったモデルだけで、ウィンドウを画面で開いたことも機械での実行もなく、書き出す STEP ファイルはこのプログラム自身が読み戻しただけで、2D スケッチ（形状、拘束、ソルバー、描画と寸法付けのためのウィンドウ）はプロジェクトファイルに保存され、その閉じた輪郭は面から押し出したり面に切り込んだりでき、レシピの手順になりますが、自身のテストが作ったスケッチでしか試しておらず、そのウィンドウもまだ画面で開いたことがなく、穴とポストのパターンおよび冷却フィン（静止空気への放熱量の簡易推定つきで、実測とは比較していません）はありますが、自身のテストが作ったモデルでしか試しておらず、カタログの部品（長さに切断したプロファイル、ライブラリファイルから読み込んでモデルに配置したネジとブラケット。部品表、切断リスト、材料ごとの密度による体積と質量つき。例のライブラリは架空のものです）も同様で、これらの部品の簡易アセンブリ（ある部品を別の部品に対して 1 つの軸に沿って間隔・整列・積み重ねで配置し、プロファイル・ブラケット・ネジの正確な形状で体積を共有する部品を調べ、ブラケットの穴を通るネジは許容します。例の 2 つのアセンブリは架空のものです）も同様です。モデルのソリッドは STL ファイル（バイナリまたはテキスト。閉じた表面を正確な体積と照合し、作れない形状は拒否します）として、平面・正面・側面は DXF と SVG の図面として、エクスポートメニューまたは `export` コマンドで書き出せます。これらのファイルはスライサー、CAD、作図ソフトで開いたことはありません。平面・円筒面・円錐面で構成されたソリッドの STEP ファイルを読み込めます（スケッチのプロファイルを押し出した角柱もその形式で書き出されます）。読み込んだものの質量と慣性は三角形から計算され、筐体の DXF 図面には用紙サイズのシートを標準尺度で載せることができます。これらの STEP ファイルを CAD ソフトで開いたことはありません。コマンドラインから、メッシュの印刷可否の確認（シェル、種数、開いた辺、裏返しの表面、回転半径）、結合と向きの修正、値と角度の変換（インチの分数、度分秒）、ボルトサークルの一覧、税を含む部品表のコスト配分、点の測定、設定と既定値の比較、スケッチの要約、テンプレートからの新規プロジェクト作成ができます。これらを機械で試したことはありません。他のプログラムのファイル（圧縮された 3MF パッケージ、寛容な STL、PLY、OFF、DXF の面、面で構成された STEP、SVG 図面）の読み書き、ソリッドの変換・拡大縮小・回転・鏡像・パターン繰り返し・平面や箱での切断と断面の作成、検証されていない表からの ISO 286 の寸法許容差とはめあいの計算ができます。これらを他のプログラムのファイルや機械で試したことはありません。設計の履歴は依存関係のグラフです（数式と単位をもつパラメータ、軸に平行なボックスに対する操作と厳密なブーリアン演算、編集が及ぶ部分だけを行う再構築、ノードごとのコード付きエラー、コマンド `history-check`）。スケッチソルバーの数値エンジンは独立したクレートで、ウィンドウはスイートのドッキングフレーム（取り外し・戻し・移動・フロート・折りたたみができるパネル、ワークスペース、メニューバー）で構成されています。これは自前のテストで作った履歴でしか試しておらず、ウィンドウは画面で開いたことがありません。2D 図面シート（履歴のソリッドのビュー（可視稜線と隠れ稜線）、ID で履歴のノードとスケッチに結び付けられ、編集に追従して孤立を知らせる寸法、レイヤー、履歴から導いた部品表に結び付けられ番号が変わらないバルーン、ハッチング付き断面図と詳細図、ISO 286 公差、連続寸法と座標寸法、改訂表、表題欄、破断ビュー、引出線付きの注記、部品の自由属性、SVG・PDF・DXF ファイル、コマンド `drawing-check` と `drawing-export`、ウィンドウのパネル）は、自前のテストで作ったソリッドとスケッチでしか試しておらず、どのファイルも作図ソフトで開いたことはありません。 プロジェクトは、スイートの 1 つの `.eh3d` ファイルの `cad` ブランチにも置けます（各プログラムが自分のブランチを持ちます）。このプログラムは `pcb` ブランチが記述する基板の周りに筐体を作り、基板の変更を知らせ、支柱を穴に合わせて移動し、部品の外形が収まるかを確認し、コネクタの開口を作り、STL・展開した面・支柱の位置を返します。安定した名前に対するセレクターと、パラメーターを制限値と照らし合わせる表もあります。基板側のプログラムはまだその記述を書き出さないため、テストが作った架空の基板でのみ試されています。ウィンドウのファイルメニューからは、このファイルを開く・保存する・ブランチを一覧する・取り込んだ原本を保持する・基板を筐体に取り込むことができ、`sketch-check` コマンドはスケッチファイルの誤りやわずかなずれを報告します。いずれも画面上で確認されたことはなく、実際の設計でも試していません。プロジェクトはバリアント（変数の名前付きの値）を保持でき、適用、検査、1 つずつのファイルへの書き出しができます。スケッチの寸法は参照寸法（読み取るだけで拘束しない）にできます。ウィンドウでは `.eh3d` プロジェクトのタイトルと説明を編集し、信頼する公開鍵でブランチの署名を検証できます。これらはテストでのみ確認しており、新しいウィンドウは画面で確認していません。また、バリアントはステップの無効化と有効化ができ、変数はスケッチの参照寸法に追従でき、バリアントは数値とともに表として書き出して読み戻せます。これもテストでのみ確認しています。

---

> **安全性とセキュリティの現状：** このリポジトリは最初の段階で、安全装置ではありません。ここに書かれたことが、機械とそのコントローラーの上限、インターロック、非常停止の代わりになることはありません。

## 📋 ひと目で

| | |
|---|---|
| バージョン | `0.1.5` |
| 成熟度 | 足場 |
| テスト | 1802 |
| 技術スタック | Rust, Markdown, Python 3.10+ |
| 実機での動作 | まだハードウェアでは実行していません |

**移動先：** [概要](#-概要) · [アーキテクチャ](#-アーキテクチャ) · [クイックスタート](#-クイックスタート) · [テスト](#-テスト) · [リポジトリの構成](#-リポジトリの構成) · [関連プロジェクト](#-関連プロジェクト)

## 🚦 プロジェクトの状況

各行はこのリポジトリの一部分で、信号は今日どこまで進んでいるかを示します。計画ではなく、リポジトリが示せるもの（テスト、ビルド、実行）に基づいて書かれています。

🟢 完了・検証済み · 🟡 動作する（既知の制限あり） · 🟠 一部のみ、または未検証 · 🔴 未実装または動作しない

| 領域 | 状態 | ここでの意味 |
|---|---|---|
| リポジトリ チェック | 🟡 | リポジトリ チェック、画像のチェック、基盤の 4 件のテストは CI で実行され、通っています。 |
| テスト | 🟡 | Rust ライブラリの 1802 件と基盤の 4 件のテストがコンピューター上で通っています。使うのはテスト自身が作ったモデルだけです。 |
| 実機 | 🟠 | どの機械やボードでも実行していません。ここにあるものは、ハードウェア上で試していません。 |
| 計画中の機能 | 🟠 | 一部完了：穴・支柱・ねじ止めの蓋を持つパラメトリックな筐体、OpenSCAD 形式のスクリプト、収まりの検査、プロジェクトファイル内の筐体のレシピと、このプログラムだけがファイルを読んだことのある最初の STEP 書き出しがあります。拘束、ソルバー、描画用ウィンドウを持つ 2D スケッチがあり、プロジェクトファイルに保存され、面から押し出したり面に切り込んだりできますが、そのウィンドウはまだ画面で開いたことがなく、穴とポストのパターンと、放熱量の推定つきの冷却フィンがありますが、テストが作ったモデルでしか試していません。ライブラリファイルのカタログ部品（プロファイル、ネジ、ブラケット）をモデルに配置し、部品表と材料ごとの質量を出す機能がありますが、架空の部品でしか試していません。これらの部品の簡易アセンブリ（ある部品を別の部品に対して 1 つの軸に沿って配置する機能と、体積を共有する部品を調べる機能）がありますが、架空の部品でしか試していません。2D 図面シート（ビュー、モデルに追従する寸法、レイヤー、部品表に結び付けられたバルーン）がありますが、テストで作ったソリッドでしか試していません。 |
| 要件 | 🟠 | 38 件の要件のうち 37 件がプロトタイプ（FCAD-001 と FCAD-003～FCAD-038、docs/REQUIREMENTS.md 参照）、1 件は計画のみで、検証済みはありません。 |

## 🎯 概要

* **計画中：** EH3D-PHOENIX-CAD は、筐体、シャーシ、ヒートシンクの設計に特化したパラメトリック機械 CAD を目指し、ウィンドウ付きのライブラリ、コマンドラインインターフェース、必要に応じてサーバータスクの形をとります。ライブラリは最初のパラメトリック筐体、ツリー・プロパティ・3D ビュー・平面ビューを表示し、穴やポストをマウスでドラッグして距離や角度を測定できるウィンドウ（画面で一度も開かれていません）、`check` コマンドとともに存在しますが、サーバータスクはまだなく、冷却フィン（放熱量の推定つき）は STEP ファイルに含まれず、STEP ファイルはこのプログラム自身が読み戻しただけです。
* **プロジェクトの添付ファイル：** ウィンドウにはスイートの「添付ファイル」パネルがあります（「表示」または *ファイル > スイートプロジェクト*）。`.eh3d` プロジェクトのデータシート、マニュアルなどを一覧表示し、添付（ファイルまたはリンクのみ）、部品への関連付け、確認後の展開と削除、一覧の横での表示ができます。プログラム、スクリプト、未知の形式はプロジェクトから出す前に確認し、何もダウンロードしません。パッケージには PDF ライブラリ（PDFium）とそのライセンスが含まれます。テストで確認しただけで、画面では試していません。
* **他のプログラムのファイルを内部で表示：** プロジェクトの 3D または平面 DXF の添付ファイル、インポートで保管した元ファイル、ディスク上のファイル（*ファイル > 3D または DXF ファイルを表示*、`--viewer FILE`）を、陰影付きのソリッドまたは線として表示します。寸法、閉じていれば体積、箱に収まるか、正面などの視点、2 頂点間の距離も示します。何も書き込みません。上限は `[model_view]` セクションです。表面は鋭い辺の間で滑らかに表示でき、内部を見るための断面はハッチングした面で閉じて面積を示せます。
* **スイートプロジェクトへの署名：** *スイートプロジェクト > スイートプロジェクトに署名* と、コマンド `eh3d-keygen`、`eh3d-sign` で、読み取るだけの Ed25519 秘密鍵ファイルを使い `cad` ブランチまたはプロジェクト全体に署名します。署名を削除することになる書き込みの前に、ウィンドウは確認します（Enter と Esc では何も書き込みません）。コマンドは確認せず、削除されたことを表示します。

## 🧩 EH3D での役割

EH3D-PHOENIX-CAD は EH3D Fabrication Suite の PHOENIX ファミリーの一部です。何をするか、まだ何をしないか、要件、他のリポジトリとの関係は `docs/` フォルダーに書かれています。

<p align="center">
  <img src="images/screens/ja/phoenix-cad-01-box.png" alt="EH3D-PHOENIX-CAD window" width="49%">
  <img src="images/screens/ja/phoenix-cad-02-sketch.png" alt="EH3D-PHOENIX-CAD sketch" width="49%">
</p>

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
cargo run -p eh3d-phoenix-cad -- eh3d-save crates/eh3d-phoenix-cad/examples/box.phoenixproj job.eh3d
cargo run -p eh3d-phoenix-cad -- eh3d-info job.eh3d
cargo run -p eh3d-phoenix-cad -- eh3d-keygen --out mine.key
cargo run -p eh3d-phoenix-cad -- eh3d-sign job.eh3d --key mine.key
cargo run -p eh3d-phoenix-cad -- variants job.eh3d --check --text
cargo run -p eh3d-phoenix-cad -- variants job.eh3d --figures
cargo run -p eh3d-phoenix-cad -- variables job.eh3d --text
cargo run -p eh3d-phoenix-cad -- select job.eh3d "edges|Z"
cargo run -p eh3d-phoenix-cad -- params job.eh3d --text
cargo run -p eh3d-phoenix-cad -- panels job.eh3d faces.svg
cargo run -p eh3d-phoenix-cad -- check crates/eh3d-phoenix-cad/examples/plate.phoenixproj
cargo run -p eh3d-phoenix-cad -- check crates/eh3d-phoenix-cad/examples/frame.phoenixproj
cargo run -p eh3d-phoenix-cad -- bill crates/eh3d-phoenix-cad/examples/frame.phoenixproj
cargo run -p eh3d-phoenix-cad -- library crates/eh3d-phoenix-cad/examples/chassis.phoenixlib
cargo run -p eh3d-phoenix-cad -- check crates/eh3d-phoenix-cad/examples/assembly.phoenixproj
cargo run -p eh3d-phoenix-cad -- interference crates/eh3d-phoenix-cad/examples/assembly.phoenixproj
cargo run -p eh3d-phoenix-cad -- balance crates/eh3d-phoenix-cad/examples/frame.phoenixproj
cargo run -p eh3d-phoenix-cad -- sketch-export crates/eh3d-phoenix-cad/examples/box.phoenixsketch --format dxf --out box-sketch.dxf
cargo run -p eh3d-phoenix-cad -- holes crates/eh3d-phoenix-cad/examples/box.phoenixproj --tools
cargo run -p eh3d-phoenix-cad -- placements crates/eh3d-phoenix-cad/examples/assembly.phoenixproj --explode 50
cargo run -p eh3d-phoenix-cad -- clearance crates/eh3d-phoenix-cad/examples/assembly.phoenixproj
cargo run -p eh3d-phoenix-cad -- cutplan crates/eh3d-phoenix-cad/examples/frame.phoenixproj --text
cargo run -p eh3d-phoenix-cad -- holes crates/eh3d-phoenix-cad/examples/box.phoenixproj --text --lang es
cargo run -p eh3d-phoenix-cad -- balance crates/eh3d-phoenix-cad/examples/frame.phoenixproj --lang de
cargo run -p eh3d-phoenix-cad -- revolve crates/eh3d-phoenix-cad/examples/plate.phoenixproj --axis y --angle 90 --material PLA
cargo run -p eh3d-phoenix-cad -- export crates/eh3d-phoenix-cad/examples/box.phoenixproj --format stl --out box.stl
cargo run -p eh3d-phoenix-cad -- export crates/eh3d-phoenix-cad/examples/box.phoenixproj --format svg --view front --out box-front.svg
cargo run --release -p eh3d-phoenix-cad --features gui -- crates/eh3d-phoenix-cad/examples/box.phoenixproj
cargo run --release -p eh3d-phoenix-cad --features gui -- gui crates/eh3d-phoenix-cad/examples/box.phoenixproj --shot box.png
cargo run --release -p eh3d-phoenix-cad --features gui -- gui crates/eh3d-phoenix-cad/examples/box.phoenixsketch --view sketch --shot sketch.png
```

## 🧪 テスト

Rust ライブラリには 1802 件のテスト（`cargo test`）、基盤には 4 件のテスト（`python -I -m unittest discover -s tools -p "test_*.py"`）があり、すべてコンピューター上で通っています。CI はリポジトリ チェック、画像のチェック、基盤のテストを実行し、Rust のコードについては書式チェック、警告をエラーとする clippy、Linux と Windows でのテストを実行し、ウィンドウも clippy（`--features gui`）で検査します。 上の件数にはドキュメントの例も含まれ、README の記載が異なると CI が失敗します。

## 🏗️ ビルドとリリース

バージョンは `VERSION` ファイルと `eh3d.project.json` にあり、各バージョンには [CHANGELOG.md](CHANGELOG.md) の項目があります。プログラムは `cargo build --release` でビルドし（ウィンドウは `--features gui` 付き）、リリースは GitHub で公開されます。

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
│   ├── SKETCH.md                                2D スケッチ：拘束、ソルバー、プロジェクトファイル、押し出し
│   ├── HISTORY.md                               グラフとしての履歴、角柱ソリッド、ソルバーのクレート
│   ├── DRAWING.md                               図面シート：ビュー、寸法、バルーン、レイヤー
│   ├── CATALOG.md                               部品カタログ：ライブラリ、部品表、切断リスト
│   ├── ASSEMBLY.md                              簡易アセンブリ：拘束と干渉の検査
│   ├── BOARD_INTERCHANGE.md                     .eh3d ファイル内の基板と筐体
│   ├── EXPORT.md                                エクスポート：STL ソリッド、DXF と SVG の図面
│   └── adr/                                     承認された決定の記録
├── Cargo.toml Cargo.lock                        Rust ワークスペースと固定された依存関係
├── rust-toolchain.toml                          固定されたRustツールチェーン
├── crates/                                      モデル：筐体、部品、スケッチ、履歴、ソルバー、スクリプト、ウィンドウ
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

* 初期段階のコード：あるのは筐体のモデルとそのスケッチ・押し出し・ポケットで、自身のテストが作ったモデルでしか試していません。機械では何も実行しておらず、検証済みの要件はありません。

**次のステップ**

* 基板ツールにプロジェクト形式の機械ドメインを読み書きさせ、STEP ファイルを CAD プログラムで開き、そのあとに 1 つの軸に沿った拘束を超えてアセンブリを拡張し、冷却フィンの推定を実測と比較する。いずれもテストとともに行う。

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
* [EH3D サーバーとの接続: サーバーに保存するプロジェクトとレポート](docs/SERVER.md)
* [スイートのプロジェクト（`.eh3d`）内の基板と筐体](docs/BOARD_INTERCHANGE.md)

## 🤝 コントリビュートとコミュニティ

* プル リクエストの前に `python -I tools/ci_validate.py` と基盤のテストを実行してください。すべて通る必要があります。[CONTRIBUTING.md](CONTRIBUTING.md) と[行動規範](CODE_OF_CONDUCT.md)を参照してください。
* README は 8 言語あります。文章を変更するときは 8 つのファイルすべてを変更し、2 つの図は `docs/architecture.json` と `docs/structure.json` から `tools/make_architecture.py` と `tools/make_structure.py` で描画します。
* 脆弱性は非公開で報告してください：[SECURITY.md](SECURITY.md)。質問、アイデア、報告：electrohobby3d@gmail.com（[SUPPORT.md](SUPPORT.md)）。

## 👤 作者

**JuanenRac (Electro Hobby 3D)** · electrohobby3d@gmail.com

## 📜 ライセンス

ソフトウェア：GPL-3.0-or-later。[LICENSE](LICENSE) を参照。スイートのハードウェア設計には CERN OHL-S を使います。
