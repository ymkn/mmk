import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

const capture = readFileSync(new URL('../src-tauri/src/capture.rs', import.meta.url), 'utf8');
const lib = readFileSync(new URL('../src-tauri/src/lib.rs', import.meta.url), 'utf8');
const lifecycle = capture.slice(capture.indexOf('fn open_fresh_batch_reader('), capture.indexOf('fn wait_for_reader_closed('));

test('every captured batch book opens a fresh reader after the foreground guard', () => {
  const batch = capture.slice(capture.indexOf('pub fn start_batch('), capture.indexOf('pub fn preview('));
  assert.ok(batch.indexOf('platform::check_foreground(handle)?') < batch.indexOf('open_fresh_batch_reader('));
  assert.match(batch, /let window =\s*open_fresh_batch_reader\(/);
  assert.doesNotMatch(batch, /super::open_capture_reader_window\(/);
});

test('closing and waiting for removal precede opening the next reader', () => {
  assert.match(lifecycle, /reader\.close\(\)/);
  assert.match(lifecycle, /wait_for_reader_closed\(manager, Duration::from_secs\(10\)/);
  assert.ok(lifecycle.indexOf('reader.close()') < lifecycle.indexOf('wait_for_reader_closed('));
  assert.ok(lifecycle.indexOf('wait_for_reader_closed(') < lifecycle.indexOf('super::open_capture_reader_window('));
  assert.match(lifecycle, /wait\(manager, Duration::ZERO\)\?/);
  assert.doesNotMatch(lifecycle, /\.navigate\(|\.clear_all_browsing_data\(/);
});

test('fresh readers retain the dedicated profile and existing navigation restrictions', () => {
  const open = lib.slice(lib.indexOf('fn open_capture_reader_window('), lib.indexOf('async fn preview_capture_layout('));
  assert.match(open, /\.data_directory\([\s\S]*?\.join\("kindle-reader-webview"\)/);
  assert.match(open, /capture_reader_navigation_allowed\(url\)/);
  assert.match(open, /actual\.eq_ignore_ascii_case\(expected\)/);
  assert.doesNotMatch(open, /\.clear_all_browsing_data\(/);
});
