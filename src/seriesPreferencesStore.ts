import type { SeriesPreferences, SeriesPreferencesInput } from "./types";

// Serialize edits and reads, including after the series panel is closed or renamed.
export function createSeriesPreferencesStore(load: () => Promise<SeriesPreferences[]>, save: (input: SeriesPreferencesInput) => Promise<SeriesPreferences>) {
  let pending: Promise<unknown> = Promise.resolve();
  const enqueue = <T>(operation: () => Promise<T>): Promise<T> => {
    const result = pending.then(operation);
    pending = result.catch(() => {});
    return result;
  };
  return {
    load: () => enqueue(load),
    afterPending: enqueue,
    save: (input: SeriesPreferencesInput) => {
      const snapshot = { ...input };
      return enqueue(() => save(snapshot));
    },
  };
}
