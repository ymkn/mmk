# MMK 開発規則

## ブランチ運用

- 変更作業は必ず main からブランチを切って実施する。main での直接作業・直接コミットはしない。
- ブランチ名は `feature/<内容を表す英字>` の形式とする。

## ビルド検証

- コード変更後は、DebugビルドとReleaseビルドを必ず実施する。
- Debugビルド完了後にReleaseビルドを開始し、同時実行しない。
- 使用するコマンドは `pnpm tauri build --debug --no-bundle`、続けて `pnpm tauri build --no-bundle` とする。
- DebugとReleaseの成否は分けて報告する。ビルド成功は実行時・UI動作確認を意味しないため、未実施の場合はその旨も報告する。
