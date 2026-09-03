#!/usr/bin/env node

import {
  access,
  mkdir,
  open,
  opendir,
  readFile,
  stat,
  writeFile,
} from "node:fs/promises";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const BODY_EXTENSIONS = new Set([
  ".azw",
  ".azw1",
  ".azw3",
  ".azw4",
  ".kfx",
  ".mobi",
  ".prc",
]);

const IMAGE_EXTENSIONS = new Set([".jpg", ".jpeg", ".png", ".webp"]);

const CANDIDATE_EXTENSIONS = new Set([
  ".xml",
  ".json",
  ".db",
  ".sqlite",
  ".sqlite3",
  ".dat",
  ".ldb",
  ".leveldb",
  ".kinf2024",
  ".pb",
  ".pma",
  ".hyb",
]);

const CANDIDATE_NAME = /(metadata|library|catalog|collection|annotation|book[_-]?asset|cover|sync|storage)/i;
const SQLITE_HEADER = Buffer.from("SQLite format 3\u0000", "ascii");

function shouldSkipDirectory(packagePath, directoryPath) {
  const relative = path.relative(packagePath, directoryPath);
  const segments = relative.split(path.sep);
  if (segments[0]?.toLowerCase() === "tempstate" && segments[1]?.toLowerCase() === "webview") {
    return true;
  }
  return (
    segments[0]?.toLowerCase() === "ac" &&
    segments.some((segment) => /^(inetcookies|cookies?)$/i.test(segment))
  );
}

function parseArgs(argv) {
  const options = { roots: [], pretty: false, output: null };
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === "--root") {
      const value = argv[index + 1];
      if (!value) throw new Error("--root requires a path");
      options.roots.push(value);
      index += 1;
    } else if (argument === "--output") {
      const value = argv[index + 1];
      if (!value) throw new Error("--output requires a path");
      options.output = value;
      index += 1;
    } else if (argument === "--pretty") {
      options.pretty = true;
    } else if (argument === "--help" || argument === "-h") {
      options.help = true;
    } else {
      throw new Error(`Unknown argument: ${argument}`);
    }
  }
  return options;
}

function helpText() {
  return [
    "Usage: node --experimental-sqlite tools/kindle-discovery.mjs [options]",
    "",
    "Options:",
    "  --root <path>    Kindle package path (repeatable)",
    "  --output <path>  Write JSON to a local file",
    "  --pretty         Pretty-print JSON",
    "  --help           Show this help",
  ].join("\n");
}

async function exists(targetPath) {
  try {
    await access(targetPath);
    return true;
  } catch {
    return false;
  }
}

function isKindlePackageName(name) {
  return /kindle/i.test(name);
}

async function normalizeRequestedRoot(requestedRoot) {
  const absolute = path.resolve(requestedRoot);
  if (await exists(absolute)) return absolute;

  const suffix = path.basename(absolute);
  const productDirectory = path.dirname(absolute);
  const productName = path.basename(productDirectory);
  if (suffix.startsWith("_") && isKindlePackageName(productName)) {
    const combined = path.join(
      path.dirname(productDirectory),
      `${productName}${suffix}`,
    );
    if (await exists(combined)) return combined;
  }

  throw new Error(`Kindle root does not exist: ${requestedRoot}`);
}

async function discoverDefaultRoots(warnings) {
  const localAppData = process.env.LOCALAPPDATA;
  if (!localAppData) {
    warnings.push("LOCALAPPDATA is not defined; no default package search was run.");
    return [];
  }

  const packagesDirectory = path.join(localAppData, "Packages");
  let directory;
  try {
    directory = await opendir(packagesDirectory);
  } catch (error) {
    warnings.push(`Cannot enumerate ${packagesDirectory}: ${error.message}`);
    return [];
  }

  const roots = [];
  for await (const entry of directory) {
    if (entry.isDirectory() && isKindlePackageName(entry.name)) {
      roots.push(path.join(packagesDirectory, entry.name));
    }
  }
  return roots;
}

export async function resolvePackageRoots(requestedRoots = [], warnings = []) {
  const roots = requestedRoots.length
    ? await Promise.all(requestedRoots.map(normalizeRequestedRoot))
    : await discoverDefaultRoots(warnings);
  return [...new Set(roots.map((root) => path.resolve(root)))].sort((a, b) =>
    a.localeCompare(b),
  );
}

function fileRecord(absolutePath, fileStat) {
  return {
    absolutePath,
    extension: path.extname(absolutePath).toLowerCase(),
    size: fileStat.size,
    lastModified: fileStat.mtime.toISOString(),
  };
}

async function readPrefix(filePath, byteCount = 65_536) {
  const handle = await open(filePath, "r");
  try {
    const buffer = Buffer.alloc(byteCount);
    const { bytesRead } = await handle.read(buffer, 0, byteCount, 0);
    return buffer.subarray(0, bytesRead);
  } finally {
    await handle.close();
  }
}

function findXmlRoot(buffer) {
  let text = buffer.toString("utf8").replace(/^\uFEFF/, "");
  text = text
    .replace(/<\?xml[\s\S]*?\?>/gi, "")
    .replace(/<!--[\s\S]*?-->/g, "")
    .replace(/<!DOCTYPE[\s\S]*?>/gi, "")
    .trimStart();
  const match = text.match(/^<([A-Za-z_][\w:.-]*)\b/);
  return match?.[1] ?? null;
}

function jsonTopLevel(value) {
  if (Array.isArray(value)) {
    return {
      kind: "array",
      length: value.length,
      firstItemType:
        value.length === 0
          ? null
          : Array.isArray(value[0])
            ? "array"
            : value[0] === null
              ? "null"
              : typeof value[0],
    };
  }
  if (value === null) return { kind: "null" };
  if (typeof value === "object") {
    return { kind: "object", keys: Object.keys(value).sort() };
  }
  return { kind: typeof value };
}

async function inspectSqlite(filePath) {
  const { DatabaseSync } = await import("node:sqlite");
  const database = new DatabaseSync(filePath, {
    open: true,
    readOnly: true,
  });
  try {
    return database
      .prepare(
        "SELECT name FROM sqlite_schema " +
          "WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
      )
      .all()
      .map((row) => row.name);
  } finally {
    database.close();
  }
}

export async function inspectStructuredFile(filePath, baseRecord = null) {
  const record = baseRecord ?? fileRecord(filePath, await stat(filePath));
  const extension = record.extension;

  if (BODY_EXTENSIONS.has(extension)) return record;

  try {
    const prefix = await readPrefix(filePath);
    if (prefix.subarray(0, SQLITE_HEADER.length).equals(SQLITE_HEADER)) {
      record.detectedType = "sqlite";
      try {
        record.sqliteTables = await inspectSqlite(filePath);
      } catch (error) {
        record.sqliteTables = [];
        record.inspectionError = error.message;
      }
      return record;
    }

    if (extension === ".xml") {
      record.detectedType = "xml";
      record.xmlRoot = findXmlRoot(prefix);
    } else if (extension === ".json") {
      record.detectedType = "json";
      try {
        const value = JSON.parse(await readFile(filePath, "utf8"));
        record.jsonTopLevel = jsonTopLevel(value);
      } catch (error) {
        record.inspectionError = error.message;
      }
    }
  } catch (error) {
    record.inspectionError = error.message;
  }
  return record;
}

function isCandidate(filePath) {
  const extension = path.extname(filePath).toLowerCase();
  if (BODY_EXTENSIONS.has(extension) || IMAGE_EXTENSIONS.has(extension)) {
    return false;
  }
  return CANDIDATE_EXTENSIONS.has(extension) || CANDIDATE_NAME.test(path.basename(filePath));
}

async function scanPackage(packagePath, state, warnings) {
  async function walk(directoryPath) {
    if (directoryPath !== packagePath && shouldSkipDirectory(packagePath, directoryPath)) {
      return;
    }

    let directory;
    try {
      directory = await opendir(directoryPath);
    } catch (error) {
      warnings.push(`Cannot enumerate ${directoryPath}: ${error.message}`);
      return;
    }

    let imageCount = 0;
    let imageBytes = 0;
    const imageExtensions = new Set();

    for await (const entry of directory) {
      const absolutePath = path.join(directoryPath, entry.name);
      if (entry.isDirectory()) {
        await walk(absolutePath);
        continue;
      }
      if (!entry.isFile()) continue;

      let fileStat;
      try {
        fileStat = await stat(absolutePath);
      } catch (error) {
        warnings.push(`Cannot stat ${absolutePath}: ${error.message}`);
        continue;
      }

      const extension = path.extname(entry.name).toLowerCase();
      if (IMAGE_EXTENSIONS.has(extension)) {
        imageCount += 1;
        imageBytes += fileStat.size;
        imageExtensions.add(extension);
      }

      if (!isCandidate(absolutePath)) continue;
      const candidate = await inspectStructuredFile(
        absolutePath,
        fileRecord(absolutePath, fileStat),
      );
      state.candidateFiles.push(candidate);
      if (candidate.detectedType === "sqlite") {
        state.databaseFiles.push(candidate);
      }
      if (candidate.detectedType === "xml" || candidate.detectedType === "json") {
        state.metadataFiles.push(candidate);
      }
    }

    if (/cover/i.test(path.basename(directoryPath)) && imageCount > 0) {
      state.coverDirectories.push({
        absolutePath: directoryPath,
        fileCount: imageCount,
        totalBytes: imageBytes,
        extensions: [...imageExtensions].sort(),
      });
    }
  }

  await walk(packagePath);
}

function sortByPath(records) {
  records.sort((a, b) => a.absolutePath.localeCompare(b.absolutePath));
}

export async function discoverKindleData({ roots = [] } = {}) {
  const warnings = [];
  const packageRoots = await resolvePackageRoots(roots, warnings);
  const state = {
    kindlePackages: [],
    metadataFiles: [],
    databaseFiles: [],
    coverDirectories: [],
    candidateFiles: [],
    warnings,
  };

  for (const packagePath of packageRoots) {
    const packageStat = await stat(packagePath);
    state.kindlePackages.push({
      absolutePath: packagePath,
      packageName: path.basename(packagePath),
      lastModified: packageStat.mtime.toISOString(),
    });
    await scanPackage(packagePath, state, warnings);
  }

  sortByPath(state.kindlePackages);
  sortByPath(state.metadataFiles);
  sortByPath(state.databaseFiles);
  sortByPath(state.coverDirectories);
  sortByPath(state.candidateFiles);
  return state;
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  if (options.help) {
    process.stdout.write(`${helpText()}\n`);
    return;
  }

  const result = await discoverKindleData({ roots: options.roots });
  const json = JSON.stringify(result, null, options.pretty ? 2 : 0);
  if (options.output) {
    const outputPath = path.resolve(options.output);
    await mkdir(path.dirname(outputPath), { recursive: true });
    await writeFile(outputPath, `${json}\n`, "utf8");
    process.stderr.write(`Wrote ${outputPath}\n`);
  } else {
    process.stdout.write(`${json}\n`);
  }
}

const invokedPath = process.argv[1] ? path.resolve(process.argv[1]) : null;
if (invokedPath === fileURLToPath(import.meta.url)) {
  main().catch((error) => {
    process.stderr.write(`${error.stack ?? error.message}\n`);
    process.exitCode = 1;
  });
}
