import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import ts from 'typescript';

const source = readFileSync(new URL('../src/amazonCheckView.ts', import.meta.url), 'utf8');
const { outputText } = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } });
const { newReleaseStatusView: view } = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString('base64')}`);
const status = (phase, lastSuccess = null) => ({ version: 1, phase, message: 'message', lastSuccess });
const result = (receivedCount = 3, storedCount = 2) => ({ completedAt: '2026-11-03T00:00:00Z', receivedCount, storedCount });

test('unknown, loading, login and errors never claim that there are no new releases', () => {
  for (const phase of ['idle', 'checking', 'login-required', 'error']) {
    const value = view(status(phase), 0);
    assert.doesNotMatch(value.emptyTitle, /ありません|0件/);
  }
  assert.equal(view(null, 0).title, '取得状態を確認中');
  assert.equal(view(status('idle'), 0).title, '未確認');
  assert.equal(view(status('checking'), 0).title, '取得中');
  assert.equal(view(status('login-required'), 0).title, 'ログイン待ち');
  assert.equal(view(status('error'), 0).title, '取得失敗');
});

test('successful empty retrieval differs from candidates removed by matching or filtering', () => {
  assert.match(view(status('success', result(0, 0)), 0).emptyTitle, /Amazonの取得結果は0件/);
  assert.match(view(status('success', result()), 2).emptyTitle, /照合・除外後/);
  assert.match(view(status('success', result()), 2).emptyDetail, /価格・シリーズ名・巻数・所蔵状況・除外設定/);
  assert.equal(view(status('idle', result()), 2).title, '前回の取得結果');
});

test('failed or pending checks explicitly label saved candidates as previous results', () => {
  for (const phase of ['idle', 'checking', 'login-required', 'error']) {
    assert.equal(view(status(phase, result()), 2).showingPrevious, true);
  }
  assert.equal(view(status('success', result()), 2).showingPrevious, false);
  const failed = view(status('success', result()), 2, '読み込み失敗');
  assert.equal(failed.failed, true);
  assert.equal(failed.showingPrevious, true);
  assert.match(failed.emptyTitle, /完了していません/);
  assert.equal(view(status('error', result(0, 0)), 0).showingPrevious, true);
});

test('tab switches views without fetching; retry is a separate action and statuses are visible', () => {
  const app = readFileSync(new URL('../src/App.tsx', import.meta.url), 'utf8');
  const tab = app.split('\n').find(line => line.includes('<button id="new-releases-tab"'));
  assert.ok(tab);
  assert.match(tab, /setActiveView/);
  assert.doesNotMatch(tab, /openAmazonChecker/);
  assert.match(app, /onClick=\{openAmazonChecker\}/);
  assert.match(app, /<NewReleaseStatus /);
  assert.doesNotMatch(app, /続刊候補はありません/);
});
