import type { NoteFilter, ScoreFilter } from "./domain";

export const FILTER_SECTIONS_STORAGE_KEY = "mmk.filter-sections.v1";
export const LIBRARY_FILTERS_STORAGE_KEY = "mmk.library-filters.v1";

export const DEFAULT_FILTER_SECTIONS = {
  tags: true,
  rating: false,
  notes: false,
  authors: false,
  publishers: false,
  labels: false,
};

export type FilterSections = typeof DEFAULT_FILTER_SECTIONS;
type StorageAccess = Pick<Storage, "getItem" | "setItem">;

export interface LibraryFilters {
  query: string;
  favoriteOnly: boolean;
  scoreFilter: ScoreFilter;
  noteFilter: NoteFilter;
  selectedAuthors: Set<string>;
  selectedPublishers: Set<string>;
  selectedLabels: Set<string>;
  selectedTags: Set<string>;
}

export function readLibraryFilters(storage?: StorageAccess): LibraryFilters {
  const result: LibraryFilters = {
    query: "",
    favoriteOnly: false,
    scoreFilter: "all",
    noteFilter: "all",
    selectedAuthors: new Set(),
    selectedPublishers: new Set(),
    selectedLabels: new Set(),
    selectedTags: new Set(),
  };
  try {
    const parsed = JSON.parse((storage ?? localStorage).getItem(LIBRARY_FILTERS_STORAGE_KEY) ?? "null");
    if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
      if (typeof parsed.query === "string") result.query = parsed.query;
      if (typeof parsed.favoriteOnly === "boolean") result.favoriteOnly = parsed.favoriteOnly;
      if (["all", "positive", "negative"].includes(parsed.scoreFilter)) result.scoreFilter = parsed.scoreFilter;
      if (["all", "with-note", "without-note"].includes(parsed.noteFilter)) result.noteFilter = parsed.noteFilter;
      for (const key of ["selectedAuthors", "selectedPublishers", "selectedLabels", "selectedTags"] as const) {
        if (Array.isArray(parsed[key])) {
          result[key] = new Set(parsed[key].filter((value: unknown): value is string => typeof value === "string" && value.trim() !== ""));
        }
      }
    }
  } catch {
    // Missing, corrupt, or inaccessible settings use the defaults.
  }
  return result;
}

export function writeLibraryFilters(filters: LibraryFilters, storage?: StorageAccess): void {
  try {
    (storage ?? localStorage).setItem(LIBRARY_FILTERS_STORAGE_KEY, JSON.stringify({
      query: filters.query,
      favoriteOnly: filters.favoriteOnly,
      scoreFilter: filters.scoreFilter,
      noteFilter: filters.noteFilter,
      selectedAuthors: [...filters.selectedAuthors],
      selectedPublishers: [...filters.selectedPublishers],
      selectedLabels: [...filters.selectedLabels],
      selectedTags: [...filters.selectedTags],
    }));
  } catch {
    // Storage failure must not prevent filtering.
  }
}

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
