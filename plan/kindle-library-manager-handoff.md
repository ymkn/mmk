# Kindle Library Manager — Codex引き継ぎメモ

## 1. 目的

自分が購入・所有している Kindle 本を、Amazon / Kindle 側の貧弱なライブラリ管理機能とは独立して整理・検索できるアプリケーションを作る。

Kindle 本そのものの閲覧機能は実装しない。
読書時は **Kindle for Web / Kindle Cloud Reader 相当の Amazon 公式 Web リーダー**を開く。

このアプリが担当するのは以下のみ。

- 所有している Kindle 本のメタデータ収集
- 自前のタグ付け
- 自前のコレクション / フォルダ分け
- 書誌情報検索
- 絞り込み・並び替え
- メモ、評価、読書状態などのユーザー独自メタデータ
- Amazon の Web リーダーへの導線

本文データの抽出・変換・DRM解除は行わない。

---

## 2. UX / プラットフォーム方針

「Webアプリ」という表現は、ブラウザ的な UI / UX を使いたいという意味であり、ブラウザ単体で完結させる必要はない。

ローカル Kindle アプリのデータにアクセスするためにネイティブ層が必要なら、普通に設けてよい。

### 推奨構成

- **Tauri 2**
- フロントエンド: React + TypeScript
- ネイティブ層: Rust
- ローカルDB: SQLite
- ORM / Query:
  - Rust側で `sqlx`
  - または `rusqlite`
- UI:
  - Vite
  - React Router
  - 任意の軽量 UI コンポーネントライブラリ

理由:

- UI は Web 技術で高速に実装・修正できる
- Windows のローカルファイルへ自然にアクセスできる
- Electron より軽量にできる
- 将来 macOS 等へ展開する余地も残る
- Kindle 本文を扱わないため、Windows 固有 API への依存は比較的小さい

初期 MVP は Windows を主対象としてよい。

---

## 3. 基本アーキテクチャ

### Frontend

担当:

- ライブラリ一覧
- 書籍詳細
- 検索
- タグ管理
- コレクション管理
- フィルタ
- ソート
- Kindle で読む
- メタデータ同期実行
- 同期結果 / エラー表示

### Native Backend

担当:

- Kindle のローカルデータ探索
- Kindle メタデータキャッシュの解析
- SQLite の読み書き
- Amazon由来メタデータの upsert
- ユーザーメタデータの保存
- 表紙画像キャッシュ探索
- URL生成
- 既定ブラウザ起動

### Database

Amazon / Kindle 由来情報と、ユーザーが手で付与した情報を明確に分離する。

Kindle 側の同期によって、自前タグ等が消えないことを最重要とする。

---

## 4. Kindle メタデータ取得方針

### 原則

Kindle 本文ファイルを解析するのではなく、Kindle デスクトップアプリが保持している **ライブラリ用メタデータキャッシュ** を優先して利用する。

想定候補:

- `KindleSyncMetadataCache.xml`
- Kindle アプリ配下の XML
- JSON
- SQLite
- 表紙キャッシュ
- その他ライブラリ管理用ローカルキャッシュ

Windows Store / MSIX 版では `AppData\Local\Packages` 以下を探索対象に含める。

既知候補例:

```text
%LOCALAPPDATA%\Packages\
  AMZNKindle.AmazonKindleReadingApp_*\LocalState\Classic\Data\Cache\
  KindleSyncMetadataCache.xml
```

ただし、このパスをハードコードして唯一の取得元とはしない。

### Discovery 層を作る

以下のような抽象化を推奨する。

```ts
interface KindleMetadataSource {
  id: string;
  detect(): Promise<DetectedSource | null>;
  load(): Promise<RawKindleBook[]>;
}
```

Rust側では trait として実装してよい。

例:

- `KindleSyncMetadataXmlSource`
- 将来必要なら `KindleSqliteSource`
- 将来必要なら `KindleJsonSource`

アプリ起動時、またはユーザーが「同期」を押したときに利用可能な source を検出する。

---

## 5. 取得したい Kindle 側メタデータ

優先度高:

- ASIN / content ID
- title
- subtitle
- author
- authors
- publisher
- publication date
- purchase / acquisition date
- content type
- series name
- series index
- language
- cover image / cover cache reference

補助:

- last access
- downloaded / not downloaded
- sample / purchased / subscription 等を推定できる情報
- source-specific identifier
- Kindle 内コレクション情報が取れるなら参考情報として保持

すべて取れるとは限らないので nullable 前提。

---

## 6. データモデル案

### `books`

Amazon / Kindle 由来の正規化済み書誌情報。

```sql
CREATE TABLE books (
    id                INTEGER PRIMARY KEY,
    asin              TEXT UNIQUE,
    source_content_id TEXT,
    title             TEXT NOT NULL,
    subtitle          TEXT,
    authors_json      TEXT,
    publisher         TEXT,
    publication_date  TEXT,
    acquisition_date  TEXT,
    content_type      TEXT,
    series_name       TEXT,
    series_index      REAL,
    language          TEXT,
    cover_path        TEXT,
    first_seen_at     TEXT NOT NULL,
    last_seen_at      TEXT NOT NULL,
    source_status     TEXT NOT NULL DEFAULT 'active'
);
```

### `tags`

```sql
CREATE TABLE tags (
    id   INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE
);
```

### `book_tags`

```sql
CREATE TABLE book_tags (
    book_id INTEGER NOT NULL,
    tag_id  INTEGER NOT NULL,
    PRIMARY KEY (book_id, tag_id)
);
```

### `collections`

階層化可能にする。

```sql
CREATE TABLE collections (
    id        INTEGER PRIMARY KEY,
    parent_id INTEGER,
    name      TEXT NOT NULL
);
```

### `book_collections`

```sql
CREATE TABLE book_collections (
    book_id       INTEGER NOT NULL,
    collection_id INTEGER NOT NULL,
    PRIMARY KEY (book_id, collection_id)
);
```

### `user_book_metadata`

Amazon 側同期で絶対に上書きしない領域。

```sql
CREATE TABLE user_book_metadata (
    book_id       INTEGER PRIMARY KEY,
    rating        INTEGER,
    reading_state TEXT,
    favorite      INTEGER NOT NULL DEFAULT 0,
    memo          TEXT,
    updated_at    TEXT NOT NULL
);
```

`reading_state` の候補:

- unread
- reading
- completed
- paused
- dropped

---

## 7. 同期仕様

同期処理では Amazon / Kindle 由来の書誌情報だけを更新する。

### Upsert

ASIN が取れる場合は ASIN を主要な同一性キーとして扱う。

ASIN がない Kindle Personal Document 等については、別の content ID を使用する。

### 削除しない

Kindle ローカルキャッシュから消えた本を DB から即 DELETE しない。

理由:

- アプリ側キャッシュ都合
- 再ログイン
- Kindle Unlimited
- サンプル
- 一時的な同期状態
- Amazon 側仕様変更

などで一時的に見えなくなる可能性があるため。

同期時:

```text
今回確認できた
    -> source_status = active
    -> last_seen_at 更新

以前は存在したが今回確認できない
    -> source_status = missing
```

必要ならユーザー操作で archive 可能にする。

---

## 8. 所有状態について

「ローカルキャッシュに存在する」ことと「永久購入済み」は必ずしも同義ではない。

Kindle Unlimited、サンプル、Personal Document 等が混在する可能性がある。

そのため最初から以下のような概念を分離しておく。

```text
content_type
ownership_type
source_status
```

`ownership_type` は取得情報から確定できない場合 `unknown` とする。

候補:

- purchased
- subscription
- sample
- personal_document
- unknown

推測で purchased と断定しない。

---

## 9. 表紙

MVP では必須ではないが、一覧 UX 上はかなり重要。

優先順位:

1. Kindle ローカルの表紙キャッシュ
2. Kindle メタデータ内の表紙 URL / reference
3. ASIN から取得可能な Amazon 側画像
4. プレースホルダー

ネットワーク依存を減らすため、取得した表紙は自前キャッシュしてよい。

表紙取得失敗で書籍同期全体を失敗させないこと。

---

## 10. Kindle で読む

ユーザーが本を選択して「Kindleで読む」を押したら Amazon 公式 Web リーダーを既定ブラウザで開く。

URL生成は独立モジュールにする。

```ts
interface KindleReaderUrlBuilder {
  build(book: Book): string | null;
}
```

Amazon 側 URL は変更される可能性があるため、UIやDBロジックに埋め込まない。

日本 Amazon を初期対象とする。

ASIN を利用できる場合は、それを元に Kindle for Web の該当書籍へ遷移する URL を生成する。

URL仕様が不安定な場合は:

- 設定でテンプレート変更可能
- fallback として Kindle ライブラリページを開く

構成にしておく。

---

## 11. MVP UI

### Library

デフォルト画面。

表示モード:

- Cover Grid
- Compact List

表示項目:

- 表紙
- タイトル
- 著者
- シリーズ
- タグ
- 読書状態

操作:

- Kindleで読む
- タグ追加
- コレクション追加
- favorite
- 詳細表示

### Search

最低限:

- タイトル
- 著者
- 出版社
- シリーズ
- ASIN
- 自前タグ
- 自前メモ

SQLite FTS5 の利用を検討する。

### Filter

- タグ
- コレクション
- 著者
- シリーズ
- 出版社
- acquisition date
- publication date
- reading state
- favorite
- content type
- ownership type

### Sort

- title
- author
- acquisition date
- publication date
- series
- last seen

---

## 12. コレクションとタグの扱い

用途を分離する。

### Collection

人間が本棚として整理する階層構造。

例:

```text
技術書
  ├─ Programming
  ├─ Electronics
  └─ 3D CAD

漫画
  ├─ 完結
  └─ 連載中
```

UI上では tree 表現してよい。

### Tag

横断的な属性。

例:

```text
積読
再読したい
資料
お気に入り
キーボード
Rust
```

1冊に複数付与。

---

## 13. 技術的な重要制約

### Kindle データ構造は内部仕様

Amazon が公開 API として保証しているものではない。

したがって:

- パスを1か所にハードコードしない
- parser を source adapter 化する
- parse error をログへ残す
- 未知フィールドを無視できるようにする
- Kindle アプリ更新で壊れても管理DB自体は利用可能にする

### ユーザーDBは Kindle と独立

Kindle が壊れても、

- タグ
- Collection
- rating
- memo
- reading state

は失われないこと。

---

## 14. 非目標

少なくとも初期版では行わない。

- Kindle 本文の閲覧
- DRM解除
- KFX / AZW / MOBI の変換
- Amazon 認証情報の取得
- Amazon Cookie の保存
- Amazonへの自動ログイン
- 書籍本文全文検索
- Kindle へのタグ / Collection 書き戻し
- Kindle アプリ内部DBの変更

原則として Kindle 側は read-only。

---

## 15. セキュリティ

- Kindle キャッシュは読み取り専用
- Amazon の credential / cookie は取得しない
- 自前DBはローカル保存
- 外部通信は表紙取得等、必要最小限
- 本文ファイルは原則として開かない
- Kindle app directory にファイルを書き込まない

---

## 16. 実装フェーズ

### Phase 0 — Kindle ローカルデータ調査

最優先。

Windows 上で Kindle 関連ディレクトリを探索し、実際に存在するデータを確認する。

候補検索:

```powershell
Get-ChildItem "$env:LOCALAPPDATA\Packages" `
  -Filter "KindleSyncMetadataCache.xml" `
  -Recurse -ErrorAction SilentlyContinue |
  Select-Object FullName, Length, LastWriteTime
```

あわせて Kindle パッケージ配下の:

- XML
- JSON
- SQLite / DB
- cover cache

を列挙する。

このフェーズではファイルを書き換えない。

### Phase 1 — Metadata Parser

- Kindle cache 検出
- XML parse
- ASIN
- title
- author
- publisher
- acquisition date

程度を `RawKindleBook` に変換。

CLI / unit test だけでよい。

### Phase 2 — SQLite

- migration
- books upsert
- missing detection
- user metadata 分離

### Phase 3 — Tauri UI

- library grid
- search
- detail
- sync button
- Kindle で読む

### Phase 4 — 管理機能

- tag
- collection
- reading state
- favorite
- memo

### Phase 5 — 表紙

- local cache discovery
- fallback
- image cache

### Phase 6 — UX強化

- FTS5
- saved filter
- bulk edit
- drag & drop collection
- keyboard navigation

---

## 17. 最初に Codex に依頼する作業

まずアプリ全体を作り始めず、**Phase 0 のデータ調査ツールを作ること。**

### Task

Windows 上にインストールされている Kindle アプリのローカルデータを read-only で探索するスクリプトまたは小型 CLI を作る。

出力したいもの:

```json
{
  "kindlePackages": [],
  "metadataFiles": [],
  "databaseFiles": [],
  "coverDirectories": [],
  "candidateFiles": []
}
```

候補ファイルについて:

- absolute path
- extension
- size
- last modified
- XML の root element
- SQLite なら table names
- JSON なら top-level structure

まで調べる。

**本文と思われる KFX / AZW 等は内容解析しない。**

目的は、現在利用している Kindle Windows アプリから蔵書メタデータを安定して取得できるデータ源を特定すること。

---

## 18. Phase 0 完了条件

以下が判明すれば次へ進む。

1. ローカル Kindle ライブラリメタデータの所在地
2. 全所有本、または十分な割合の本を列挙できるか
3. ASIN が取得できるか
4. title / author が取得できるか
5. acquisition date が取得できるか
6. series 情報が存在するか
7. content type を判定できるか
8. cover との対応付けが可能か
9. Kindle Unlimited / sample / personal document の判別材料があるか
10. Kindle アプリ更新に対してどこまで安定しそうか

---

## 19. 重要な設計判断

- 本文閲覧は Amazon 公式 Kindle Web Reader に委譲する。
- 自作アプリはライブラリ管理に専念する。
- ブラウザ単体アプリにはこだわらない。
- Kindle ローカル情報へのアクセス用にネイティブ層を設けてよい。
- UI は Web 技術で実装する。
- 第一候補は Tauri + React + TypeScript + Rust + SQLite。
- Kindle ローカルデータは read-only。
- Amazon / Kindle 由来メタデータとユーザー独自メタデータを分離する。
- Kindle 側で本が見えなくなっても、自前DBから即削除しない。
- DRM解除や本文解析は対象外。
- まず現在の Kindle Windows アプリのローカルデータ構造を実測してから parser を決める。

---

## 20. Codex向け開始指示

この文書を仕様の初期版として扱うこと。

最初のコミットでは製品 UI を作らず、Phase 0 の Kindle local metadata discovery を実施すること。

調査結果を `docs/kindle-local-data.md` にまとめ、次を記録する。

- Kindle package path
- metadata source candidates
- sample schema
- usable fields
- unusable / uncertain fields
- cover metadata
- ownership classification の可否
- 推奨 parser
- 今後壊れそうな依存箇所

実ファイルや個人の Kindle ライブラリ内容はリポジトリへコミットしないこと。
