import { invoke } from "@tauri-apps/api/core";
import type { AmazonContinueSeriesItem, Book, KindleOpenKind, MetadataInput, SeriesMetadataInput, SyncResult } from "./types";

export const api = {
  listBooks: () => invoke<Book[]>("list_books"),
  syncKindle: (profilePath: string | null) => invoke<SyncResult>("sync_kindle", { profilePath }),
  saveMetadata: (input: MetadataInput) => invoke<void>("save_book_metadata", { input }),
  adjustScore: (bookId: number, delta: number) => invoke<number>("adjust_score", { bookId, delta }),
  setFavorite: (bookId: number, favorite: boolean) => invoke<void>("set_favorite", { bookId, favorite }),
  updateSeriesMetadata: (input: SeriesMetadataInput) => invoke<void>("update_series_metadata", { input }),
  openInKindle: (asin: string, kind: KindleOpenKind) => invoke<void>("open_in_kindle", { asin, kind }),
  openStorePage: (asin: string) => invoke<void>("open_amazon_product_page", { asin }),
  openKindleLibrary: () => invoke<void>("open_kindle_library"),
  openKofiDonation: () => invoke<void>("open_kofi_donation"),
  openAmazonContinueSeries: () => invoke<void>("open_amazon_continue_series"),
  listAmazonContinueSeries: () => invoke<AmazonContinueSeriesItem[]>("list_amazon_continue_series"),
  clearAmazonSession: () => invoke<void>("clear_amazon_session"),
};
