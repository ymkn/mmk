import { useEffect, useRef, useState } from "react";
import { api } from "./api";
import type { Book, CaptureStatus } from "./types";

export function BackupDialog({ books, open, batch, outputDirectory, onClose }: {
  books: Book[]; open: boolean; batch: boolean; outputDirectory: string; onClose: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const rescanSeq = useRef(0);
  const refreshSeq = useRef(0);
  const [accepted, setAccepted] = useState(false);
  const [pendingIds, setPendingIds] = useState<Set<number>>(new Set());
  const [status, setStatus] = useState<CaptureStatus | null>(null);
  const [engine, setEngine] = useState<"web" | "desktop">("web");
  const [loading, setLoading] = useState(true);
  const [pending, setPending] = useState(false);
  const [rescanning, setRescanning] = useState(false);
  const [rescanNote, setRescanNote] = useState("");
  const [indexReady, setIndexReady] = useState(false);
  const [error, setError] = useState("");
  const bookKey = books.filter((book) => book.sourceStatus === "active").map((book) => book.id).join(",");
  // Until the backup index is ready, the pending list is provisional
  // (every candidate reported as pending) and must not be acted on.
  const targets = (indexReady ? books.filter((book) => book.sourceStatus === "active" && pendingIds.has(book.id)) : []).sort((a, b) =>
    (a.effectiveSeriesName ?? a.title).localeCompare(b.effectiveSeriesName ?? b.title, "ja") ||
    (a.effectiveSeriesIndex ?? Infinity) - (b.effectiveSeriesIndex ?? Infinity) || a.title.localeCompare(b.title, "ja"));
  const locked = loading || pending || rescanning || !indexReady || status?.running === true;
  // The rescan button doubles as the retry path when the initial
  // index preparation failed, so it must stay usable then.
  const rescanLocked = loading || pending || rescanning || status?.running === true;
  // The desktop engine applies to batch backups only; single-book capture
  // always uses the web reader.
  const displayEngine = batch ? engine : "web";

  useEffect(() => {
    if (!open) { dialog.current?.close(); return; }
    setAccepted(false); setError(""); setLoading(true); setPendingIds(new Set()); setIndexReady(false);
    rescanSeq.current += 1; setRescanning(false); setRescanNote("");
    void api.getBackupSettings().then((s) => setEngine(s.engine)).catch(() => {});
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    let retryTimer: ReturnType<typeof setTimeout>;
    let wasRunning = false;
    const refreshStartedAt = Date.now();
    const refreshTargets = async (): Promise<void> => {
      // One chain only: an immediate refresh supersedes any
      // scheduled retry instead of running alongside it.
      clearTimeout(retryTimer);
      const seq = ++refreshSeq.current;
      try {
        const result = await api.listPendingBackups(outputDirectory, bookKey ? bookKey.split(",").map(Number) : []);
        if (disposed || seq !== refreshSeq.current) return;
        if (result.indexReady) {
          setIndexReady(true);
          setPendingIds(new Set(result.pending));
          setLoading(false);
          return;
        }
        // The one-time index scan is still running in the
        // background; poll until it lands instead of blocking.
        if (result.scanError) {
          setError(`バックアップ記録の準備に失敗しました: ${result.scanError}`);
          setLoading(false);
          return;
        }
        if (Date.now() - refreshStartedAt > 10 * 60 * 1000) {
          setError("バックアップ記録の準備に時間がかかっています。保存先の応答が遅い可能性があります。「記録を再スキャン」で再試行できます。");
          setLoading(false);
          return;
        }
        retryTimer = setTimeout(() => { void refreshTargets(); }, 2000);
      } catch (e) {
        if (!disposed && seq === refreshSeq.current) { setError(String(e)); setLoading(false); }
      }
    };
    const poll = async () => {
      try {
        const result = await api.pageCaptureStatus();
        if (disposed) return;
        setStatus(result);
        if (wasRunning && !result.running) await refreshTargets();
        wasRunning = result.running;
      } catch (e) { if (!disposed) setError(String(e)); }
      if (!disposed) timer = setTimeout(poll, 500);
    };
    void refreshTargets();
    void poll();
    try {
      dialog.current?.showModal();
    } catch (e) {
      // Already open (an effect re-run) is fine; anything else is
      // surfaced instead of stranding the dialog.
      if (!(e instanceof DOMException && e.name === "InvalidStateError")) {
        setError(`ダイアログを開けませんでした: ${String(e)}`);
      }
    }
    return () => { disposed = true; clearTimeout(timer); clearTimeout(retryTimer); };
  }, [open, outputDirectory, bookKey]);

  const applyPending = async (directory: string, seq?: number) => {
    const refresh = ++refreshSeq.current;
    const result = await api.listPendingBackups(directory, bookKey ? bookKey.split(",").map(Number) : []);
    if (seq !== undefined && seq !== rescanSeq.current) return;
    // A newer refresh (e.g. the dialog's own poll cycle) wins.
    if (refresh !== refreshSeq.current) return;
    if (result.indexReady) { setIndexReady(true); setPendingIds(new Set(result.pending)); }
  };

  const rescan = async () => {
    const seq = ++rescanSeq.current;
    const directory = outputDirectory;
    setRescanning(true); setError(""); setRescanNote("");
    try {
      const result = await api.rescanBackups(directory);
      if (seq !== rescanSeq.current) return;
      setRescanNote(`再スキャン完了: ${result.scanned}件のCBZを確認し、${result.recorded}件を記録しました（マニフェストなし ${result.unrecognized}件）`);
      await applyPending(directory, seq);
    } catch (e) { if (seq === rescanSeq.current) setError(String(e)); }
    finally { if (seq === rescanSeq.current) setRescanning(false); }
  };

  const start = async () => {
    if (!accepted || locked || !targets.length) return;
    setPending(true); setError("");
    try {
      await api.startBatchCapture({ bookIds: targets.map((book) => book.id), outputDirectory });
      // バッチ中は別モニターで進捗を見られるよう、進捗ウィンドウを自動で開く。
      // 状態取得の成否に引きずられないよう、開始直後に独立して呼ぶ。
      if (batch) void api.openProgressWindow().catch((e) => setError(`進捗ウィンドウを開けませんでした: ${String(e)}`));
      const after = await api.pageCaptureStatus();
      setStatus(after);
      if (!after.running) {
        // A batch of pure skips can finish before the poll ever
        // observes it running; refresh the targets right away.
        await applyPending(outputDirectory);
      }
    } catch (e) { setError(String(e)); }
    finally { setPending(false); }
  };

  return <dialog ref={dialog} className="capture-dialog" onCancel={(event) => { event.preventDefault(); onClose(); }}>
    <header className="panel-header"><h2>{accepted ? batch ? "一括バックアップ" : "バックアップ" : "バックアップ前のご確認"}</h2><button className="icon-button" onClick={onClose} aria-label="バックアップ画面を閉じる">×</button></header>
    {!accepted ? <>
      <p>{displayEngine === "desktop" ? "公式Kindleデスクトップアプリに表示された書籍の画面" : "正規のKindle Cloud Readerにより画面に表示された書籍のスクリーンショット"}から、CBZファイルを生成します。</p>
      <p>用途はバックアップです。利用権限のある書籍について、私的複製の範囲での利用に限ります。第三者への配布・公開・共有は行わないでください。</p>
      <p className="muted">コンテンツ保護の回避は行いません。この説明は、個々の複製行為の適法性を保証するものではありません。</p>
      <div className="backup-actions"><button className="secondary" onClick={onClose}>キャンセル</button><button className="primary" onClick={() => setAccepted(true)}>確認して詳細へ</button></div>
    </> : <>
      <p className="muted">{batch ? "現在の一覧に含まれる未バックアップの書籍を順に処理します。検索・絞り込み・開いているシリーズが対象に反映されます。" : "書籍を先頭からバックアップします。"}保存形式: シリーズ名／書籍名.cbz</p>
      <ol className="backup-book-list" aria-label="バックアップ対象の書籍">
        {targets.map((book) => <li key={book.id}><strong>{book.title}</strong><small className="muted">{book.effectiveSeriesName || "シリーズなし"}</small></li>)}
      </ol>
      {loading ? <p role="status">{indexReady ? "バックアップ済みの書籍を確認しています…" : "バックアップ記録を準備しています…（初回のみ時間がかかります）"}</p> : !targets.length && <p>{batch ? "現在の一覧に未バックアップの書籍はありません。" : "未バックアップの書籍はありません。"}</p>}
      <div className="backup-actions"><button className="primary" disabled={locked || !targets.length} onClick={start}>バックアップ開始</button><button className="secondary" disabled={!status?.running || pending} onClick={() => api.stopPageCapture().catch((e) => setError(String(e)))}>バックアップ停止</button>{batch && <button className="secondary" onClick={() => api.openProgressWindow().catch((e) => setError(String(e)))}>進捗ウィンドウ</button>}<button className="secondary" disabled={rescanLocked} onClick={() => void rescan()}>記録を再スキャン</button></div>
      {rescanNote && <p className="muted">{rescanNote}</p>}
      {status?.message && <div className="hint" role="status"><strong>{status.message}</strong>{status.batch && <p>完了 {status.completedBooks}冊・スキップ {status.skippedBooks}冊{status.textBooks > 0 && <>・対象外 {status.textBooks}冊</>}／全{status.totalBooks}冊<br />対象: {status.currentTitle || "確認中"}</p>}<p>{status.savedPages}枚保存済み</p><span className="capture-path">{status.outputDirectory}</span></div>}
      {status?.logPath && <p className="capture-path">操作ログ: {status.logPath}</p>}
      {status?.logError && <p className="capture-error" role="alert">{status.logError}</p>}
      <p className="muted">{displayEngine === "desktop" ? "設定は歯車から変更できます。画面を閉じても処理は続きます。停止は上のボタンまたはEscで行えます。処理中はKindleアプリを操作しないでください。" : "設定は歯車から変更できます。初回はリーダーでAmazonにログインしてください。画面を閉じても処理は続きます。停止は上のボタンまたはEscで行えます。リーダーをほかの画面で覆わないでください。"}</p>
    </>}
    {error && <p className="capture-error" role="alert">{error}</p>}
  </dialog>;
}
