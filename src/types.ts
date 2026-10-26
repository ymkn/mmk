export type SeriesMode = "auto" | "manual" | "none";
export type KindleReaderMode = "auto" | "book" | "manga";
export type KindleOpenKind = "book" | "manga";

export interface CaptureWindow {
  handle: string;
  processId: number;
  title: string;
  width: number;
  height: number;
}

export interface CaptureOptions {
  handle: string;
  processId: number;
  asin: string;
  bookTitle: string;
  outputDirectory: string;
  region: { x: number; y: number; width: number; height: number };
  key: "left" | "right" | "pagedown" | "space";
  maxPages: number;
  autoPage: boolean;
  portrait: boolean;
  fromBeginning: boolean;
  rewindOnly: boolean;
}

export interface CapturePreview {
  window: CaptureWindow;
  region: CaptureOptions["region"] | null;
  image: string;
  source: string;
  warning: string | null;
}

export interface CaptureStatus {
  running: boolean;
  savedPages: number;
  unchangedCount: number;
  outputDirectory: string;
  message: string;
  detectedRegion: CaptureOptions["region"] | null;
  batch: boolean;
  totalBooks: number;
  completedBooks: number;
  skippedBooks: number;
  currentTitle: string;
  currentPosition: number | null;
  totalPositions: number | null;
  logPath: string | null;
  logError: string | null;
}

export interface BatchCaptureOptions {
  bookIds: number[];
  outputDirectory: string;
}

export interface BackupSettings {
  pageLayout: "single" | "spread";
  configPath: string;
}

export interface Book {
  id: number;
  asin: string;
  title: string;
  authors: string[];
  publisher: string | null;
  publicationDate: string | null;
  acquisitionDate: string | null;
  ownershipType: string;
  sourceOrigin: string;
  sourceStatus: string;
  coverPath: string | null;
  score: number;
  favorite: boolean;
  seriesMode: SeriesMode;
  inferredSeriesName: string | null;
  inferredSeriesIndex: number | null;
  manualSeriesName: string | null;
  manualSeriesIndex: number | null;
  effectiveSeriesName: string | null;
  effectiveSeriesIndex: number | null;
  manualTitle: string | null;
  manualAuthors: string | null;
  manualPublisher: string | null;
  label: string | null;
  tags: string[];
}

export interface SyncResult {
  sourcePath: string;
  activeBooks: number;
  insertedBooks: number;
  updatedBooks: number;
  excludedBooks: number;
  missingBooks: number;
}

export interface AmazonContinueSeriesItem {
  asin: string;
  title: string;
  productUrl: string;
  price: number | null;
  hasRating: boolean;
  imageUrl: string | null;
  seriesName: string | null;
  volumeIndex: number | null;
  seenAt: string;
}

export interface MetadataInput {
  bookId: number;
  score: number;
  favorite: boolean;
  seriesMode: SeriesMode;
  manualSeriesName: string | null;
  manualSeriesIndex: number | null;
  manualTitle: string | null;
  manualAuthors: string | null;
  manualPublisher: string | null;
  tags: string[];
}

export interface SeriesMetadataInput {
  bookIds: number[];
  scoreDelta: number;
  favorite: boolean | null;
  renameTo: string | null;
  tagsToAdd: string[];
  tagsToRemove: string[];
  renameFromKey?: string;
  renameToKey?: string;
}

export interface SeriesPreferences {
  key: string;
  score: number;
  favorite: boolean;
  note: string;
}

export interface SeriesPreferencesInput {
  key: string;
  scoreDelta: number;
  favorite: boolean | null;
  note: string | null;
}
