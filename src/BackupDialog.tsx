import { useEffect, useRef, useState } from "react";
import { api } from "./api";
import type { Book, CaptureStatus } from "./types";

export function BackupDialog({ books, open, batch, outputDirectory, onClose }: {
  books: Book[]; open: boolean; batch: boolean; outputDirectory: string; onClose: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [accepted, setAccepted] = useState(false);
  const [pendingIds, setPendingIds] = useState<Set<number>>(new Set());
  const [status, setStatus] = useState<CaptureStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  const bookKey = books.filter((book) => book.sourceStatus === "active").map((book) => book.id).join(",");
  const targets = books.filter((book) => book.sourceStatus === "active" && pendingIds.has(book.id)).sort((a, b) =>
    (a.effectiveSeriesName ?? a.title).localeCompare(b.effectiveSeriesName ?? b.title, "ja") ||
    (a.effectiveSeriesIndex ?? Infinity) - (b.effectiveSeriesIndex ?? Infinity) || a.title.localeCompare(b.title, "ja"));
  const locked = loading || pending || status?.running === true;

  useEffect(() => {
    if (!open) { dialog.current?.close(); return; }
    setAccepted(false); setError(""); setLoading(true); setPendingIds(new Set());
    dialog.current?.showModal();
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    let wasRunning = false;
    const refreshTargets = async () => {
      const ids = await api.listPendingBackups(outputDirectory, bookKey ? bookKey.split(",").map(Number) : []);
      if (!disposed) { setPendingIds(new Set(ids)); setLoading(false); }
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
    void refreshTargets().catch((e) => { if (!disposed) setError(String(e)); });
    void poll();
    return () => { disposed = true; clearTimeout(timer); };
  }, [open, outputDirectory, bookKey]);

  const start = async () => {
    if (!accepted || locked || !targets.length) return;
    setPending(true); setError("");
    try {
      await api.startBatchCapture({ bookIds: targets.map((book) => book.id), outputDirectory });
      setStatus(await api.pageCaptureStatus());
    } catch (e) { setError(String(e)); }
    finally { setPending(false); }
  };

  return <dialog ref={dialog} className="capture-dialog" onCancel={(event) => { event.preventDefault(); onClose(); }}>
    <header className="panel-header"><h2>{accepted ? batch ? "一括バックアップ" : "バックアップ" : "バックアップ前のご確認"}</h2><button className="icon-button" onClick={onClose} aria-label="バックアップ画面を閉じる">×</button></header>
    {!accepted ? <>
      <p>正規のKindle Cloud Readerにより画面に表示された書籍のスクリーンショットから、CBZファイルを生成します。</p>
      <p>用途はバックアップです。利用権限のある書籍について、私的複製の範囲での利用に限ります。第三者への配布・公開・共有は行わないでください。</p>
      <p className="muted">コンテンツ保護の回避は行いません。この説明は、個々の複製行為の適法性を保証するものではありません。</p>
      <div className="backup-actions"><button className="secondary" onClick={onClose}>キャンセル</button><button className="primary" onClick={() => setAccepted(true)}>確認して詳細へ</button></div>
    </> : <>
      <p className="muted">{batch ? "現在の一覧に含まれる未バックアップの書籍を順に処理します。検索・絞り込み・開いているシリーズが対象に反映されます。" : "書籍を先頭からバックアップします。"}保存形式: シリーズ名／書籍名.cbz</p>
      <ol className="backup-book-list" aria-label="バックアップ対象の書籍">
        {targets.map((book) => <li key={book.id}><strong>{book.title}</strong><small className="muted">{book.effectiveSeriesName || "シリーズなし"}</small></li>)}
      </ol>
      {loading ? <p role="status">バックアップ済みの書籍を確認しています…</p> : !targets.length && <p>{batch ? "現在の一覧に未バックアップの書籍はありません。" : "未バックアップの書籍はありません。"}</p>}
      <div className="backup-actions"><button className="primary" disabled={locked || !targets.length} onClick={start}>バックアップ開始</button><button className="secondary" disabled={!status?.running || pending} onClick={() => api.stopPageCapture().catch((e) => setError(String(e)))}>バックアップ停止</button></div>
      {status?.message && <div className="hint" role="status"><strong>{status.message}</strong>{status.batch && <p>完了 {status.completedBooks}冊・スキップ {status.skippedBooks}冊／全{status.totalBooks}冊<br />対象: {status.currentTitle || "確認中"}</p>}<p>{status.savedPages}枚保存済み</p><span className="capture-path">{status.outputDirectory}</span></div>}
      {status?.logPath && <p className="capture-path">操作ログ: {status.logPath}</p>}
      {status?.logError && <p className="capture-error" role="alert">{status.logError}</p>}
      <p className="muted">設定は歯車から変更できます。初回はリーダーでAmazonにログインしてください。画面を閉じても処理は続きます。停止は上のボタンまたはEscで行えます。リーダーをほかの画面で覆わないでください。</p>
    </>}
    {error && <p className="capture-error" role="alert">{error}</p>}
  </dialog>;
}
