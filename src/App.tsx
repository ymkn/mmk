import { getVersion } from "@tauri-apps/api/app";
import { convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useMemo, useRef, useState, type Dispatch, type ReactNode, type SetStateAction } from "react";
import { api } from "./api";
import { BackupDialog } from "./BackupDialog";
import { BackupSettingsSection } from "./BackupSettings";
import { readCaptureRoot, writeCaptureRoot } from "./captureSettings";
import { booksInLibraryView, groupBooks, matchesRatingFilters, seriesKey, type ScoreFilter, type SeriesShelf } from "./domain";
import { readFilterSections, writeFilterSections, type FilterSections } from "./filterSettings";
import type { AmazonContinueSeriesItem, Book, KindleOpenKind, KindleReaderMode, MetadataInput, SeriesMetadataInput, SeriesMode, SeriesPreferences, SeriesPreferencesInput, SyncResult } from "./types";

type LibraryView = "library" | "new-releases";

interface UiStateSnapshot {
  version: 1;
  openedSeries: string | null;
  selectedBookId: number | null;
}

const UI_STATE_STORAGE_KEY = "mmk.ui-state.v1";
const KINDLE_READER_MODE_STORAGE_KEY = "mmk.kindle-reader-mode.v1";
const KINDLE_PROFILE_PATH_STORAGE_KEY = "mmk.kindle-profile-path.v1";
const KOFI_LINK_VISIBLE_STORAGE_KEY = "mmk.kofi-link-visible.v1";
const EXCLUDE_PRE_RELEASE_NEW_RELEASES_STORAGE_KEY = "mmk.exclude-pre-release-new-releases.v1";

function readKindleReaderMode(): KindleReaderMode {
  try {
    const value = localStorage.getItem(KINDLE_READER_MODE_STORAGE_KEY);
    return value === "book" || value === "manga" || value === "auto" ? value : "auto";
  } catch {
    return "auto";
  }
}

function writeKindleReaderMode(mode: KindleReaderMode): void {
  try {
    localStorage.setItem(KINDLE_READER_MODE_STORAGE_KEY, mode);
  } catch {
    // 保存失敗は無視する。
  }
}

function readKindleProfilePath(): string {
  try {
    return localStorage.getItem(KINDLE_PROFILE_PATH_STORAGE_KEY) ?? "";
  } catch {
    return "";
  }
}

function writeKindleProfilePath(profilePath: string): void {
  try {
    if (profilePath.trim()) localStorage.setItem(KINDLE_PROFILE_PATH_STORAGE_KEY, profilePath.trim());
    else localStorage.removeItem(KINDLE_PROFILE_PATH_STORAGE_KEY);
  } catch {
    // 保存失敗は無視する。
  }
}

function readKofiLinkVisible(): boolean {
  try {
    return localStorage.getItem(KOFI_LINK_VISIBLE_STORAGE_KEY) !== "false";
  } catch {
    return true;
  }
}

function writeKofiLinkVisible(visible: boolean): void {
  try {
    if (visible) localStorage.removeItem(KOFI_LINK_VISIBLE_STORAGE_KEY);
    else localStorage.setItem(KOFI_LINK_VISIBLE_STORAGE_KEY, "false");
  } catch {
    // 保存失敗は無視する。
  }
}

function readExcludePreReleaseNewReleases(): boolean {
  try {
    return localStorage.getItem(EXCLUDE_PRE_RELEASE_NEW_RELEASES_STORAGE_KEY) !== "false";
  } catch {
    return true;
  }
}

function writeExcludePreReleaseNewReleases(exclude: boolean): void {
  try {
    if (exclude) localStorage.removeItem(EXCLUDE_PRE_RELEASE_NEW_RELEASES_STORAGE_KEY);
    else localStorage.setItem(EXCLUDE_PRE_RELEASE_NEW_RELEASES_STORAGE_KEY, "false");
  } catch {
    // 保存失敗は無視する。
  }
}

function inferKindleOpenKind(book: Book): KindleOpenKind {
  return book.label ? "manga" : "book";
}

function readUiState(): UiStateSnapshot | null {
  try {
    const raw = localStorage.getItem(UI_STATE_STORAGE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Partial<UiStateSnapshot> | null;
    if (!parsed || parsed.version !== 1) return null;
    return {
      version: 1,
      openedSeries: typeof parsed.openedSeries === "string" ? parsed.openedSeries : null,
      selectedBookId: typeof parsed.selectedBookId === "number" ? parsed.selectedBookId : null,
    };
  } catch {
    return null;
  }
}

function writeUiState(snapshot: UiStateSnapshot): void {
  try {
    localStorage.setItem(UI_STATE_STORAGE_KEY, JSON.stringify(snapshot));
  } catch {
    // 保存失敗は無視する。
  }
}

function getActiveSeriesKeys(books: Book[]): Set<string> {
  return new Set(books.filter((book) => book.sourceStatus === "active" && book.effectiveSeriesName).map((book) => seriesKey(book.effectiveSeriesName!)));
}

function resolveUiState(snapshot: UiStateSnapshot | null, books: Book[]): Pick<UiStateSnapshot, "openedSeries" | "selectedBookId"> {
  const activeBooks = books.filter((book) => book.sourceStatus === "active");
  const activeSeriesKeys = getActiveSeriesKeys(books);
  const selectedBook = snapshot?.selectedBookId == null ? null : activeBooks.find((book) => book.id === snapshot.selectedBookId) ?? null;
  const storedSeries = snapshot?.openedSeries && activeSeriesKeys.has(snapshot.openedSeries) ? snapshot.openedSeries : null;

  if (selectedBook) {
    const seriesKeyName = selectedBook.effectiveSeriesName ? seriesKey(selectedBook.effectiveSeriesName) : null;
    return {
      openedSeries: seriesKeyName && activeSeriesKeys.has(seriesKeyName) ? seriesKeyName : null,
      selectedBookId: selectedBook.id,
    };
  }

  if (storedSeries) {
    return {
      openedSeries: storedSeries,
      selectedBookId: null,
    };
  }

  return {
    openedSeries: null,
    selectedBookId: null,
  };
}

function parseTags(value: string): string[] {
  return [...new Set(value.split(/[,、\n]/).map((tag) => tag.trim()).filter(Boolean))];
}

function countFacetValues(values: string[]): [string, number][] {
  const counts = new Map<string, number>();
  for (const value of values) {
    const normalized = value.trim();
    if (normalized) counts.set(normalized, (counts.get(normalized) ?? 0) + 1);
  }
  return [...counts].sort((left, right) => left[0].localeCompare(right[0], "ja"));
}

function FacetList({ items, selected, emptyText, onToggle }: {
  items: [string, number][];
  selected: Set<string>;
  emptyText: string;
  onToggle: (value: string) => void;
}) {
  return <div className="facet-list">
    {items.length ? items.map(([value, count]) => <button
      key={value}
      className={`facet-item ${selected.has(value) ? "active" : ""}`}
      aria-pressed={selected.has(value)}
      onClick={() => onToggle(value)}
    ><span>{value}</span><small>{count}</small></button>) : <p className="muted">{emptyText}</p>}
  </div>;
}

function FilterSection({ id, title, open, count = 0, onToggle, children }: {
  id: keyof FilterSections;
  title: string;
  open: boolean;
  count?: number;
  onToggle: () => void;
  children: ReactNode;
}) {
  return <section className={`filter-section ${id === "tags" ? "tag-section" : ""}`}>
    <h3><button className="filter-section-toggle" aria-expanded={open} aria-controls={`filter-${id}`} onClick={onToggle}>
      <span aria-hidden="true">{open ? "▾" : "▸"}</span><span>{title}</span>{count > 0 && <small>{count}件選択</small>}
    </button></h3>
    <div id={`filter-${id}`} hidden={!open}>{children}</div>
  </section>;
}

function coverUrl(book: Book): string | undefined {
  return book.coverPath ? convertFileSrc(book.coverPath) : undefined;
}

function BookCover({ book }: { book: Book }) {
  const source = coverUrl(book);
  return source ? <img className="cover" src={source} alt="" /> : <div className="cover placeholder">表紙なし</div>;
}

function NewReleaseCover({ item }: { item: AmazonContinueSeriesItem }) {
  const [failed, setFailed] = useState(false);
  useEffect(() => setFailed(false), [item.imageUrl]);
  return item.imageUrl && !failed
    ? <img className="new-release-cover" src={item.imageUrl} alt="" onError={() => setFailed(true)} />
    : <div className="new-release-cover new-release-cover-placeholder">表紙なし</div>;
}

function BookCard({ book, selected, onSelect, onOpenKindle }: { book: Book; selected: boolean; onSelect: () => void; onOpenKindle: (book: Book) => void }) {
  return (
    <button className={`book-card ${selected ? "selected" : ""}`} onClick={onSelect} onDoubleClick={() => onOpenKindle(book)}>
      <BookCover book={book} />
      <span className="book-title">{book.title}</span>
      <span className="muted one-line">{book.authors.join("、") || "著者不明"}</span>
      <span className="card-meta"><span>{book.favorite ? "★" : "☆"}</span><span>評価 {book.score > 0 ? `+${book.score}` : book.score}</span></span>
    </button>
  );
}

function SeriesCard({ series, preferences, onSelect }: { series: SeriesShelf; preferences?: SeriesPreferences; onSelect: () => void }) {
  const score = preferences?.score ?? 0;
  return <button className="series-card" onClick={onSelect}>
    <div className="series-cover"><BookCover book={series.coverBook} /><span className="book-stack" /></div>
    <span className="book-title">{series.name}</span><span className="muted">{series.books.length}冊</span>
    <span className="card-meta"><span>{preferences?.favorite ? "★" : "☆"}</span><span>シリーズ評価 {score > 0 ? `+${score}` : score}</span>{preferences?.note?.trim() ? <span>メモあり</span> : null}</span>
  </button>;
}

interface NewReleaseSeries {
  key: string;
  name: string;
  latestOwnedVolume: number;
  recommendation: AmazonContinueSeriesItem;
}

function formatPrice(price: number | null): string {
  return price == null ? "価格不明" : `￥${price.toLocaleString("ja-JP")}`;
}

function App() {
  const [books, setBooks] = useState<Book[]>([]);
  const [seriesPreferences, setSeriesPreferences] = useState<SeriesPreferences[]>([]);
  const [amazonItems, setAmazonItems] = useState<AmazonContinueSeriesItem[]>([]);
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [query, setQuery] = useState("");
  const [favoriteOnly, setFavoriteOnly] = useState(false);
  const [scoreFilter, setScoreFilter] = useState<ScoreFilter>("all");
  const [selectedAuthors, setSelectedAuthors] = useState<Set<string>>(new Set());
  const [selectedPublishers, setSelectedPublishers] = useState<Set<string>>(new Set());
  const [selectedLabels, setSelectedLabels] = useState<Set<string>>(new Set());
  const [selectedTags, setSelectedTags] = useState<Set<string>>(new Set());
  const [openedSeries, setOpenedSeries] = useState<string | null>(null);
  const [activeView, setActiveView] = useState<LibraryView>("library");
  const [filterSections, setFilterSections] = useState(() => readFilterSections());
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [captureBook, setCaptureBook] = useState<Book | null>(null);
  const [captureOpen, setCaptureOpen] = useState(false);
  const [batchCaptureOpen, setBatchCaptureOpen] = useState(false);
  const [captureRoot, setCaptureRoot] = useState(readCaptureRoot);
  const [kindleReaderMode, setKindleReaderMode] = useState<KindleReaderMode>(() => readKindleReaderMode());
  const [kindleProfilePath, setKindleProfilePath] = useState(() => readKindleProfilePath());
  const [kofiLinkVisible, setKofiLinkVisible] = useState(() => readKofiLinkVisible());
  const [excludePreReleaseNewReleases, setExcludePreReleaseNewReleases] = useState(() => readExcludePreReleaseNewReleases());
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState("Kindle ライブラリを同期してください");
  const [uiHydrated, setUiHydrated] = useState(false);
  const initialUiStateRef = useRef<UiStateSnapshot | null>(null);
  const uiRestoreAppliedRef = useRef(false);

  useEffect(() => { writeCaptureRoot(captureRoot); }, [captureRoot]);
  useEffect(() => { writeFilterSections(filterSections); }, [filterSections]);

  const reload = async () => {
    const [result, recommendations, preferences] = await Promise.all([
      api.listBooks(),
      api.listAmazonContinueSeries(),
      api.listSeriesPreferences(),
    ]);
    setBooks(result);
    setAmazonItems(recommendations);
    setSeriesPreferences(preferences);
    setUiHydrated(true);
    if (result.length) setStatus(`${result.length.toLocaleString()} 冊`);
    return result;
  };

  useEffect(() => {
    initialUiStateRef.current = readUiState();
  }, []);

  useEffect(() => {
    reload().catch((error) => setStatus(String(error)));
  }, []);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    listen<number>("amazon-continue-series-updated", async ({ payload }) => {
      try {
        const recommendations = await api.listAmazonContinueSeries();
        if (disposed) return;
        setAmazonItems(recommendations);
        setStatus(`Amazonから候補を取得しました（${payload.toLocaleString()}件）`);
      } catch (error) {
        if (!disposed) setStatus(`Amazon候補の読み込みに失敗: ${String(error)}`);
      }
    }).then((remove) => {
      if (disposed) remove();
      else unlisten = remove;
    }).catch((error) => setStatus(`Amazon連携の準備に失敗: ${String(error)}`));
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    if (uiRestoreAppliedRef.current || !uiHydrated) return;
    const next = resolveUiState(initialUiStateRef.current, books);
    setOpenedSeries(next.openedSeries);
    setSelectedId(next.selectedBookId);
    uiRestoreAppliedRef.current = true;
    setUiHydrated(true);
  }, [books, uiHydrated]);

  useEffect(() => {
    if (!uiRestoreAppliedRef.current) return;
    const activeBooks = books.filter((book) => book.sourceStatus === "active");
    const activeSeriesKeys = getActiveSeriesKeys(books);
    const selectedBook = selectedId == null ? null : activeBooks.find((book) => book.id === selectedId) ?? null;
    const selectedBookSeriesKey = selectedBook?.effectiveSeriesName ? seriesKey(selectedBook.effectiveSeriesName) : null;
    const openedSeriesIsValid = openedSeries ? activeSeriesKeys.has(openedSeries) : false;

    if (!selectedBook && selectedId !== null) setSelectedId(null);
    if (selectedBook && selectedId !== selectedBook.id) setSelectedId(selectedBook.id);
    if (openedSeries && !openedSeriesIsValid) setOpenedSeries(null);
    if (selectedBook && selectedBookSeriesKey && openedSeries !== selectedBookSeriesKey && activeSeriesKeys.has(selectedBookSeriesKey)) {
      setOpenedSeries(selectedBookSeriesKey);
    }
  }, [books, openedSeries, selectedId]);

  useEffect(() => {
    if (!uiRestoreAppliedRef.current) return;
    writeUiState({ version: 1, openedSeries, selectedBookId: selectedId });
  }, [openedSeries, selectedId]);

  useEffect(() => {
    writeKindleReaderMode(kindleReaderMode);
  }, [kindleReaderMode]);

  useEffect(() => {
    writeKindleProfilePath(kindleProfilePath);
  }, [kindleProfilePath]);

  useEffect(() => {
    writeKofiLinkVisible(kofiLinkVisible);
  }, [kofiLinkVisible]);

  useEffect(() => {
    writeExcludePreReleaseNewReleases(excludePreReleaseNewReleases);
  }, [excludePreReleaseNewReleases]);

  const tagCounts = useMemo(() => {
    const counts = new Map<string, number>();
    for (const book of books) for (const tag of book.tags) counts.set(tag, (counts.get(tag) ?? 0) + 1);
    return [...counts].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0], "ja"));
  }, [books]);

  const authorCounts = useMemo(() => countFacetValues(
    books.filter((book) => book.sourceStatus === "active").flatMap((book) => [...new Set(book.authors)]),
  ), [books]);
  const publisherCounts = useMemo(() => countFacetValues(
    books.filter((book) => book.sourceStatus === "active" && book.publisher).map((book) => book.publisher!),
  ), [books]);

  const labelCounts = useMemo(() => countFacetValues(
    books.filter((book) => book.sourceStatus === "active" && book.label).map((book) => book.label!),
  ), [books]);

  const toggleFacet = (setter: Dispatch<SetStateAction<Set<string>>>, value: string) => {
    setter((current) => {
      const next = new Set(current);
      next.has(value) ? next.delete(value) : next.add(value);
      return next;
    });
  };

  const toggleFilterSection = (key: keyof FilterSections) => {
    setFilterSections((current) => ({ ...current, [key]: !current[key] }));
  };
  const preferencesByKey = useMemo(() => new Map(seriesPreferences.map((item) => [item.key, item])), [seriesPreferences]);

  const filtered = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase("ja");
    return books.filter((book) => {
      if (book.sourceStatus !== "active") return false;
      const preferences = book.effectiveSeriesName ? preferencesByKey.get(seriesKey(book.effectiveSeriesName)) : undefined;
      if (!matchesRatingFilters(book, preferences, favoriteOnly, scoreFilter)) return false;
      if (selectedAuthors.size > 0 && !book.authors.some((author) => selectedAuthors.has(author))) return false;
      if (selectedPublishers.size > 0 && (!book.publisher || !selectedPublishers.has(book.publisher))) return false;
      if (selectedLabels.size > 0 && (!book.label || !selectedLabels.has(book.label))) return false;
      if ([...selectedTags].some((tag) => !book.tags.includes(tag))) return false;
      if (!needle) return true;
      return [book.title, book.authors.join(" "), book.publisher ?? "", book.asin, book.effectiveSeriesName ?? "", book.tags.join(" ")]
        .join(" ").toLocaleLowerCase("ja").includes(needle);
    });
  }, [books, preferencesByKey, favoriteOnly, query, scoreFilter, selectedAuthors, selectedPublishers, selectedLabels, selectedTags]);

  const shelves = useMemo(() => groupBooks(filtered), [filtered]);
  const newReleaseSeries = useMemo<NewReleaseSeries[]>(() => {
    const owned = books.filter((book) => book.sourceStatus === "active" && book.effectiveSeriesName);
    const ownedAsins = new Set(owned.map((book) => book.asin));
    const candidates = new Map<string, NewReleaseSeries>();

    for (const item of amazonItems) {
      if (excludePreReleaseNewReleases && !item.hasRating) continue;
      if (ownedAsins.has(item.asin) || !item.seriesName || item.volumeIndex == null || !Number.isFinite(item.volumeIndex)) continue;
      const key = seriesKey(item.seriesName);
      const matchingBooks = owned.filter((book) => book.effectiveSeriesName && seriesKey(book.effectiveSeriesName) === key && book.effectiveSeriesIndex != null && Number.isFinite(book.effectiveSeriesIndex));
      if (!matchingBooks.length) continue;
      const latestOwnedVolume = Math.max(...matchingBooks.map((book) => book.effectiveSeriesIndex!));
      if (item.volumeIndex <= latestOwnedVolume) continue;
      const current = candidates.get(key);
      if (!current || item.volumeIndex < (current.recommendation.volumeIndex ?? Infinity)) {
        candidates.set(key, { key, name: item.seriesName, latestOwnedVolume, recommendation: item });
      }
    }

    return [...candidates.values()].sort((left, right) => left.name.localeCompare(right.name, "ja"));
  }, [amazonItems, books, excludePreReleaseNewReleases]);
  const openedSeriesName = openedSeries
    ? books.find((book) => book.effectiveSeriesName && seriesKey(book.effectiveSeriesName) === openedSeries)?.effectiveSeriesName ?? "シリーズ"
    : null;
  const openedSeriesBooks = openedSeries
    ? books.filter((book) => book.sourceStatus === "active" && book.effectiveSeriesName && seriesKey(book.effectiveSeriesName) === openedSeries)
    : [];
  const booksInView = useMemo(() => booksInLibraryView(filtered, openedSeries), [filtered, openedSeries]);
  const visibleBooks = openedSeries
    ? [...booksInView].sort((a, b) => (a.effectiveSeriesIndex ?? Infinity) - (b.effectiveSeriesIndex ?? Infinity))
    : shelves.singles;
  const selected = books.find((book) => book.id === selectedId) ?? null;
  const openedSeriesPreferences = openedSeries ? preferencesByKey.get(openedSeries) : undefined;

  const openKindle = (book: Book) => {
    const kind = kindleReaderMode === "auto" ? inferKindleOpenKind(book) : kindleReaderMode;
    api.openInKindle(book.asin, kind).catch((error) => setStatus(`Kindleを開けませんでした: ${String(error)}`));
  };

  const sync = async () => {
    setBusy(true);
    try {
      const result: SyncResult = await api.syncKindle(kindleProfilePath.trim() || null);
      await reload();
      setStatus(`同期完了: ${result.activeBooks.toLocaleString()}冊（新規 ${result.insertedBooks}、更新 ${result.updatedBooks}、除外 ${result.excludedBooks}）`);
    } catch (error) {
      setStatus(`同期失敗: ${String(error)}`);
    } finally {
      setBusy(false);
    }
  };

  const openAmazonChecker = async () => {
    setActiveView("new-releases");
    try {
      await api.openAmazonContinueSeries();
      setStatus("Amazonの新刊情報を確認しています。ログインが必要な場合だけ画面を表示します");
    } catch (error) {
      setStatus(`Amazon画面を開けませんでした: ${String(error)}`);
    }
  };

  const clearAmazonSession = async () => {
    try {
      await api.clearAmazonSession();
      setAmazonItems([]);
      setStatus("Amazonの連携データと専用WebViewのログイン状態を削除しました");
    } catch (error) {
      setStatus(`Amazon連携データを削除できませんでした: ${String(error)}`);
    }
  };

  const openKofi = () => {
    api.openKofiDonation().catch((error) => setStatus(`Ko-Fiを開けませんでした: ${String(error)}`));
  };

  const patchBook = (bookId: number, patch: Partial<Book>) => setBooks((current) => current.map((book) => book.id === bookId ? { ...book, ...patch } : book));
  const adjust = async (delta: number) => {
    if (!selected) return;
    const score = await api.adjustScore(selected.id, delta);
    patchBook(selected.id, { score });
  };
  const favorite = async () => {
    if (!selected) return;
    const value = !selected.favorite;
    await api.setFavorite(selected.id, value);
    patchBook(selected.id, { favorite: value });
  };
  const updateSeriesPreferences = async (input: SeriesPreferencesInput) => {
    const updated = await api.updateSeriesPreferences(input);
    setSeriesPreferences((current) => [...current.filter((item) => item.key !== updated.key), updated]);
  };

  const hasActiveFilters = query.trim() !== "" || favoriteOnly || scoreFilter !== "all" || selectedAuthors.size > 0 || selectedPublishers.size > 0 || selectedLabels.size > 0 || selectedTags.size > 0;
  const clearFilters = () => {
    setQuery("");
    setFavoriteOnly(false);
    setScoreFilter("all");
    setSelectedAuthors(new Set());
    setSelectedPublishers(new Set());
    setSelectedLabels(new Set());
    setSelectedTags(new Set());
  };
  const openSeriesFromNewRelease = (key: string) => {
    clearFilters();
    setActiveView("library");
    setOpenedSeries(key);
    setSelectedId(null);
  };
  const activeFilterChips = ([
    query.trim() && { key: "query", label: `検索: ${query.trim()}` },
    favoriteOnly && { key: "favorite", label: "★ お気に入り" },
    scoreFilter !== "all" && { key: "score", label: scoreFilter === "positive" ? "評価 ＋" : "評価 −" },
    selectedAuthors.size > 0 && { key: "authors", label: `著者: ${[...selectedAuthors].join("、")}` },
    selectedPublishers.size > 0 && { key: "publishers", label: `出版社: ${[...selectedPublishers].join("、")}` },
    selectedLabels.size > 0 && { key: "labels", label: `レーベル: ${[...selectedLabels].join("、")}` },
    selectedTags.size > 0 && { key: "tags", label: `タグ: ${[...selectedTags].join("、")}` },
  ].filter(Boolean) as { key: string; label: string }[]);

  return (
    <main className="app-shell">
      <aside className="panel filter-panel">
        <header className="panel-header"><h2>フィルター</h2></header>
        {hasActiveFilters && <div className="active-filters" role="status">
          <div className="active-filter-chips">{activeFilterChips.map((chip) => <span key={chip.key} className="active-filter-chip" title={chip.label}>{chip.label}</span>)}</div>
          <button className="icon-button" onClick={clearFilters} aria-label="選択中のフィルターを解除">×</button>
        </div>}
        <input className="search" type="search" value={query} onChange={(event) => setQuery(event.target.value)} placeholder="タイトル、著者、タグ、ASIN" />
        <FilterSection id="tags" title="タグ" open={filterSections.tags} count={selectedTags.size} onToggle={() => toggleFilterSection("tags")}><div className="tag-cloud">
          {tagCounts.length ? tagCounts.map(([tag, count]) => <button key={tag} className={selectedTags.has(tag) ? "tag active" : "tag"} aria-pressed={selectedTags.has(tag)} onClick={() => toggleFacet(setSelectedTags, tag)}>{tag}<small>{count}</small></button>) : <p className="muted">タグはまだありません</p>}
        </div></FilterSection>
        <FilterSection id="rating" title="お気に入り・評価" open={filterSections.rating} count={Number(favoriteOnly) + Number(scoreFilter !== "all")} onToggle={() => toggleFilterSection("rating")}>
        <button className={`filter-button ${favoriteOnly ? "active" : ""}`} aria-pressed={favoriteOnly} onClick={() => setFavoriteOnly(!favoriteOnly)}>★ お気に入り</button>
        <div className="segmented rating-filter">
          <button className={scoreFilter === "all" ? "active" : ""} onClick={() => setScoreFilter("all")}>すべて</button>
          <button className={scoreFilter === "positive" ? "active" : ""} onClick={() => setScoreFilter("positive")}>＋</button>
          <button className={scoreFilter === "negative" ? "active" : ""} onClick={() => setScoreFilter("negative")}>−</button>
        </div><p className="muted rating-filter-hint">書籍またはシリーズの登録が対象です。</p></FilterSection>
        <FilterSection id="authors" title="著者" open={filterSections.authors} count={selectedAuthors.size} onToggle={() => toggleFilterSection("authors")}><FacetList items={authorCounts} selected={selectedAuthors} emptyText="著者情報はありません" onToggle={(value) => toggleFacet(setSelectedAuthors, value)} /></FilterSection>
        <FilterSection id="publishers" title="出版社" open={filterSections.publishers} count={selectedPublishers.size} onToggle={() => toggleFilterSection("publishers")}><FacetList items={publisherCounts} selected={selectedPublishers} emptyText="出版社情報はありません" onToggle={(value) => toggleFacet(setSelectedPublishers, value)} /></FilterSection>
        <FilterSection id="labels" title="レーベル" open={filterSections.labels} count={selectedLabels.size} onToggle={() => toggleFilterSection("labels")}><FacetList items={labelCounts} selected={selectedLabels} emptyText="レーベル情報はありません" onToggle={(value) => toggleFacet(setSelectedLabels, value)} /></FilterSection>
      </aside>

      <section className="library-pane">
        <header className="library-tab-bar">
          <nav className="library-tabs" role="tablist" aria-label="中央ペインの表示">
            <button id="library-tab" role="tab" aria-selected={activeView === "library"} aria-controls="library-panel" className={activeView === "library" ? "active" : ""} onClick={() => setActiveView("library")}>ライブラリ</button>
            <button id="new-releases-tab" role="tab" aria-selected={activeView === "new-releases"} aria-controls="new-releases-panel" className={activeView === "new-releases" ? "active" : ""} onClick={openAmazonChecker}>新刊チェック<span className="tab-count">{newReleaseSeries.length}</span></button>
          </nav>
          <div className="library-actions" role="group" aria-label="ライブラリの操作">
            <button className="primary" disabled={busy} onClick={sync}>{busy ? "同期中…" : "Kindleと同期"}</button>
            <button className="icon-button" onClick={() => setSettingsOpen(true)} aria-label="設定を開く">⚙</button>
            {kofiLinkVisible && <button className="kofi-link" title="Ko-Fi でコーヒーをおごる" onClick={openKofi}>☕ コーヒーをおごる</button>}
          </div>
        </header>
        <div className="series-navigation">
          <div className="series-navigation-main">{activeView === "library" && openedSeries && <button className="back-button" onClick={() => { setOpenedSeries(null); setSelectedId(null); }}>← ライブラリ</button>}
            <div className="view-heading"><span className="muted">{activeView === "new-releases" ? "Amazonの続刊候補" : openedSeries ? "シリーズ" : hasActiveFilters ? "絞り込み結果" : "すべての書籍"}</span><h2>{activeView === "new-releases" ? "新刊チェック" : openedSeriesName ?? (hasActiveFilters ? activeFilterChips.map((chip) => chip.label).join(" / ") : "ライブラリ")}</h2>
              {activeView === "library" && openedSeries && hasActiveFilters && <p className="view-filter-summary">{activeFilterChips.map((chip) => chip.label).join(" / ")}</p>}
            </div>
          </div>
          {activeView === "library" && <button className="secondary toolbar-button" onClick={() => setBatchCaptureOpen(true)}>一括バックアップ…</button>}
        </div>
        <div className="library-scroll">
          {activeView === "new-releases" ? <section id="new-releases-panel" role="tabpanel" aria-labelledby="new-releases-tab" className="new-release-section">
            <div className="section-heading"><h2 className="section-title">新刊チェック</h2><span className="muted">{newReleaseSeries.length}シリーズ</span></div>
            {newReleaseSeries.length > 0 ? <div className="new-release-list">
              {newReleaseSeries.map((series) => <article className="new-release-card" key={series.key}>
                <div className="new-release-main">
                  <NewReleaseCover item={series.recommendation} />
                  <div className="new-release-copy"><button className="new-release-series-link" onClick={() => openSeriesFromNewRelease(series.key)} aria-label={`${series.name}の巻一覧を表示`}>{series.name}</button><span className="muted">手元の最新: {series.latestOwnedVolume}巻</span><span className="new-release-title">続刊候補: {series.recommendation.title}</span><span className="muted">{formatPrice(series.recommendation.price)}</span></div>
                </div>
                <button className="secondary new-release-button" onClick={() => api.openStorePage(series.recommendation.asin)}>Amazonで確認 ↗</button>
              </article>)}
            </div> : <div className="empty-state new-release-empty"><strong>続刊候補はありません</strong><span>「新刊チェック」を実行すると、Amazonの推薦と手元の巻数を照合します。</span></div>}
            <div className="new-release-footer"><span className="hint">Amazonの推薦と手元の巻数を照合した候補です。未購入の確定情報ではありません。</span><button className="library-link" onClick={clearAmazonSession}>Amazon連携を解除</button></div>
          </section> : <div id="library-panel" role="tabpanel" aria-labelledby="library-tab">
            {!openedSeries && shelves.series.length > 0 && <><h2 className="section-title">シリーズ</h2><div className="book-grid">
              {shelves.series.map((series) => <SeriesCard key={series.key} series={series} preferences={preferencesByKey.get(series.key)} onSelect={() => { setOpenedSeries(series.key); setSelectedId(null); }} />)}
            </div></>}
            <h2 className="section-title">{openedSeries ? "巻一覧" : "書籍"}</h2>
            <div className="book-grid">{visibleBooks.map((book) => <BookCard key={book.id} book={book} selected={book.id === selectedId} onSelect={() => setSelectedId(book.id)} onOpenKindle={openKindle} />)}</div>
            {!visibleBooks.length && (!shelves.series.length || openedSeries) && <div className="empty-state"><strong>表示する本がありません</strong><span>同期またはフィルター条件を確認してください。</span></div>}
          </div>}
        </div>
        <footer className="library-status" role="status" title={status}>{status}</footer>
      </section>

      {settingsOpen
        ? <SettingsPanel mode={kindleReaderMode} onChange={setKindleReaderMode} profilePath={kindleProfilePath} onProfilePathChange={setKindleProfilePath} captureRoot={captureRoot} onCaptureRootChange={setCaptureRoot} kofiLinkVisible={kofiLinkVisible} onKofiLinkVisibleChange={setKofiLinkVisible} excludePreReleaseNewReleases={excludePreReleaseNewReleases} onExcludePreReleaseNewReleasesChange={setExcludePreReleaseNewReleases} onClose={() => setSettingsOpen(false)} />
        : openedSeries && !selected
        ? <SeriesMetadataPanel
            key={openedSeries}
            name={openedSeriesName ?? "シリーズ"}
            books={openedSeriesBooks}
            preferences={openedSeriesPreferences ?? { key: openedSeries, score: 0, favorite: false, note: "" }}
            onPreferencesUpdated={updateSeriesPreferences}
            onUpdated={async (nextName) => {
              await reload();
              if (nextName) setOpenedSeries(seriesKey(nextName));
              setStatus("シリーズメタデータを更新しました");
            }}
          />
        : <MetadataPanel book={selected} onSeriesSelect={openedSeries ? () => setSelectedId(null) : undefined} onAdjust={adjust} onFavorite={favorite} onSaved={async () => { await reload(); setStatus("メタデータを保存しました"); }} onOpenKindle={openKindle} onCapture={(book) => { setCaptureBook(book); setCaptureOpen(true); }} />}
      <BackupDialog books={captureBook ? [captureBook] : []} open={captureOpen} batch={false} outputDirectory={captureRoot} onClose={() => setCaptureOpen(false)} />
      <BackupDialog books={booksInView} open={batchCaptureOpen} batch outputDirectory={captureRoot} onClose={() => setBatchCaptureOpen(false)} />
    </main>
  );
}

function SeriesMetadataPanel({ name, books, preferences, onPreferencesUpdated, onUpdated }: {
  name: string;
  books: Book[];
  preferences: SeriesPreferences;
  onPreferencesUpdated: (input: SeriesPreferencesInput) => Promise<void>;
  onUpdated: (nextName?: string) => Promise<void>;
}) {
  const [seriesName, setSeriesName] = useState(name);
  const [tagText, setTagText] = useState("");
  const [noteText, setNoteText] = useState(preferences.note);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  useEffect(() => setSeriesName(name), [name]);
  useEffect(() => setNoteText(preferences.note), [preferences.note]);

  const hasBooks = books.length > 0;
  const baseInput = (): SeriesMetadataInput => ({
    bookIds: books.map((book) => book.id),
    scoreDelta: 0,
    favorite: null,
    renameTo: null,
    tagsToAdd: [],
    tagsToRemove: [],
  });
  const apply = async (input: SeriesMetadataInput, successMessage: string, nextName?: string) => {
    setBusy(true);
    setMessage("");
    try {
      await api.updateSeriesMetadata(input);
      await onUpdated(nextName);
      setMessage(successMessage);
      return true;
    } catch (error) {
      setMessage(`更新失敗: ${String(error)}`);
      return false;
    } finally {
      setBusy(false);
    }
  };
  const updatePreferences = async (scoreDelta: number, favorite: boolean | null = null) => {
    setBusy(true);
    setMessage("");
    try {
      await onPreferencesUpdated({ key: preferences.key, scoreDelta, favorite, note: null });
      setMessage("シリーズの評価・お気に入りを保存しました");
    } catch (error) {
      setMessage(`更新失敗: ${String(error)}`);
    } finally {
      setBusy(false);
    }
  };
  const saveNote = async () => {
    setBusy(true);
    setMessage("");
    try {
      await onPreferencesUpdated({ key: preferences.key, scoreDelta: 0, favorite: null, note: noteText });
      setMessage("シリーズの備考を保存しました");
    } catch (error) {
      setMessage(`更新失敗: ${String(error)}`);
    } finally {
      setBusy(false);
    }
  };
  const tags = parseTags(tagText);
  const trimmedSeriesName = seriesName.trim();

  return <aside className="panel metadata-panel series-metadata-panel" aria-busy={busy}>
    <header className="panel-header"><h2>シリーズメタデータ</h2></header>
    <p className="series-summary"><strong>{name}</strong><span>{books.length}冊</span></p>
    <label>シリーズの評価スコア<div className="score-editor"><button disabled={busy || !hasBooks} onClick={() => updatePreferences(-1)}>−</button><output>{preferences.score > 0 ? `+${preferences.score}` : preferences.score}</output><button disabled={busy || !hasBooks} onClick={() => updatePreferences(1)}>＋</button></div></label>
    <button className={`favorite-button ${preferences.favorite ? "active" : ""}`} aria-pressed={preferences.favorite} disabled={busy || !hasBooks} onClick={() => updatePreferences(0, !preferences.favorite)}>{preferences.favorite ? "★ シリーズのお気に入り" : "☆ シリーズをお気に入りに追加"}</button>
    <p className="hint">シリーズ自身に保存します。各巻の評価・お気に入りは変更しません。</p>
    <label>シリーズの備考<textarea rows={3} value={noteText} onChange={(event) => setNoteText(event.target.value)} placeholder="どういう話だったか、どこまで読んだかなど" /></label>
    <button className="secondary centered" disabled={busy || noteText === preferences.note} onClick={saveNote}>備考を保存</button>
    <label>シリーズ名<input value={seriesName} onChange={(event) => setSeriesName(event.target.value)} /></label>
    <button className="secondary centered" disabled={busy || !hasBooks || !trimmedSeriesName || trimmedSeriesName === name} onClick={async () => { await apply({ ...baseInput(), renameTo: trimmedSeriesName, renameFromKey: preferences.key, renameToKey: seriesKey(trimmedSeriesName) }, "シリーズ名を全巻へ適用しました", trimmedSeriesName); }}>シリーズ名を全巻へ適用</button>
    <label>一括タグ<textarea rows={3} value={tagText} onChange={(event) => setTagText(event.target.value)} placeholder="カンマ区切り" /></label>
    <div className="series-button-row"><button disabled={busy || !hasBooks || tags.length === 0} onClick={async () => { const ok = await apply({ ...baseInput(), tagsToAdd: tags }, "タグを全巻へ追加しました"); if (ok) setTagText(""); }}>全巻へ追加</button><button disabled={busy || !hasBooks || tags.length === 0} onClick={async () => { const ok = await apply({ ...baseInput(), tagsToRemove: tags }, "タグを全巻から削除しました"); if (ok) setTagText(""); }}>全巻から削除</button></div>
    {message && <p className="hint" role="status">{message}</p>}
    <p className="hint">シリーズ名と一括タグの操作は、現在の絞り込みにかかわらずシリーズ全巻へ反映されます。</p>
    {!hasBooks && <p className="hint">対象の有効な巻が見つかりませんでした。</p>}
  </aside>;
}

function SettingsPanel({ mode, onChange, profilePath, onProfilePathChange, captureRoot, onCaptureRootChange, kofiLinkVisible, onKofiLinkVisibleChange, excludePreReleaseNewReleases, onExcludePreReleaseNewReleasesChange, onClose }: {
  mode: KindleReaderMode;
  onChange: (mode: KindleReaderMode) => void;
  profilePath: string;
  onProfilePathChange: (profilePath: string) => void;
  captureRoot: string;
  onCaptureRootChange: (path: string) => void;
  kofiLinkVisible: boolean;
  onKofiLinkVisibleChange: (visible: boolean) => void;
  excludePreReleaseNewReleases: boolean;
  onExcludePreReleaseNewReleasesChange: (exclude: boolean) => void;
  onClose: () => void;
}) {
  const [choosingRoot, setChoosingRoot] = useState(false);
  const [captureError, setCaptureError] = useState("");
  const [appVersion, setAppVersion] = useState("読み込み中…");
  useEffect(() => {
    let active = true;
    getVersion().then(
      (version) => { if (active) setAppVersion(version); },
      () => { if (active) setAppVersion("取得できませんでした"); },
    );
    return () => { active = false; };
  }, []);
  return <aside className="panel settings-panel">
    <header className="panel-header"><h2>設定</h2><button className="icon-button" onClick={onClose} aria-label="設定を閉じる">×</button></header>
    <section>
      <h3>バックアップ保存先</h3>
      <label className="settings-path-label">保存先ルートフォルダー（全体共通）<input value={captureRoot} onChange={(event) => onCaptureRootChange(event.target.value)} placeholder="空欄: ピクチャ\\MMK" spellCheck={false} disabled={choosingRoot} /></label>
      <button className="secondary centered" disabled={choosingRoot} onClick={async () => {
        setChoosingRoot(true); setCaptureError("");
        try { const path = await api.chooseCaptureRoot(captureRoot); if (path !== null) onCaptureRootChange(path); }
        catch (error) { setCaptureError(String(error)); }
        finally { setChoosingRoot(false); }
      }}>保存先ルートフォルダーを選択…</button>
      <button className="library-link" disabled={choosingRoot || !captureRoot} onClick={() => onCaptureRootChange("")}>既定の保存先に戻す</button>
      <p className="hint">シリーズ名／書籍名.cbz（シリーズ未設定：シリーズなし／書籍名.cbz）</p>
      {captureError && <p className="capture-error" role="alert">{captureError}</p>}
    </section>
    <BackupSettingsSection />
    <section>
      <h3>Kindleで読むURL</h3>
      <label className="settings-select-label">開き方<select value={mode} onChange={(event) => onChange(event.target.value as KindleReaderMode)}>
        <option value="auto">自動判定</option>
        <option value="book">常に本（?asin=）</option>
        <option value="manga">常にマンガ（/manga/）</option>
      </select></label>
      <p className="hint">自動判定では、同期時に取得した刊行レーベルがある書籍をマンガ、それ以外を本として開きます。判定できないものは本として扱います。</p>
    </section>
    <section>
      <h3>Kindleプロフィール</h3>
      <label className="settings-path-label">読み込む場所<input
        type="text"
        value={profilePath}
        onChange={(event) => onProfilePathChange(event.target.value)}
        placeholder="既定の場所を自動検出"
        spellCheck={false}
      /></label>
      <button className="secondary centered" onClick={() => onProfilePathChange("")} disabled={!profilePath}>既定の場所に戻す</button>
      <p className="hint"><code>KindleSyncMetadataCache.xml</code> を含むフォルダー、または XML ファイルのフルパスを指定できます。空欄では <code>%LOCALAPPDATA%\Packages</code> から自動検出します。</p>
    </section>
    <section>
      <h3>表示</h3>
      <label className="settings-check-label"><input type="checkbox" checked={excludePreReleaseNewReleases} onChange={(event) => onExcludePreReleaseNewReleasesChange(event.target.checked)} />発売前の新刊を除外</label>
      <p className="hint">評価が表示されていない予約商品とみられる新刊候補を表示しません。判定はAmazonカードの評価表示に基づきます。</p>
    </section>
    <label className="settings-check-label"><input type="checkbox" checked={kofiLinkVisible} onChange={(event) => onKofiLinkVisibleChange(event.target.checked)} />「コーヒーをおごる」リンクを表示</label>
    <footer className="settings-version muted" aria-label="バージョン情報">
      <strong>Manga Manager for Kindle</strong>
      <span role="status">バージョン {appVersion}</span>
    </footer>
  </aside>;
}

function MetadataPanel({ book, onSeriesSelect, onAdjust, onFavorite, onSaved, onOpenKindle, onCapture }: { book: Book | null; onSeriesSelect?: () => void; onAdjust: (delta: number) => Promise<void>; onFavorite: () => Promise<void>; onSaved: () => Promise<void>; onOpenKindle: (book: Book) => void; onCapture: (book: Book) => void }) {
  const [draft, setDraft] = useState<MetadataInput | null>(null);
  const [saving, setSaving] = useState(false);
  useEffect(() => {
    setDraft(book ? { bookId: book.id, score: book.score, favorite: book.favorite, seriesMode: book.seriesMode, manualSeriesName: book.manualSeriesName, manualSeriesIndex: book.manualSeriesIndex, manualTitle: book.manualTitle, manualAuthors: book.manualAuthors, manualPublisher: book.manualPublisher, tags: book.tags } : null);
  }, [book]);
  if (!book || !draft) return <aside className="panel metadata-panel"><header className="panel-header"><h2>メタデータ</h2></header><p className="muted">書籍またはシリーズを選択してください</p></aside>;
  const update = <K extends keyof MetadataInput>(key: K, value: MetadataInput[K]) => setDraft({ ...draft, [key]: value });
  const save = async () => { setSaving(true); try { await api.saveMetadata(draft); await onSaved(); } finally { setSaving(false); } };
  return <aside className="panel metadata-panel">
    <header className="panel-header"><h2>書籍メタデータ</h2></header>
    {onSeriesSelect && <button className="library-link series-metadata-link" onClick={onSeriesSelect}>← シリーズのメタデータへ</button>}
    <div className="detail-cover"><BookCover book={book} /></div>
    <button className="read-button" onClick={() => onOpenKindle(book)}>Kindleで読む ↗</button>
    <button className="secondary centered capture-launch" onClick={() => onCapture(book)}>バックアップ…</button>
    <button className="read-button store-button" onClick={() => api.openStorePage(book.asin)}>ストアページを開く ↗</button>
    <button className="library-link" onClick={() => api.openKindleLibrary()}>Kindleライブラリを開く</button>
    <label>評価スコア<div className="score-editor"><button onClick={async () => { await onAdjust(-1); update("score", draft.score - 1); }}>−</button><output>{draft.score > 0 ? `+${draft.score}` : draft.score}</output><button onClick={async () => { await onAdjust(1); update("score", draft.score + 1); }}>＋</button></div></label>
    <button className={`favorite-button ${book.favorite ? "active" : ""}`} onClick={async () => { await onFavorite(); update("favorite", !draft.favorite); }}>{book.favorite ? "★ お気に入り" : "☆ お気に入りに追加"}</button>
    <label>タイトル上書き<input value={draft.manualTitle ?? ""} placeholder={book.title} onChange={(event) => update("manualTitle", event.target.value || null)} /></label>
    <label>著者上書き<input value={draft.manualAuthors ?? ""} placeholder={book.authors.join("、")} onChange={(event) => update("manualAuthors", event.target.value || null)} /></label>
    <label>出版社上書き<input value={draft.manualPublisher ?? ""} placeholder={book.publisher ?? ""} onChange={(event) => update("manualPublisher", event.target.value || null)} /></label>
    <label>シリーズ設定<select value={draft.seriesMode} onChange={(event) => update("seriesMode", event.target.value as SeriesMode)}><option value="auto">自動</option><option value="manual">手動</option><option value="none">シリーズなし</option></select></label>
    {draft.seriesMode === "auto" && <p className="hint">推定: {book.inferredSeriesName ? `${book.inferredSeriesName} / ${book.inferredSeriesIndex ?? "?"}巻` : "候補なし"}</p>}
    {draft.seriesMode === "manual" && <><label>シリーズ名<input value={draft.manualSeriesName ?? ""} onChange={(event) => update("manualSeriesName", event.target.value)} /></label><label>巻数<input type="number" step="0.1" value={draft.manualSeriesIndex ?? ""} onChange={(event) => update("manualSeriesIndex", event.target.value === "" ? null : Number(event.target.value))} /></label></>}
    <label>タグ<textarea rows={3} value={draft.tags.join(", ")} onChange={(event) => update("tags", parseTags(event.target.value))} placeholder="カンマ区切り" /></label>
    <dl><dt>ASIN</dt><dd>{book.asin}</dd><dt>レーベル</dt><dd>{book.label ?? "—"}</dd><dt>取得種別</dt><dd>{book.ownershipType}</dd><dt>購入・取得日</dt><dd>{book.acquisitionDate?.slice(0, 10) ?? "不明"}</dd></dl>
    <button className="primary save-button" disabled={saving} onClick={save}>{saving ? "保存中…" : "変更を保存"}</button>
  </aside>;
}

export default App;
