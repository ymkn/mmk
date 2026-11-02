import type { AmazonCheckStatus } from "./types";

export function newReleaseStatusView(status: AmazonCheckStatus | null, savedCount: number, error = "") {
  const phase = status?.phase;
  const result = status?.lastSuccess;
  const checking = phase === "checking";
  const loginRequired = phase === "login-required";
  const failed = Boolean(error) || phase === "error";
  const title = failed ? "取得失敗" : checking ? "取得中" : loginRequired ? "ログイン待ち" : phase === "success" ? "取得成功" : result ? "前回の取得結果" : status ? "未確認" : "取得状態を確認中";
  let emptyTitle: string;
  let emptyDetail: string;
  if (failed) {
    emptyTitle = "今回の新刊確認は完了していません";
    emptyDetail = "取得失敗を「新刊なし」とは判断できません。再チェックしてください。";
  } else if (checking) {
    emptyTitle = "Amazonの新刊情報を確認しています";
    emptyDetail = "取得が完了するまで、新刊候補の有無は未確認です。";
  } else if (loginRequired) {
    emptyTitle = "Amazonへのログインが必要です";
    emptyDetail = "専用画面でログインすると取得を続けます。「ログイン画面を表示」で開き直せます。";
  } else if (result) {
    emptyTitle = result.receivedCount === 0 ? "Amazonの取得結果は0件でした" : "照合・除外後の候補は0件です";
    emptyDetail = result.receivedCount === 0
      ? "Amazonの推薦ページの結果です。新刊が存在しないことを保証するものではありません。"
      : "取得した候補が、価格・シリーズ名・巻数・所蔵状況・除外設定の条件を満たしていません。";
  } else {
    emptyTitle = "新刊情報の取得状態は未確認です";
    emptyDetail = "「再チェック」で取得してください。候補がないと確認できた状態ではありません。";
  }
  return {
    title, emptyTitle, emptyDetail, checking, loginRequired, failed,
    message: error || status?.message || "取得状態を読み込んでいます",
    showingPrevious: (savedCount > 0 || Boolean(result)) && (phase !== "success" || failed),
  };
}
