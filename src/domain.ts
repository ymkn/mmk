import type { Book, SeriesPreferences } from "./types";

export type ScoreFilter = "all" | "positive" | "negative";
export type NoteFilter = "all" | "with-note" | "without-note";

export function matchesNoteFilter(preferences: SeriesPreferences | undefined, filter: NoteFilter): boolean {
  const hasNote = Boolean(preferences?.note?.trim());
  return filter === "all" || (filter === "with-note" ? hasNote : !hasNote);
}

// A series match includes its volumes without changing their individual metadata.
export function matchesRatingFilters(book: Book, preferences: SeriesPreferences | undefined, favoriteOnly: boolean, scoreFilter: ScoreFilter): boolean {
  if (favoriteOnly && !book.favorite && !preferences?.favorite) return false;
  const scores = [book.score, preferences?.score ?? 0];
  if (scoreFilter === "positive" && !scores.some((score) => score > 0)) return false;
  if (scoreFilter === "negative" && !scores.some((score) => score < 0)) return false;
  return true;
}

export interface SeriesShelf {
  key: string;
  name: string;
  books: Book[];
  coverBook: Book;
}

export interface LibraryShelves {
  series: SeriesShelf[];
  singles: Book[];
}

export function seriesKey(value: string): string {
  return value.normalize("NFKC").toLocaleLowerCase("ja").replace(/[\s・:：_\-‐–—]/g, "");
}

// At the library root, series cards represent every filtered volume in each series.
export function booksInLibraryView(filtered: Book[], openedSeries: string | null): Book[] {
  return openedSeries
    ? filtered.filter((book) => book.effectiveSeriesName && seriesKey(book.effectiveSeriesName) === openedSeries)
    : filtered;
}

export function groupBooks(books: Book[]): LibraryShelves {
  const candidates = new Map<string, { name: string; books: Book[] }>();
  const singles: Book[] = [];
  for (const book of books) {
    const name = book.effectiveSeriesName?.trim();
    if (!name) {
      singles.push(book);
      continue;
    }
    const key = seriesKey(name);
    const group = candidates.get(key) ?? { name, books: [] };
    group.books.push(book);
    candidates.set(key, group);
  }

  const series: SeriesShelf[] = [];
  for (const [key, group] of candidates) {
    if (group.books.length < 2) {
      singles.push(...group.books);
      continue;
    }
    group.books.sort(compareVolumes);
    series.push({ key, name: group.name, books: group.books, coverBook: group.books[0] });
  }
  series.sort((left, right) => left.name.localeCompare(right.name, "ja"));
  singles.sort((left, right) => left.title.localeCompare(right.title, "ja"));
  return { series, singles };
}

export function compareVolumes(left: Book, right: Book): number {
  const leftIndex = left.effectiveSeriesIndex ?? Number.POSITIVE_INFINITY;
  const rightIndex = right.effectiveSeriesIndex ?? Number.POSITIVE_INFINITY;
  return leftIndex - rightIndex || left.title.localeCompare(right.title, "ja") || left.asin.localeCompare(right.asin);
}
