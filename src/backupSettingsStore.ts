import type { BackupSettings } from "./types";

// Keep writes and later reads ordered, including across settings-panel remounts.
export function createBackupSettingsStore(load: () => Promise<BackupSettings>, save: (settings: BackupSettings) => Promise<void>) {
  let pending: Promise<unknown> = Promise.resolve();
  const enqueue = <T>(operation: () => Promise<T>): Promise<T> => {
    const result = pending.then(operation);
    pending = result.catch(() => {});
    return result;
  };
  return {
    load: () => enqueue(load),
    afterPending: enqueue,
    save: (settings: BackupSettings) => {
      const snapshot = { ...settings };
      return enqueue(() => save(snapshot));
    },
  };
}
