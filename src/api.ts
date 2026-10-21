import { invoke } from "@tauri-apps/api/core";
import { createBackupSettingsStore } from "./backupSettingsStore";
import type { AmazonContinueSeriesItem, BackupSettings, BatchCaptureOptions, Book, CaptureOptions, CapturePreview, CaptureStatus, CaptureWindow, KindleOpenKind, MetadataInput, SeriesMetadataInput, SeriesPreferences, SeriesPreferencesInput, SyncResult } from "./types";

const backupSettings = createBackupSettingsStore(
  () => invoke<BackupSettings>("get_backup_settings"),
  (settings) => invoke<void>("save_backup_settings", { settings }),
);

export const api = {
  listBooks: () => invoke<Book[]>("list_books"),
  syncKindle: (profilePath: string | null) => invoke<SyncResult>("sync_kindle", { profilePath }),
  saveMetadata: (input: MetadataInput) => invoke<void>("save_book_metadata", { input }),
  adjustScore: (bookId: number, delta: number) => invoke<number>("adjust_score", { bookId, delta }),
  setFavorite: (bookId: number, favorite: boolean) => invoke<void>("set_favorite", { bookId, favorite }),
  updateSeriesMetadata: (input: SeriesMetadataInput) => invoke<void>("update_series_metadata", { input }),
  listSeriesPreferences: () => invoke<SeriesPreferences[]>("list_series_preferences"),
  updateSeriesPreferences: (input: SeriesPreferencesInput) => invoke<SeriesPreferences>("update_series_preferences", { input }),
  openInKindle: (asin: string, kind: KindleOpenKind) => invoke<void>("open_in_kindle", { asin, kind }),
  openStorePage: (asin: string) => invoke<void>("open_amazon_product_page", { asin }),
  openKindleLibrary: () => invoke<void>("open_kindle_library"),
  openKofiDonation: () => invoke<void>("open_kofi_donation"),
  openAmazonContinueSeries: () => invoke<void>("open_amazon_continue_series"),
  listAmazonContinueSeries: () => invoke<AmazonContinueSeriesItem[]>("list_amazon_continue_series"),
  clearAmazonSession: () => invoke<void>("clear_amazon_session"),
  listCaptureWindows: () => invoke<CaptureWindow[]>("list_capture_windows"),
  openCaptureReader: (asin: string, portrait: boolean) => invoke<CaptureWindow>("open_capture_reader", { asin, portrait }),
  clearCaptureReaderSession: () => invoke<void>("clear_capture_reader_session"),
  startPageCapture: (options: CaptureOptions) => invoke<void>("start_page_capture", { options }),
  startBatchCapture: (options: BatchCaptureOptions) => backupSettings.afterPending(() => invoke<void>("start_batch_capture", { options })),
  listPendingBackups: (outputDirectory: string, bookIds: number[]) => invoke<number[]>("list_pending_backups", { outputDirectory, bookIds }),
  getBackupSettings: backupSettings.load,
  saveBackupSettings: backupSettings.save,
  pageCaptureStatus: () => invoke<CaptureStatus>("page_capture_status"),
  stopPageCapture: () => invoke<void>("stop_page_capture"),
  openCaptureDirectory: () => invoke<void>("open_capture_directory"),
  previewCaptureLayout: (setup: { handle: string; processId: number; portrait: boolean }) => invoke<CapturePreview>("preview_capture_layout", { setup }),
  chooseCaptureRoot: (current: string) => invoke<string | null>("choose_capture_root", { current }),
};
