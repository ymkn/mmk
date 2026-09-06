fn main() {
    let attributes =
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(&[
            "list_books",
            "sync_kindle",
            "save_book_metadata",
            "adjust_score",
            "set_favorite",
            "update_series_metadata",
            "open_in_kindle",
            "open_amazon_product_page",
            "open_kindle_library",
            "open_kofi_donation",
            "open_amazon_continue_series",
            "list_amazon_continue_series",
            "show_amazon_login",
            "store_amazon_continue_series",
            "clear_amazon_session",
        ]));
    tauri_build::try_build(attributes).expect("failed to build Tauri application metadata");
}
