export const FILTER_SECTIONS_STORAGE_KEY = "mmk.filter-sections.v1";

export const DEFAULT_FILTER_SECTIONS = {
  tags: true,
  rating: false,
  authors: false,
  publishers: false,
  labels: false,
};

export type FilterSections = typeof DEFAULT_FILTER_SECTIONS;
type StorageAccess = Pick<Storage, "getItem" | "setItem">;

export function readFilterSections(storage?: StorageAccess): FilterSections {
  const result = { ...DEFAULT_FILTER_SECTIONS };
  try {
    const parsed = JSON.parse((storage ?? localStorage).getItem(FILTER_SECTIONS_STORAGE_KEY) ?? "null");
    if (parsed && typeof parsed === "object") {
      for (const key of Object.keys(result) as (keyof FilterSections)[]) {
        if (typeof parsed[key] === "boolean") result[key] = parsed[key];
      }
    }
  } catch {
    // Missing or inaccessible settings use the defaults.
  }
  return result;
}

export function writeFilterSections(sections: FilterSections, storage?: StorageAccess): void {
  try {
    (storage ?? localStorage).setItem(FILTER_SECTIONS_STORAGE_KEY, JSON.stringify(sections));
  } catch {
    // Storage failure must not prevent filtering.
  }
}
