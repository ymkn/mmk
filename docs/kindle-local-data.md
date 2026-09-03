# Kindle ローカルデータ調査

調査日: 2026-09-03
対象: Microsoft Store / MSIX 版 Kindle for Windows

## 調査境界

- Kindle パッケージ配下は読み取り専用で調査した。
- KFX / AZW / MOBI 等の本文候補は開いていない。
- Amazon の credential、cookie、本文、書名、著者名、ASIN 実値は記録していない。
- `TempState\Webview` と `AC\INetCookies` は走査対象外とした。
- 実行結果 JSON は絶対パス等を含むため `artifacts/` にのみ保存し、Git の追跡対象外とする。

## Package path

実環境で検出した Package Family Name は次の形式だった。

```text
%LOCALAPPDATA%\Packages\AMZNKindle.AmazonKindleReadingApp_m1sc522ngdk36
```

`AMZNKindle.AmazonKindleReadingApp\_m1sc522ngdk36` のような二階層ではなく、アプリ名と接尾辞を連結した一つのディレクトリである。調査 CLI は誤って二階層に分けた入力も補正する。

実環境での CLI 結果は、Kindle package 1 件、metadata file 40 件、SQLite database 10 件、cover directory 1 件、candidate file 57 件だった。WebView 候補、本文候補、SQLite inspection error はいずれも 0 件である。JSON の大半は session breadcrumb であり、蔵書メタデータ源としては採用しない。

## Metadata source candidates

最有力候補は次の XML である。

```text
%LOCALAPPDATA%\Packages\AMZNKindle.AmazonKindleReadingApp_m1sc522ngdk36\LocalState\Classic\Data\Cache\KindleSyncMetadataCache.xml
```

- サイズ: 約 3.1 MB
- XML root: `response`
- `/response/add_update_list/meta_data`: 3,651 レコード
- ASIN のユニーク数: 3,647

新アプリのパッケージ内に `LocalState\Classic` があり、従来形式の XML キャッシュは現時点でも生成されている。ただし内部仕様であり、将来も維持される保証はない。

補助候補として SQLite データベース、JSON、独自拡張子のキャッシュが存在する。テーブル名などは調査 CLI のローカル JSON 出力で確認する。

## Sample schema

実値を除いた XML 構造は次の通りである。

```xml
<response>
  <add_update_list>
    <meta_data>
      <ASIN />
      <authors><author /></authors>
      <cde_contenttype />
      <content_type />
      <origins><origin><type /></origin></origins>
      <publication_date />
      <publishers><publisher /></publishers>
      <purchase_date />
      <textbook_type />
      <title />
    </meta_data>
  </add_update_list>
  <cache_metadata><version /></cache_metadata>
  <sync_time />
</response>
```

## Usable fields

| Field | 非空件数 / 3,651 | 判断 |
|---|---:|---|
| `ASIN` | 3,651 | 主識別子候補。4 ASIN が各 2 レコードずつ存在するため upsert 前の重複処理が必要 |
| `title` | 3,651 | 利用可能 |
| `authors/author` | 3,651 | 複数著者を保持可能。著者要素は合計 7,377 件 |
| `publishers/publisher` | 3,543 | nullable で利用可能 |
| `publication_date` | 3,545 | ISO 8601 datetime。nullable で利用可能 |
| `purchase_date` | 3,637 | ISO 8601 datetime。acquisition date 候補だが名称どおり購入日と断定できないケースに注意 |
| `cde_contenttype` | 3,651 | `EBOK` 3,645 件、`EBSP` 6 件 |
| `content_type` | 3,651 | 全件 `application/x-mobipocket-ebook` |
| `origins/origin/type` | 3,637 | ownership 分類の最有力材料 |
| `textbook_type` | 1 | ほぼ利用不能 |

## Ownership classification

`origins/origin/type` の観測値は次の通りだった。

| Type | 件数 | 暫定マッピング |
|---|---:|---|
| `Purchase` | 3,597 | `purchased` 候補 |
| `KindleUnlimited` | 20 | `subscription` 候補 |
| 空 | 14 | `unknown` |
| `Prime` | 10 | `subscription` 候補 |
| `Sample` | 6 | `sample` 候補 |
| `KindleDictionary` | 4 | 専用種別として保持、または `unknown` |

このフィールドは有用だが、Amazon の公開仕様ではない。未知値は `unknown` とし、元の文字列も保存するのが妥当である。Personal Document を示す値は今回のデータでは確認できなかったため、判別可否は未確認。

重複した 4 ASIN は、いずれも同じ title / author / publication date に対して `Sample` と `Purchase` が一件ずつ存在した。Phase 1 では ASIN 単位にまとめ、`Purchase` を現在の ownership として優先しつつ、観測した origin の履歴は失わない設計が必要である。

## Kindle collections

次の SQLite DB は Kindle 側コレクションの参照元として利用できる。

```text
%LOCALAPPDATA%\Packages\AMZNKindle.AmazonKindleReadingApp_m1sc522ngdk36\LocalState\Classic\Data\Cache\db\synced_collections.db
```

- `cloud_collections`: 8 行。`id`, `name`, `sync_count`, `offline_status`, `last_updated_timestamp`
- `cloud_collections_items`: 2,542 行。`book_asin`, `collection_id`, `resource_type`, `sync_count`, `offline_status`, `last_updated_timestamp`

コレクション名の実値は記録していない。`book_asin` で主要 XML と結合できる構造である。Kindle 側へ書き戻さず、参考情報として import する adapter を別に設けるのが妥当である。

`Classic\Content\book_asset.db` には `Book`、`Asset`、`BookDownloadInfo`、`AssetDownloadInfo` 等のテーブルがあるものの、今回の確認時点では主要 4 テーブルはいずれも 0 行だった。全蔵書の書誌情報源には使えない。

## Series / subtitle / language

今回の XML スキーマには `series`、`series_index`、`subtitle`、`language` に相当する要素が見つからなかった。タイトル文字列からの推測は行わない。

## Cover metadata

表紙キャッシュ:

```text
%LOCALAPPDATA%\Packages\AMZNKindle.AmazonKindleReadingApp_m1sc522ngdk36\LocalState\Classic\Data\Cache\covers
```

- JPEG: 3,647 ファイル
- ファイル名: 32 桁の hex
- 対応規則: `uppercase(hex(MD5(UTF-8(ASIN)))) + ".jpg"`
- 3,647 個のユニーク ASIN すべてについて完全一致した

MD5 はセキュリティ用途ではなく、既存キャッシュ名の決定規則としてのみ使う。

## 推奨 parser

Phase 1 の第一 adapter は `KindleSyncMetadataXmlSource` とする。

1. Package Family Name を列挙して `LocalState\Classic\Data\Cache\KindleSyncMetadataCache.xml` を検出する。
2. XML は外部 DTD / entity を無効にした streaming parser で読む。
3. `meta_data` を ASIN 単位に正規化する。`Sample` と `Purchase` が重複した場合は `Purchase` を現在状態として優先し、観測した origin は別途保持する。
4. 未知要素は無視し、既知フィールドの欠損はエラーにしない。
5. `origin/type` の生値を保存し、既知値だけ ownership type へ写像する。
6. 表紙は ASIN の MD5 からローカル JPEG を引く。欠損しても書籍同期は継続する。

SQLite は `KindleCollectionsSqliteSource` として補助情報源にできる。XML で不足する series 等を補えるデータ源は現時点で見つかっていない。

## Unusable / uncertain fields

- `purchase_date` が購入以外の取得経路でも acquisition date として正しいかは未確認。
- 同一 ASIN の重複 4 件は Sample と Purchase の併存だった。なぜ古い Sample 行が残るかは未確認。
- `source_status` を判定する削除リストが XML に含まれるかは未確認。
- series、subtitle、language は主要 XML から取得できない。
- Kindle 内コレクションは `synced_collections.db` に候補があるが、書籍との対応関係は未確認。

## 壊れやすい依存箇所

- `LocalState\Classic` という内部ディレクトリ名
- `KindleSyncMetadataCache.xml` のファイル名と XML path
- `origin/type` の列挙値
- ASIN と表紙ファイル名の MD5 対応規則
- ライブ更新中の XML / SQLite を読む際の一時的不整合

parser は source adapter とし、検出失敗や parse error が起きても、将来のユーザー DB や自前タグを利用不能にしない。
