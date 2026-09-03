import assert from "node:assert/strict";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import {
  discoverKindleData,
  resolvePackageRoots,
} from "../tools/kindle-discovery.mjs";

test("discovers structured metadata without inspecting Kindle body files", async (t) => {
  const fixtureRoot = await mkdtemp(path.join(tmpdir(), "hondana-discovery-"));
  t.after(async () => {
    assert.ok(fixtureRoot.startsWith(tmpdir()));
    await rm(fixtureRoot, { recursive: true, force: true });
  });

  const packageRoot = path.join(fixtureRoot, "AMZNKindle.TestApp_suffix");
  const cacheRoot = path.join(packageRoot, "LocalState", "Classic", "Data", "Cache");
  const coverRoot = path.join(cacheRoot, "covers");
  await mkdir(coverRoot, { recursive: true });

  await writeFile(path.join(cacheRoot, "KindleSyncMetadataCache.xml"), "<?xml version=\"1.0\"?><response><book /></response>");
  await writeFile(path.join(cacheRoot, "metadata.json"), JSON.stringify({ books: [] }));
  await writeFile(path.join(cacheRoot, "ignored.kfx"), "not metadata");
  await writeFile(path.join(coverRoot, "cover.jpg"), Buffer.from([0xff, 0xd8, 0xff, 0xd9]));

  const webViewRoot = path.join(packageRoot, "TempState", "Webview", "EBWebView");
  await mkdir(webViewRoot, { recursive: true });

  const { DatabaseSync } = await import("node:sqlite");
  const databasePath = path.join(cacheRoot, "library.db");
  const database = new DatabaseSync(databasePath);
  database.exec("CREATE TABLE books (id INTEGER PRIMARY KEY, title TEXT)");
  database.close();

  const webViewDatabase = new DatabaseSync(path.join(webViewRoot, "Cookies.db"));
  webViewDatabase.exec("CREATE TABLE cookies (id INTEGER PRIMARY KEY)");
  webViewDatabase.close();

  const result = await discoverKindleData({ roots: [packageRoot] });
  assert.equal(result.kindlePackages.length, 1);
  assert.equal(result.metadataFiles.length, 2);
  assert.equal(result.databaseFiles.length, 1);
  assert.deepEqual(result.databaseFiles[0].sqliteTables, ["books"]);
  assert.equal(result.coverDirectories[0].fileCount, 1);
  assert.equal(
    result.metadataFiles.find((file) => file.extension === ".xml").xmlRoot,
    "response",
  );
  assert.deepEqual(
    result.metadataFiles.find((file) => file.extension === ".json").jsonTopLevel,
    { kind: "object", keys: ["books"] },
  );
  assert.equal(
    result.candidateFiles.some((file) => file.extension === ".kfx"),
    false,
  );
  assert.equal(
    result.databaseFiles.some((file) => file.absolutePath.includes("Webview")),
    false,
  );
});

test("accepts the package-family suffix as a mistakenly separated path", async (t) => {
  const fixtureRoot = await mkdtemp(path.join(tmpdir(), "hondana-root-"));
  t.after(async () => {
    assert.ok(fixtureRoot.startsWith(tmpdir()));
    await rm(fixtureRoot, { recursive: true, force: true });
  });

  const actualRoot = path.join(fixtureRoot, "AMZNKindle.TestApp_suffix");
  await mkdir(actualRoot);
  const separatedRoot = path.join(fixtureRoot, "AMZNKindle.TestApp", "_suffix");
  assert.deepEqual(await resolvePackageRoots([separatedRoot]), [actualRoot]);
});
