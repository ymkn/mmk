export type SeriesMode = "auto" | "manual" | "none";
export type KindleReaderMode = "auto" | "book" | "manga";
export type KindleOpenKind = "book" | "manga";

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
}
