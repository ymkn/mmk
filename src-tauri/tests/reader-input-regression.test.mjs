import { readFileSync, readdirSync } from 'node:fs';
import { test } from 'node:test';
import assert from 'node:assert/strict';

test('reader scripts do not force focus or modify input handling', () => {
  const directory = new URL('../src/', import.meta.url);
  for (const name of readdirSync(directory)) {
    if (!/^reader_.*\.js$/.test(name)) continue;
    const source = readFileSync(new URL(name, directory), 'utf8');
    assert.doesNotMatch(source, /\.(?:focus|blur)\s*\(/, `${name} must not redirect input focus`);
    assert.doesNotMatch(source, /\btabindex\b|\btabIndex\b/, `${name} must not change the reader's focusability`);
    assert.doesNotMatch(source, /\.(?:preventDefault|stopPropagation|stopImmediatePropagation|setPointerCapture)\s*\(/, `${name} must not intercept manual input`);
    assert.doesNotMatch(source, /addEventListener\s*\(\s*['"](?:key|pointer|mouse|touch|click)/, `${name} must not register input handlers`);
  }
});
