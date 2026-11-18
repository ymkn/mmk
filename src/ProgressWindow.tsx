import { useEffect, useState } from "react";
import { api } from "./api";
import type { CaptureStatus } from "./types";

// 進捗が止まっている可能性を示すまでの猶予。未ダウンロード本の
// ダウンロード中など、正当に進捗が出ないフェーズがあるため長めに取る。
const STALL_THRESHOLD_MS = 5 * 60 * 1000;

function elapsedLabel(ms: number): string {
  const seconds = Math.floor(ms / 1000);
  if (seconds < 10) return "たった今";
  if (seconds < 60) return `${seconds}秒前`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}分前`;
  return `${Math.floor(minutes / 60)}時間前`;
}

type Tone = "running" | "stalled" | "error" | "done" | "idle";

export function ProgressWindow() {
  const [status, setStatus] = useState<CaptureStatus | null>(null);
  const [pollError, setPollError] = useState("");
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    document.body.classList.add("progress-view");
    return () => document.body.classList.remove("progress-view");
  }, []);

  useEffect(() => {
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    let nextDelay = 1000;
    const poll = async () => {
      try {
        const result = await api.pageCaptureStatus();
        if (disposed) return;
        setStatus(result);
        setPollError("");
        // 実行中は1秒、待機・完了後は5秒に落として無駄なポーリングを減らす。
        nextDelay = result.running ? 1000 : 5000;
      } catch (e) {
        if (!disposed) setPollError(String(e));
      }
      if (!disposed) timer = setTimeout(poll, nextDelay);
    };
    void poll();
    const tick = setInterval(() => setNow(Date.now()), 1000);
    return () => { disposed = true; clearTimeout(timer); clearInterval(tick); };
  }, []);

  const running = status?.running === true;
  // 最終進捗時刻は Rust 側が付けたものを使う（ウィンドウを開いた
  // 時点から測り始めると、既に止まっているケースを見逃すため）。
  const lastProgressAt = status?.progressUpdatedAt ?? null;
  const stalled = running && lastProgressAt !== null && now - lastProgressAt >= STALL_THRESHOLD_MS;
  const batchDone = !running && status !== null && status.batch && status.totalBooks > 0
    && status.completedBooks + status.skippedBooks + status.failedBooks + status.textBooks >= status.totalBooks;

  let tone: Tone;
  let label: string;
  // logError は操作ログ側のエラーでバックアップ自体の失敗ではないため、
  // 状態ラベルには使わず下部の行でそのまま見せる。
  if (pollError) { tone = "error"; label = "状態を取得できません"; }
  else if (stalled) { tone = "stalled"; label = "進捗が止まっているかも"; }
  else if (running) { tone = "running"; label = "バックアップ中"; }
  else if (batchDone) { tone = "done"; label = (status?.failedBooks ?? 0) > 0 ? "完了（失敗あり）" : "完了"; }
  else if (!running && status?.batch && status.totalBooks > 0) { tone = "idle"; label = "停止"; }
  else { tone = "idle"; label = "待機中"; }

  return (
    <main className="progress-window" data-tone={tone}>
      <div className="progress-status-line">
        <span className="progress-dot" aria-hidden="true" />
        <strong className="progress-state">{label}</strong>
        {running && lastProgressAt !== null && <span className="progress-updated">最終進捗 {elapsedLabel(now - lastProgressAt)}</span>}
      </div>
      {running && status?.currentTitle
        ? <p className="progress-title">{status.currentTitle}</p>
        : status?.message && <p className="progress-title">{status.message}</p>}
      {status?.batch && status.totalBooks > 0 && (
        <p className="progress-counts">
          完了 {status.completedBooks}冊・スキップ {status.skippedBooks}冊・失敗 {status.failedBooks}冊{status.textBooks > 0 && <>・対象外 {status.textBooks}冊</>}／全{status.totalBooks}冊
        </p>
      )}
      {status && (
        <p className="progress-pages">
          {status.savedPages}枚保存済み
          {status.currentPosition != null && status.totalPositions != null && (
            <>（Location {status.currentPosition} / {status.totalPositions}）</>
          )}
        </p>
      )}
      {status && status.failedTitles.length > 0 && (
        <p className="progress-failed">失敗: {status.failedTitles.slice(0, 3).join("、")}{status.failedTitles.length > 3 ? " ほか" : ""}</p>
      )}
      {(status?.logError || pollError) && <p className="progress-error">{status?.logError || pollError}</p>}
    </main>
  );
}
