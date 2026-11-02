import { newReleaseStatusView } from "./amazonCheckView";
import type { AmazonCheckStatus } from "./types";

export function NewReleaseStatus({ status, savedCount, displayedCount, excludePreRelease, error }: {
  status: AmazonCheckStatus | null;
  savedCount: number;
  displayedCount: number;
  excludePreRelease: boolean;
  error: string;
}) {
  const view = newReleaseStatusView(status, savedCount, error);
  const result = status?.lastSuccess;
  return <div className={`new-release-status${view.failed ? " is-error" : ""}`} role="status" aria-live="polite" aria-atomic="true">
    <strong>{view.title}</strong>
    <span>{view.message}</span>
    {result && <>
      <span>最終取得成功: <time dateTime={result.completedAt}>{new Date(result.completedAt).toLocaleString("ja-JP")}</time></span>
      <span>Amazon取得 {result.receivedCount.toLocaleString()}件 ／ 保存対象 {result.storedCount.toLocaleString()}件 ／ 照合後 {displayedCount.toLocaleString()}シリーズ</span>
    </>}
    {!result && savedCount > 0 && <span>保存済み候補 {savedCount.toLocaleString()}件（取得成功日時は不明）</span>}
    {view.showingPrevious && <span>表示している候補と件数は保存済みの結果です。今回の取得結果ではありません。</span>}
    <span className="muted">発売前の新刊を除外: {excludePreRelease ? "有効（評価表示のない候補を除外）" : "無効"}</span>
  </div>;
}
