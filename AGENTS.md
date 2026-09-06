# MMK 開発規則

## ビルド検証

- コード変更後は、DebugビルドとReleaseビルドを必ず実施する。
- Debugビルド完了後にReleaseビルドを開始し、同時実行しない。
- 使用するコマンドは `pnpm tauri build --debug --no-bundle`、続けて `pnpm tauri build --no-bundle` とする。
- DebugとReleaseの成否は分けて報告する。ビルド成功は実行時・UI動作確認を意味しないため、未実施の場合はその旨も報告する。
