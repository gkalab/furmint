mod highlight;
mod image;
mod text;

use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub use highlight::{
    HighlightBatch, HighlightRequest, HighlightWorker, LineSegments, parse_hex_color,
};
pub use image::{ImageLoadResult, ImageViewerState};
pub use text::{ContentLoadResult, TextViewerState};

/// Top-level state for the file viewer panel.
///
/// Owns the shared panel state (path, visibility, layout rects) and delegates to
/// [`TextViewerState`] for the text/archive view and [`ImageViewerState`] for the image view.
#[derive(Default)]
pub struct FileViewerState {
    pub path: PathBuf,
    pub is_visible: bool,
    pub focused: bool,
    pub is_loading: bool,
    pub area: ratatui::layout::Rect,
    /// The area actually used to render content (the block's inner area, excluding borders).
    /// Zoom/pan math uses this so the displayed scale matches the requested scale.
    pub render_area: ratatui::layout::Rect,
    pub text: TextViewerState,
    pub image: ImageViewerState,
    cancel_flag: Option<Arc<AtomicBool>>,
}

#[derive(Default)]
pub struct FileViewerSearchState {
    pub is_visible: bool,
    pub query: String,
    pub cursor_position: usize,
    pub error: Option<String>,
}

impl FileViewerSearchState {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.query = String::new();
        self.cursor_position = 0;
        self.error = None;
    }
}

impl FileViewerState {
    #[must_use]
    pub fn new(is_dark_theme: bool, app_theme_name: &str) -> Self {
        let theme_name = if app_theme_name == "solarized light" {
            "solarized_winter_light"
        } else if app_theme_name == "catppuccin macchiato" {
            "catppuccin_macchiato"
        } else if app_theme_name == "catppuccin frappe" {
            "catppuccin_frappe"
        } else if app_theme_name == "catppuccin mocha" {
            "catppuccin_mocha"
        } else if app_theme_name == "catppuccin latte" {
            "catppuccin_latte"
        } else if app_theme_name == "dracula" {
            "dracula"
        } else if app_theme_name == "nord" {
            "nord"
        } else if is_dark_theme {
            "catppuccin_mocha"
        } else {
            "papercolor_light"
        };
        let theme = lumis::themes::get(theme_name).ok();

        Self {
            path: PathBuf::new(),
            is_visible: false,
            focused: false,
            is_loading: false,
            area: ratatui::layout::Rect::default(),
            render_area: ratatui::layout::Rect::default(),
            text: TextViewerState::with_theme_name(theme_name, theme),
            image: ImageViewerState::default(),
            cancel_flag: None,
        }
    }

    pub fn reset(&mut self) {
        self.image.release_decoded();
        self.text.reset();
    }

    fn cancel_background_load(&mut self) {
        if let Some(flag) = self.cancel_flag.take() {
            flag.store(true, Ordering::Relaxed);
        }
    }

    fn new_cancel_flag(&mut self) -> Arc<AtomicBool> {
        self.cancel_background_load();
        let flag = Arc::new(AtomicBool::new(false));
        self.cancel_flag = Some(flag.clone());
        flag
    }

    /// Drop the decoded image, its scaled cache and any installed protocol.
    ///
    /// Called when the viewer is hidden so a large decoded image is not held in memory
    /// while it cannot be seen. The image is reloaded the next time the viewer is shown.
    pub fn release_image(&mut self) {
        self.image.release_decoded();
    }

    pub fn init_picker_detached(&mut self) {
        self.image.init_picker_detached();
    }

    pub async fn init_picker(&mut self) {
        self.image.init_picker().await;
    }

    #[must_use]
    pub fn is_image(path: &std::path::Path) -> bool {
        if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
            let ext = ext.to_lowercase();
            return matches!(
                ext.as_str(),
                "avif"
                    | "bmp"
                    | "dds"
                    | "exr"
                    | "ff"
                    | "gif"
                    | "hdr"
                    | "ico"
                    | "jpg"
                    | "jpeg"
                    | "png"
                    | "pbm"
                    | "pgm"
                    | "ppm"
                    | "pnm"
                    | "qoi"
                    | "tga"
                    | "tif"
                    | "tiff"
                    | "webp"
            );
        }
        false
    }

    pub fn load_image(
        &mut self,
        path: &std::path::Path,
        provider: &std::sync::Arc<dyn crate::fs::fs_provider::FileSystemProvider>,
    ) {
        self.is_loading = true;
        let flag = self.new_cancel_flag();
        self.image.load_image(path, provider, flag);
    }

    pub async fn load_content(
        &mut self,
        path: &std::path::Path,
        provider: &std::sync::Arc<dyn crate::fs::fs_provider::FileSystemProvider>,
        size: Option<u64>,
        limit_bytes: u64,
    ) {
        self.cancel_background_load();
        self.reset();
        self.path.clone_from(&path.to_path_buf());
        if provider.is_dir(path).await {
            self.text.content = vec!["Directory".to_string()];
            return;
        }

        // RPM metadata view (F3)
        if let Some(ext) = path.extension().and_then(|e| e.to_str())
            && ext.to_lowercase() == "rpm"
            && provider.is_local()
        {
            let handler = crate::fs::archive::rpm::RpmHandler::new(path);
            match handler.get_metadata() {
                Ok(meta) => {
                    self.text.content = meta.lines().map(String::from).collect();
                    return;
                }
                Err(e) => {
                    self.text.content = vec![
                        "RPM package detected.".to_string(),
                        format!("Failed to parse metadata: {e}"),
                        "Falling back to binary view...".to_string(),
                    ];
                    // Don't return, let it fall back
                }
            }
        }

        if Self::is_image(path) {
            self.load_image(path, provider);
            return;
        }

        // Archive tree preview — scan in background to avoid blocking the UI
        if provider.is_local() && crate::fs::archive::looks_like_archive(path) {
            self.is_loading = true;
            let flag = self.new_cancel_flag();
            self.text.spawn_archive_scan(path, flag);
            return;
        }

        self.text.language =
            lumis::languages::Language::from_str(&self.path.to_string_lossy()).unwrap_or_default();

        let limit = usize::try_from(limit_bytes).unwrap_or(usize::MAX);
        let limit_u64 = limit_bytes;

        // Read small chunk to check for binary and encoding
        let chunk = match provider.read_file_at(&self.path, 0u64, 8192usize).await {
            Ok(buf) => buf,
            Err(e) => {
                self.text.content = vec![format!("Error reading file: {e}")];
                return;
            }
        };

        if crate::large_text::file_reader::is_binary(&chunk) {
            self.text.content = vec!["Binary file detected".to_string()];
            return;
        }

        if let Some(s) = size
            && s > limit_u64
        {
            if provider.is_local() {
                // Try vendored large_text for local files — index in background to avoid
                // blocking the UI event loop
                let encoding = crate::large_text::file_reader::detect_encoding(&chunk);
                self.is_loading = true;
                let flag = self.new_cancel_flag();
                self.text
                    .spawn_large_file_index(path, encoding, s, limit_u64, flag);
                return;
            }

            self.text.content = vec![format!(
                "File too large to display (size: {}, limit: {})",
                crate::fs::utils::format_size(Some(s), false, false),
                crate::fs::utils::format_size(Some(limit_u64), false, false)
            )];
            return;
        }

        self.text.warm_highlight();

        // Now safe to read content up to limit
        match provider.read_file_content(&self.path, limit).await {
            Ok(content) => {
                self.text.content = content.lines().map(String::from).collect();
            }
            Err(e) => {
                self.text.content = vec![format!("Error reading file: {e}")];
            }
        }
    }

    pub fn handle_load_result(&mut self, res: ImageLoadResult) {
        if let Some(p) = res.picker {
            self.image.picker = Some(p);
        }
        if res.load_id == 0 {
            return;
        }
        if res.load_id != self.image.current_load_id || res.path != self.path {
            return;
        }
        self.is_loading = false;
        self.image.image_zoom.reset();
        self.image.image_zoom.image = res.image;
        match res.result {
            Ok(proto) => self.image.install_protocol(proto),
            Err(e) => {
                self.text.content = vec![format!("Error loading image: {e}")];
            }
        }
    }

    pub fn handle_content_load_result(&mut self, res: ContentLoadResult) {
        if res.load_id != self.text.content_load_id || res.path != self.path {
            return;
        }
        self.is_loading = false;
        self.text.content = res.content;
        self.text.archive_rows = res.archive_rows;
        self.text.language = res.language;
        if let Some((reader, indexer)) = res.large_file {
            self.text.large_file_reader = Some(reader);
            self.text.large_file_indexer = Some(indexer);
        }
    }

    /// The active search query, if any.
    #[must_use]
    pub fn search_query(&self) -> &str {
        self.text.search_query()
    }

    #[must_use]
    pub fn total_lines(&self) -> usize {
        self.text.total_lines()
    }

    /// Number of lines that fit in the viewer's visible area (borders excluded).
    #[must_use]
    pub fn visible_lines(&self) -> usize {
        self.area.height.saturating_sub(2) as usize
    }

    /// Maximum scroll offset so the last line sits at the bottom of the viewport.
    #[must_use]
    pub fn max_scroll_offset(&self) -> usize {
        self.text.total_lines().saturating_sub(self.visible_lines())
    }

    #[must_use]
    pub fn get_line(&self, idx: usize) -> Option<String> {
        self.text.get_line(idx)
    }

    #[must_use]
    pub fn get_selected_text(&self) -> Option<String> {
        self.text.get_selected_text()
    }

    /// Converts a display column to a character index for a given row.
    /// Handles tab expansion (4 spaces) and wide characters.
    #[must_use]
    pub fn display_col_to_char_idx(&self, row: usize, display_col: usize) -> usize {
        self.text.display_col_to_char_idx(row, display_col)
    }

    /// Selects the word or syntax chunk at the given display coordinates.
    pub fn select_word_at(&mut self, row: usize, display_col: usize) {
        self.text.select_word_at(row, display_col);
    }

    pub fn search(&mut self, query: &str) -> bool {
        self.text.search(query, self.area)
    }

    pub fn search_next(&mut self) -> bool {
        self.text.search_next(self.area)
    }

    pub fn search_prev(&mut self) -> bool {
        self.text.search_prev(self.area)
    }

    /// Whether an image is currently displayed zoomed in beyond "fit" (panning is active).
    #[must_use]
    pub fn is_image_zoomed(&self) -> bool {
        self.image.is_zoomed(self.render_area)
    }

    /// Zoom in one step towards 100%.
    pub fn zoom_image_in(&mut self) {
        self.image.zoom_in(self.render_area);
    }

    /// Zoom out one step towards "fit".
    pub fn zoom_image_out(&mut self) {
        self.image.zoom_out(self.render_area);
    }

    /// Pan the displayed window by `(dx, dy)` viewport cells (clamped to image bounds).
    pub fn pan_image(&mut self, dx: i64, dy: i64) {
        self.image.pan(dx, dy, self.render_area);
    }

    /// Scale factor that fits the image proportionally into the viewport (capped at 1.0).
    #[must_use]
    pub fn fit_scale(&self) -> f32 {
        self.image.fit_scale(self.render_area)
    }

    /// Ensure the image protocol matches the current zoom/pan/`render_area`.
    ///
    /// When zoomed in, a crop of the scaled source image sized to the viewport is produced so
    /// the (larger than viewport) image can be panned. When not zoomed, the full image protocol
    /// is restored. Rebuilds only when the zoom, pan or area changed.
    pub fn prepare_image_protocol(&mut self) {
        self.image.prepare_protocol(self.render_area);
    }
}
