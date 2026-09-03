# Hondana

Kindle のローカル・ライブラリ情報を、Kindle 側へ書き込まずに調査する Phase 0 ツールです。
製品 UI、本文閲覧、本文解析、DRM 処理はまだ実装していません。

## 必要環境

- Windows 11
- Node.js 22.5 以降

外部 npm パッケージは使用しません。

## 実行

既定では `%LOCALAPPDATA%\\Packages` 直下の Kindle パッケージを検出します。

```powershell
node --experimental-sqlite tools/kindle-discovery.mjs --pretty
```

パッケージを明示し、ローカル成果物へ保存する場合:

```powershell
node --experimental-sqlite tools/kindle-discovery.mjs `
  --root "$env:LOCALAPPDATA\\Packages\\AMZNKindle.AmazonKindleReadingApp_m1sc522ngdk36" `
  --output "artifacts\\kindle-discovery.json" `
  --pretty
```

`artifacts/` は Git の追跡対象外です。調査結果には絶対パスなど利用者固有の情報が含まれるため、コミットしないでください。

## テスト

```powershell
node --experimental-sqlite --test
```

`package.json` にも同じ script を定義しているため、npm が正常な環境では `npm run discover:kindle` と `npm test` でも実行できます。

調査結果と次フェーズの判断は [docs/kindle-local-data.md](docs/kindle-local-data.md) に記録しています。
