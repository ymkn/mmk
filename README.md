# Manga Manager for Kindle (MMK)

![](imgs/screenshot-main.png)

Windows 版 Kindle のローカルメタデータを読み取り、自分用のタグ、評価スコア、お気に入り、シリーズを管理するデスクトップアプリです。Kindle 側へは書き込まず、読書時は Amazon 公式 Web Reader を既定ブラウザで開きます。

主な機能:

- Kindle ライブラリの同期（公式Kindleアプリのメタデータを参照）
- タイトル・著者・タグ・ASIN の検索
- お気に入り、正負の評価スコア、タグ
- タイトルからのシリーズ推定とシリーズ単位の棚表示
- メタデータ（タイトル、著者、出版社、シリーズ名、巻数）は手動で管理可能
- ログインするとシリーズの新刊情報を取得可能（Amazon の「読み始めたシリーズの続きを読むにはこちら」がソース）、ストアページを開くリンク付き

> [!tip]
> 新刊チェック機能を除き、このアプリがAmazonへの認証情報を持つことはありません。新刊チェック機能を使う場合はアプリ内蔵ブラウザ経由でユーザがAmazonにログインする必要があり、その結果の認証情報（Cookie）はこのアプリで保持します。この場合においても、ID/メールアドレスやパスワードをこのアプリが参照・保有することはありません。

## 動作環境

- Windows 11以降
- Microsoft Edge WebView2 Runtime（Windows 11 には標準で同梱）
- Kindle 公式アプリが導入され、ログイン済みであること

## 利用方法

ReleasesからダウンロードしたZIPを展開し、mmk.exeを実行、「Kindleと同期」ボタンを押してください。

公式アプリが古い場合や設定を変えている場合、同期に失敗することがあります。その場合は歯車アイコンから設定ペインを開き、`KindleSyncMetadataCache.xml`の場所を設定してください。

## 開発環境

- Windows 11
- Node.js 22.5 以降
- pnpm 11 以降
- Rust 1.77.2 以降
- Microsoft C++ Build Tools

ビルド済み exe を実行するだけの場合、Node.js、pnpm、Rust、Microsoft C++ Build Tools は不要です。Windows の WebView2 Runtime は必要です。Windows 11 には通常インストール済みですが、起動できない場合は Microsoft Edge WebView2 Runtime を導入してください。

## セットアップと起動

```powershell
pnpm install
pnpm tauri dev
```

初回起動後に「Kindleと同期」を押すと、`%LOCALAPPDATA%\Packages` から Kindle パッケージを検出します。Kindle のプロファイルを別の場所へ移している場合は、設定画面の「Kindleプロフィール」に `KindleSyncMetadataCache.xml` を含むフォルダー、または XML ファイルのフルパスを指定できます。MMK の DB は Tauri の app data directory に `mmk.sqlite3` として保存されます。

「新刊チェック」はAmazonのページをアプリ専用WebViewで開きます。ログイン状態はそのWebViewの専用データ領域にのみ保存し、Chrome等のCookieを読み取ったり、Cookie文字列をMMKのDBへ保存したりしません。「Amazon連携を解除」を押すと、専用WebViewの閲覧データと取得済み候補を削除します。

Release 実行ファイルの生成:

```powershell
pnpm build:exe
```

出力は `src-tauri\target\release\mmk.exe` です。
この exe はフロントエンド資産と SQLite を内包しているため、`dist` や Node.js を同じ場所へ置かずに起動できます。Tauri の Windows 実行基盤である WebView2 Runtime だけは OS 側の依存関係として残ります。

## テスト

```powershell
pnpm build
cargo test --manifest-path src-tauri\Cargo.toml
```

実ユーザーの Kindle キャッシュを読み取り専用で parser に通す確認:

```powershell
cargo test --manifest-path src-tauri\Cargo.toml live_kindle_cache_parses_without_personal_output -- --ignored
```

設計文書は [docs/README.md](docs/README.md) から参照できます。

## 免責

本ソフトウェアは Amazon によって作成・承認・後援されたものではありません。Amazon、Kindle および関連するロゴ・名称は、それぞれの権利者に帰属します。