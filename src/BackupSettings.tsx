import { useEffect, useRef, useState } from "react";
import { api } from "./api";
import type { BackupSettings } from "./types";

export function BackupSettingsSection() {
  const [settings, setSettings] = useState<BackupSettings | null>(null);
  const [busy, setBusy] = useState(false);
  const [saveError, setSaveError] = useState("");
  const mounted = useRef(false);
  const saveRevision = useRef(0);
  useEffect(() => {
    mounted.current = true;
    let disposed = false;
    api.getBackupSettings().then((value) => {
      if (disposed) return;
      setSettings(value);
    }).catch((e) => { if (!disposed) setSaveError(`設定の読み込みに失敗しました: ${String(e)}`); });
    return () => { disposed = true; mounted.current = false; };
  }, []);
  const update = (next: BackupSettings) => {
    setSettings(next);
    const revision = ++saveRevision.current;
    setSaveError("");
    void api.saveBackupSettings(next).catch((e) => {
      if (mounted.current && revision === saveRevision.current) setSaveError(`設定を保存できませんでした: ${String(e)}`);
    });
  };
  const act = async (action: () => Promise<void>) => {
    setBusy(true); setSaveError("");
    try { await action(); } catch (e) { setSaveError(String(e)); } finally { setBusy(false); }
  };
  return <section>
    <h3>バックアップの動作</h3>
    {settings && <>
      <fieldset disabled={busy} className="backup-settings-fields">
        <div className="capsule-options" role="group" aria-label="ページ表示">
          <button type="button" aria-pressed={settings.pageLayout === "single"} onClick={() => update({ ...settings, pageLayout: "single" })}>単ページ</button>
          <button type="button" aria-pressed={settings.pageLayout === "spread"} onClick={() => update({ ...settings, pageLayout: "spread" })}>見開き</button>
        </div>
      </fieldset>
      <button type="button" className="library-link" disabled={busy} onClick={() => {
        if (!window.confirm("バックアップ用リーダーを閉じ、Amazonのログイン状態と閲覧データを削除しますか？")) return;
        void act(() => api.clearCaptureReaderSession());
      }}>バックアップ用リーダーのAmazon連携を解除</button>
    </>}
    {saveError && <p className="capture-error" role="alert">{saveError}</p>}
  </section>;
}
