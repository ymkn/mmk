# MVP アーキテクチャ

## 構成

MMK は Tauri 2、React、TypeScript、Rust、SQLite で構成する。React は表示と入力、Rust は Kindle データ探索・同期・永続化・URL 起動を担当する。Kindle のパッケージ配下は常に読み取り専用とし、MMK 自身の SQLite だけへ書き込む。

UI は 三ペイン構成。

- 左ペイン: 検索、お気に入り、評価、タグクラウド
- 中央ペイン: 「ライブラリ」と「新刊チェック」のタブ、シリーズカードと単巻カード、表示・並び替え、シリーズ内の巻一覧
- 右ペイン: 選択書籍のスコア、お気に入り、タグ、シリーズ名、巻数などの編集

左右ペインは開閉可能にする。標準幅は左 260px、右 280px とし、中央を可変幅にする。

## データ所有権

Amazon / Kindle 由来値とユーザー入力値は同じ列へ保存しない。

- `books`: Kindle 同期で更新してよい書誌情報と自動推定値
- `user_book_metadata`: スコア、お気に入り、シリーズ設定モード、手動シリーズ名・巻数、手動タイトル・著者・出版社
- `tags`, `book_tags`: ユーザーが管理するタグ
- `amazon_continue_series_items`: Amazon推薦ページから取り込んだ候補。ASIN、タイトル、価格、Amazon画像配信ホストの書影URL、推定シリーズ名・巻数、取得日時だけを保存し、CookieやAmazonのリクエストメタデータは保存しない

同期は `user_book_metadata`、`tags`、`book_tags` を更新・削除しない。Kindle キャッシュから見えなくなった本は削除せず `source_status = 'missing'` にする。

シリーズ編集は独立したシリーズ行を保存するのではなく、その時点でシリーズに属する各書籍の `user_book_metadata` と `book_tags` を一つのトランザクションで更新する。対象は現在の画面フィルターで見えている巻だけではなく、シリーズの全巻とする。シリーズ名の一括変更時は手動シリーズへ切り替え、既存の手動巻数、なければ自動推定巻数を維持する。

## 同期対象

`KindleSyncMetadataCache.xml` の `meta_data` を ASIN 単位にまとめる。`origin/type` が `Sample` または `KindleDictionary` だけの ASIN は登録しない。同一 ASIN に `Sample` と `Purchase` がある場合は購入本として一冊にまとめる。

設定画面で Kindle プロフィールのフォルダーまたは `KindleSyncMetadataCache.xml` のフルパスを指定できる。未指定時は `%LOCALAPPDATA%\Packages` 配下の Kindle パッケージを探索する。指定値は MMK のフロントエンド設定に保存し、Kindle 側のファイルへは書き込まない。

`Purchase`、`KindleUnlimited`、`Prime`、未知または空の origin は登録する。ownership の正規化値と source の生値を両方保存する。

## レーベル

Amazon の出版社フィールドは統合後の法人単位（例: KADOKAWA）に一本化されるため、細かな刊行レーベルはタイトル末尾の括弧書きにしか残らない。同期時にこの括弧を抽出し、Wikipedia「漫画レーベル一覧」と各社別レーベル一覧を基礎に、Wikipedia 未収録の公式デジタルレーベル（Kindle で観測されるもの）も併録した対照表（`src-tauri/src/labels.txt`）に正規化して一致した場合だけ `books.label` へ保存する。対照表は全角半角・空白・`・`・カタカナ/英字表記の揺れを吸収し、Kindle 固有の「デジタル版」接頭辞も許容する。一致しない括弧（数字のみ、特典表記、単行本など）はレーベルとしない。

## 有効値

シリーズの有効値は `series_mode` で決める。

- `auto`: 自動推定値を使用
- `manual`: 手動シリーズ名・巻数を使用
- `none`: シリーズなしとして扱う

手動値が空になったことと、自動推定へ戻したことを区別するため、nullable 列だけで優先順位を表現しない。

スコアは上限・下限のない符号付き整数で、初期値は 0 とする。お気に入りはスコアと独立した真偽値である。

## 読書導線

ASIN がある書籍は Amazon 公式 Web Reader の直接 URL を既定ブラウザで開く。Kindle の主要 XML では `content_type` と `cde_contenttype` だけでは本とマンガを判別できないため、既存の刊行レーベル抽出結果を自動判定に利用する。レーベルがある書籍はマンガとして `https://read.amazon.co.jp/manga/{ASIN}`、それ以外は本として `https://read.amazon.co.jp/?asin={ASIN}` を開く。判定方法は設定画面で「自動判定」「常に本」「常にマンガ」から選択できる。直接 URL が機能しない場合に備え、右ペインから Kindle ライブラリのトップページも開けるようにする。URL 構築は UI と分離し、将来テンプレートを差し替えられるようにする。

## 新刊チェック

Amazonの `ContinueSeriesDRS_V2` ウィジェットを、`widgetId`、`title`、`sourceType=RECS`、`page`、`filters=v1:FORMAT[kindle_edition]`、`cardFeedType=widgetId` の安定パラメータと、端末種別を表す固定の `metadata` で開く。`ccs_id` は開くたびに生成する。`pd_rd_*`、`pf_rd_*`、`ref_` などのリクエスト単位・表示単位の値は固定せず、`metadata` と `ccs_id` も含めて保存しない。

AmazonページのカードにはシリーズIDや購入状態がないため、カードのタイトルからシリーズ名・巻数を推定し、アクティブなKindleライブラリの同シリーズにおける最大巻数より後の候補だけを「新刊チェック」タブへ表示する。通常のライブラリ一覧には候補を混在させない。無料配布やKindle Unlimitedで取得した序盤巻がライブラリに存在する場合も、手元の最大巻数に含める。候補にはAmazonカードの書影を添え、シリーズ名から「ライブラリ」タブの同シリーズ巻一覧へ移動できるようにする。

これはAmazonが示す「未購入」の確定判定ではない。シリーズ名または巻数を推定できない候補、手元のライブラリと照合できない候補は安全側に倒して表示しない。Amazonページはアプリ専用の永続データ領域を使うWebViewで開き、ログイン状態はその領域だけで管理する。ログアウト状態では目的ページからAmazon公式ログイン画面へ非表示のまま遷移してから前面表示し、認証後は目的ページへ戻して候補を取得する。取得完了後はWebViewを閉じる。ログイン済みの場合は画面を表示しない。
