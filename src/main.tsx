import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import "./styles.css";

const root = createRoot(document.getElementById("root")!);

// 進捗ウィンドウかどうかはウィンドウラベルで判定する。
// URL クエリは Tauri 外（ブラウザ開発時）の予備にだけ使う。
let view: string | null = null;
let inTauri = true;
try {
  if (getCurrentWebviewWindow().label === "progress") view = "progress";
} catch {
  inTauri = false;
}
if (!inTauri) {
  view = new URLSearchParams(window.location.search).get("view");
}

// 進捗ウィンドウでは App 本体（とその依存モジュール）を読み込まない。
// 必要なのは page_capture_status のポーリングだけに絞る。
if (view === "progress") {
  void import("./ProgressWindow").then(({ ProgressWindow }) => {
    root.render(
      <StrictMode>
        <ProgressWindow />
      </StrictMode>,
    );
  }).catch(() => root.render(<p>画面の読み込みに失敗しました。ウィンドウを開き直してください。</p>));
} else {
  void import("./App").then(({ default: App }) => {
    root.render(
      <StrictMode>
        <App />
      </StrictMode>,
    );
  }).catch(() => root.render(<p>画面の読み込みに失敗しました。アプリを再起動してください。</p>));
}
