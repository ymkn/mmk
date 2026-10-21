const STORAGE_KEY = "mmk.capture-root.v1";
type SettingsStorage = Pick<Storage, "getItem" | "setItem">;

export function readCaptureRoot(storage?: SettingsStorage): string {
  try {
    storage ??= localStorage;
    const saved = storage.getItem(STORAGE_KEY);
    if (saved !== null) return saved;
    // Preserve an existing custom root when upgrading from separate settings.
    for (const key of ["mmk.batch-capture.v1", "mmk.page-capture.v1"]) {
      try {
        const previous = JSON.parse(storage.getItem(key) ?? "null");
        if (typeof previous?.outputDirectory === "string" && previous.outputDirectory.trim()) return previous.outputDirectory;
      } catch { /* Try the other legacy setting. */ }
    }
  } catch { /* Use the OS default if storage is unavailable. */ }
  return "";
}

export function writeCaptureRoot(value: string, storage?: SettingsStorage): void {
  try { (storage ?? localStorage).setItem(STORAGE_KEY, value); } catch { /* Match other application settings. */ }
}
