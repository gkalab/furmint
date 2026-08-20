use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use ratatui_image::ResizeEncodeRender;
use ratatui_image::protocol::StatefulProtocol;

const ZOOM_IN_FACTOR: f32 = 1.25;
const ZOOM_OUT_FACTOR: f32 = 0.8;

#[derive(Default)]
pub struct FileViewerState {
    pub path: PathBuf,
    pub content: Vec<String>,
    pub scroll_offset: usize,
    pub horizontal_scroll_offset: usize,
    pub is_visible: bool,
    pub language: lumis::languages::Language,
    pub theme: Option<lumis::themes::Theme>,
    pub focused: bool,
    pub protocol: Option<ratatui_image::thread::ThreadProtocol>,
    pub resize_rx: Option<UnboundedReceiver<ratatui_image::thread::ResizeRequest>>,
    pub resize_tx: Option<UnboundedSender<ratatui_image::thread::ResizeRequest>>,
    pub picker: Option<ratatui_image::picker::Picker>,
    pub image_load_tx: Option<UnboundedSender<ImageLoadResult>>,
    pub content_load_tx: Option<UnboundedSender<ContentLoadResult>>,
    pub content_load_id: usize,
    pub is_loading: bool,
    pub current_load_id: usize,
    pub area: ratatui::layout::Rect,
    /// The area actually used to render content (the block's inner area, excluding borders).
    /// Zoom/pan math uses this so the displayed scale matches the requested scale.
    pub render_area: ratatui::layout::Rect,
    pub selection: Option<((usize, usize), (usize, usize))>,
    pub large_file_reader: Option<crate::large_text::file_reader::FileReader>,
    pub large_file_indexer: Option<crate::large_text::line_indexer::LineIndexer>,
    pub search_query: String,
    pub search_regex: Option<regex::Regex>,
    pub current_search_match: Option<(usize, usize, usize)>, // (line_idx, start_char, end_char)
    pub image_zoom: ImageZoomState,
    pub archive_rows: Option<Vec<crate::fs::archive::preview::ArchiveTreeRow>>,
    cancel_flag: Option<Arc<AtomicBool>>,
}

/// State for viewing an image at higher than "fit" magnification.
///
/// `zoom` uses `0.0` as a sentinel meaning "fit the image to the viewport" (the default).
/// Any value `> 0.0` is an absolute scale factor in the range `[fit_scale, 1.0]`, where
/// `1.0` corresponds to 100% (natural) zoom. Panning is only meaningful while zoomed in past
/// "fit".
#[derive(Default)]
pub struct ImageZoomState {
    /// The decoded source image, retained so it can be re-cropped for zoom/pan.
    pub image: Option<image::DynamicImage>,
    /// Sentinal `0.0` = fit; otherwise the target scale factor in `[fit, 1.0]`.
    pub zoom: f32,
    /// Horizontal pan offset (in viewport cells).
    pub pan_x: u16,
    /// Vertical pan offset (in viewport cells).
    pub pan_y: u16,
    /// The area the currently installed protocol was prepared for, to detect changes.
    applied_area: ratatui::layout::Rect,
    /// The zoom the currently installed protocol was prepared for.
    applied_zoom: f32,
    /// The pan the currently installed protocol was prepared for.
    applied_pan: (u16, u16),
    /// Cached copy of the source image scaled to `scaled_zoom`, so pan-only updates
    /// can crop from it instead of re-resizing the source on every drag event.
    scaled: Option<image::DynamicImage>,
    /// The zoom factor `scaled` was produced at.
    scaled_zoom: f32,
}

impl ImageZoomState {
    #[must_use]
    pub fn effective_zoom(&self, fit: f32) -> f32 {
        if self.zoom > 0.0 { self.zoom } else { fit }
    }

    /// Whether the image is zoomed in beyond "fit" (i.e. panning is active).
    #[must_use]
    pub fn is_zoomed(&self, fit: f32) -> bool {
        self.image.is_some() && self.effective_zoom(fit) > fit + 0.001
    }

    pub fn zoom_in(&mut self, fit: f32) {
        if self.image.is_none() {
            return;
        }
        let eff = self.effective_zoom(fit);
        if eff >= 0.999 {
            return;
        }
        let new = (eff * ZOOM_IN_FACTOR).min(1.0);
        if self.zoom == 0.0 {
            self.pan_x = 0;
            self.pan_y = 0;
        }
        self.zoom = new;
    }

    pub fn zoom_out(&mut self, fit: f32) {
        if self.image.is_none() {
            return;
        }
        let eff = self.effective_zoom(fit);
        let new = (eff * ZOOM_OUT_FACTOR).max(fit).min(1.0);
        if new <= fit {
            self.reset_zoom();
        } else {
            self.zoom = new;
        }
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub fn pan(&mut self, dx: i64, dy: i64, max: (u16, u16)) {
        let nx = i64::from(self.pan_x) + dx;
        let ny = i64::from(self.pan_y) + dy;
        self.pan_x = nx.clamp(0, i64::from(max.0)) as u16;
        self.pan_y = ny.clamp(0, i64::from(max.1)) as u16;
    }

    fn reset_zoom(&mut self) {
        self.zoom = 0.0;
        self.pan_x = 0;
        self.pan_y = 0;
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
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

pub struct ImageLoadResult {
    pub load_id: usize,
    pub path: PathBuf,
    pub result: Result<ratatui_image::protocol::StatefulProtocol, String>,
    pub picker: Option<ratatui_image::picker::Picker>,
    pub image: Option<image::DynamicImage>,
}

pub struct ContentLoadResult {
    pub load_id: usize,
    pub path: PathBuf,
    pub content: Vec<String>,
    pub archive_rows: Option<Vec<crate::fs::archive::preview::ArchiveTreeRow>>,
    pub language: lumis::languages::Language,
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
            content: Vec::new(),
            scroll_offset: 0,
            horizontal_scroll_offset: 0,
            is_visible: false,
            language: lumis::languages::Language::default(),
            theme,
            focused: false,
            protocol: None,
            resize_rx: None,
            resize_tx: None,
            picker: None,
            image_load_tx: None,
            content_load_tx: None,
            content_load_id: 0,
            is_loading: false,
            current_load_id: 0,
            area: ratatui::layout::Rect::default(),
            render_area: ratatui::layout::Rect::default(),
            selection: None,
            large_file_reader: None,
            large_file_indexer: None,
            search_query: String::new(),
            search_regex: None,
            current_search_match: None,
            image_zoom: ImageZoomState::default(),
            archive_rows: None,
            cancel_flag: None,
        }
    }

    pub fn reset(&mut self) {
        if let Some(tp) = &mut self.protocol {
            tp.empty_protocol();
        }
        self.content = Vec::new();
        self.scroll_offset = 0;
        self.horizontal_scroll_offset = 0;
        self.language = lumis::languages::Language::default();
        self.selection = None;
        self.large_file_reader = None;
        self.large_file_indexer = None;
        self.search_query = String::new();
        self.search_regex = None;
        self.current_search_match = None;
        self.image_zoom.reset();
        self.archive_rows = None;
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
        if let Some(tp) = &mut self.protocol {
            tp.empty_protocol();
        }
        self.image_zoom.reset();
    }

    pub fn init_picker_detached(&mut self) {
        if self.picker.is_some() {
            return;
        }
        if self.resize_tx.is_none() {
            self.init_channels();
        }
        let tx = self.image_load_tx.clone();
        tokio::spawn(async move {
            let picker = Self::create_picker().await;
            if let Some(tx) = tx {
                let _ = tx.send(ImageLoadResult {
                    load_id: 0,
                    path: PathBuf::new(),
                    result: Err("PICKER_INIT".to_string()),
                    picker: Some(picker),
                    image: None,
                });
            }
        });
    }

    pub async fn init_picker(&mut self) {
        if self.picker.is_some() {
            return;
        }
        if self.resize_tx.is_none() {
            self.init_channels();
        }
        self.picker = Some(Self::create_picker().await);
    }

    fn init_channels(&mut self) {
        let (tx, rx) = unbounded_channel();
        self.resize_tx = Some(tx);
        self.resize_rx = Some(rx);
    }

    async fn create_picker() -> ratatui_image::picker::Picker {
        tokio::task::spawn_blocking(|| {
            ratatui_image::picker::Picker::from_query_stdio()
                .unwrap_or_else(|_| ratatui_image::picker::Picker::halfblocks())
        })
        .await
        .unwrap_or_else(|_| ratatui_image::picker::Picker::halfblocks())
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
        // Ensure channels and picker initialization are kicked off
        if self.resize_tx.is_none() {
            self.init_picker_detached();
        }

        if let (Some(image_tx), Some(_resize_tx)) = (&self.image_load_tx, &self.resize_tx) {
            self.is_loading = true;
            self.current_load_id += 1;
            let load_id = self.current_load_id;
            let image_tx_clone = image_tx.clone();
            let cancel_flag = self.new_cancel_flag();
            let path_clone = path.to_path_buf();
            let provider_clone = std::sync::Arc::clone(provider);
            let picker = self.picker.clone();

            tokio::spawn(async move {
                let path_for_result = path_clone.clone();
                let (result, image): (
                    Result<StatefulProtocol, String>,
                    Option<image::DynamicImage>,
                ) = tokio::task::spawn_blocking(move || {
                    let p = picker.unwrap_or_else(|| {
                        ratatui_image::picker::Picker::from_query_stdio()
                            .unwrap_or_else(|_| ratatui_image::picker::Picker::halfblocks())
                    });

                    let decode_result = provider_clone
                        .read_file(&path_clone)
                        .map_err(|e| e.to_string())
                        .and_then(|data| {
                            use image::ImageReader;
                            use std::io::Cursor;
                            ImageReader::new(Cursor::new(data))
                                .with_guessed_format()
                                .map_err(|e| e.to_string())
                                .and_then(|r| r.decode().map_err(|e| e.to_string()))
                        });

                    match decode_result {
                        Ok(dyn_image) => (
                            Ok(p.new_resize_protocol(dyn_image.clone())),
                            Some(dyn_image),
                        ),
                        Err(e) => (Err(e), None),
                    }
                })
                .await
                .unwrap_or_else(|e| (Err(e.to_string()), None));

                if cancel_flag.load(Ordering::Relaxed) {
                    return;
                }
                let _ = image_tx_clone.send(ImageLoadResult {
                    load_id,
                    path: path_for_result,
                    result,
                    image,
                    picker: None,
                });
            });
        }
    }

    pub fn load_content(
        &mut self,
        path: &std::path::Path,
        provider: &std::sync::Arc<dyn crate::fs::fs_provider::FileSystemProvider>,
        size: Option<u64>,
        limit_bytes: u64,
    ) {
        self.cancel_background_load();
        self.reset();
        self.path.clone_from(&path.to_path_buf());
        if provider.is_dir(path) {
            self.content = vec!["Directory".to_string()];
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
                    self.content = meta.lines().map(String::from).collect();
                    return;
                }
                Err(e) => {
                    self.content = vec![
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
        if provider.is_local()
            && let Ok(handler) = crate::fs::archive::get_archive_handler(path)
        {
            self.spawn_archive_scan(handler);
            return;
        }

        self.language =
            lumis::languages::Language::from_str(&self.path.to_string_lossy()).unwrap_or_default();

        let limit = usize::try_from(limit_bytes).unwrap_or(usize::MAX);
        let limit_u64 = limit_bytes;

        // Read small chunk to check for binary and encoding
        let chunk = match provider.read_file_at(&self.path, 0u64, 8192usize) {
            Ok(buf) => buf,
            Err(e) => {
                self.content = vec![format!("Error reading file: {e}")];
                return;
            }
        };

        if crate::large_text::file_reader::is_binary(&chunk) {
            self.content = vec!["Binary file detected".to_string()];
            return;
        }

        if let Some(s) = size
            && s > limit_u64
        {
            if provider.is_local() {
                // Try vendored large_text for local files
                let encoding = crate::large_text::file_reader::detect_encoding(&chunk);
                if let Ok(reader) =
                    crate::large_text::file_reader::FileReader::new(path.to_path_buf(), encoding)
                {
                    let mut indexer = crate::large_text::line_indexer::LineIndexer::new();
                    indexer.index_file(&reader);
                    self.large_file_reader = Some(reader);
                    self.large_file_indexer = Some(indexer);
                    self.language = lumis::languages::Language::default(); // Disable syntax highlighting
                    return;
                }
            }

            self.content = vec![format!(
                "File too large to display (size: {}, limit: {})",
                crate::fs::utils::format_size(Some(s), false, false),
                crate::fs::utils::format_size(Some(limit_u64), false, false)
            )];
            return;
        }

        // Now safe to read content up to limit
        match provider.read_file_content(&self.path, limit) {
            Ok(content) => {
                self.content = content.lines().map(String::from).collect();
            }
            Err(e) => {
                self.content = vec![format!("Error reading file: {e}")];
            }
        }
    }

    pub fn handle_load_result(&mut self, res: ImageLoadResult) {
        if let Some(p) = res.picker {
            self.picker = Some(p);
        }
        if res.load_id == 0 {
            return;
        }
        if res.load_id != self.current_load_id || res.path != self.path {
            return;
        }
        self.is_loading = false;
        self.image_zoom.reset();
        self.image_zoom.image = res.image;
        match res.result {
            Ok(proto) => {
                if let Some(tp) = &mut self.protocol {
                    tp.replace_protocol(proto);
                } else if let Some(tx) = &self.resize_tx {
                    self.protocol = Some(ratatui_image::thread::ThreadProtocol::new(
                        tx.clone(),
                        Some(proto),
                    ));
                }
            }
            Err(e) => {
                self.content = vec![format!("Error loading image: {e}")];
            }
        }
    }

    pub fn handle_content_load_result(&mut self, res: ContentLoadResult) {
        if res.load_id != self.content_load_id || res.path != self.path {
            return;
        }
        self.is_loading = false;
        self.content = res.content;
        self.archive_rows = res.archive_rows;
        self.language = res.language;
    }

    fn spawn_archive_scan(&mut self, handler: Box<dyn crate::fs::archive::ArchiveFormat>) {
        self.is_loading = true;
        self.content_load_id += 1;
        let load_id = self.content_load_id;
        let cancel_flag = self.new_cancel_flag();
        let path = self.path.clone();
        let Some(tx) = self.content_load_tx.clone() else {
            return;
        };
        tokio::spawn(async move {
            let result = tokio::task::spawn_blocking(move || handler.scan())
                .await
                .unwrap_or_else(|e| Err(anyhow::anyhow!(e.to_string())));
            if cancel_flag.load(Ordering::Relaxed) {
                return;
            }
            let load_result = match result {
                Ok(scan_result) => {
                    let rows = crate::fs::archive::preview::build_archive_tree(&scan_result);
                    if rows.is_empty() {
                        ContentLoadResult {
                            load_id,
                            path,
                            content: Vec::new(),
                            archive_rows: None,
                            language: lumis::languages::Language::default(),
                        }
                    } else {
                        let content = rows
                            .iter()
                            .map(|r| format!("{}{}", r.prefix, r.name))
                            .collect();
                        ContentLoadResult {
                            load_id,
                            path,
                            content,
                            archive_rows: Some(rows),
                            language: lumis::languages::Language::default(),
                        }
                    }
                }
                Err(e) => ContentLoadResult {
                    load_id,
                    path,
                    content: vec![format!("Error scanning archive: {e}")],
                    archive_rows: None,
                    language: lumis::languages::Language::default(),
                },
            };
            let _ = tx.send(load_result);
        });
    }

    #[must_use]
    fn font_size(&self) -> Option<ratatui_image::FontSize> {
        self.picker
            .as_ref()
            .map(ratatui_image::picker::Picker::font_size)
    }

    /// Scale factor that fits the image proportionally into the viewport (capped at 1.0).
    #[must_use]
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    pub fn fit_scale(&self) -> f32 {
        let (Some(img), Some(font)) = (&self.image_zoom.image, self.font_size()) else {
            return 1.0;
        };
        if self.render_area.width == 0 || self.render_area.height == 0 {
            return 1.0;
        }
        let nw = img.width() as f32 / f32::from(font.width);
        let nh = img.height() as f32 / f32::from(font.height);
        (f32::from(self.render_area.width) / nw)
            .min(f32::from(self.render_area.height) / nh)
            .min(1.0)
    }

    /// Whether an image is currently displayed zoomed in beyond "fit" (panning is active).
    #[must_use]
    pub fn is_image_zoomed(&self) -> bool {
        self.image_zoom.is_zoomed(self.fit_scale())
    }

    /// Zoom in one step towards 100%.
    pub fn zoom_image_in(&mut self) {
        self.image_zoom.zoom_in(self.fit_scale());
    }

    /// Zoom out one step towards "fit".
    pub fn zoom_image_out(&mut self) {
        self.image_zoom.zoom_out(self.fit_scale());
    }

    /// Pan the displayed window by `(dx, dy)` viewport cells (clamped to image bounds).
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::similar_names
    )]
    pub fn pan_image(&mut self, dx: i64, dy: i64) {
        if !self.is_image_zoomed() {
            return;
        }
        let Some(font) = self.font_size() else {
            return;
        };
        let fit = self.fit_scale();
        let scale = self.image_zoom.effective_zoom(fit);
        let (rw, rh) = self.render_pixel_size(scale);
        let rw_cells = (rw as f32 / f32::from(font.width)).ceil() as u16;
        let rh_cells = (rh as f32 / f32::from(font.height)).ceil() as u16;
        let max_x = rw_cells.saturating_sub(self.render_area.width);
        let max_y = rh_cells.saturating_sub(self.render_area.height);
        self.image_zoom.pan(dx, dy, (max_x, max_y));
    }

    /// Pixel dimensions of the whole image at the given scale.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    fn render_pixel_size(&self, scale: f32) -> (u32, u32) {
        let Some(img) = &self.image_zoom.image else {
            return (0, 0);
        };
        (
            (img.width() as f32 * scale).round() as u32,
            (img.height() as f32 * scale).round() as u32,
        )
    }

    /// Ensure `self.protocol` holds a protocol matching the current zoom/pan/`render_area`.
    ///
    /// When zoomed in, a crop of the scaled source image sized to the viewport is produced so
    /// the (larger than viewport) image can be panned. When not zoomed, the full image protocol
    /// is restored. Rebuilds only when the zoom, pan or area changed.
    #[allow(clippy::float_cmp)]
    pub fn prepare_image_protocol(&mut self) {
        let (Some(font), Some(picker)) = (self.font_size(), self.picker.clone()) else {
            return;
        };
        if self.image_zoom.image.is_none() {
            return;
        }

        let area = self.render_area;
        let fit = self.fit_scale();
        let scale = self.image_zoom.effective_zoom(fit);
        let pan = (self.image_zoom.pan_x, self.image_zoom.pan_y);

        if self.image_zoom.applied_area == area
            && self.image_zoom.applied_zoom == scale
            && self.image_zoom.applied_pan == pan
            && self.protocol.is_some()
        {
            return;
        }

        let proto = if self.image_zoom.is_zoomed(fit) {
            self.build_zoomed_protocol(&picker, font, scale)
        } else {
            self.image_zoom.scaled = None;
            self.image_zoom.scaled_zoom = 0.0;
            self.image_zoom
                .image
                .clone()
                .map(|img| picker.new_resize_protocol(img))
        };

        let Some(proto) = proto else { return };

        if let Some(tp) = &mut self.protocol {
            tp.replace_protocol(proto);
        } else if let Some(tx) = &self.resize_tx {
            self.protocol = Some(ratatui_image::thread::ThreadProtocol::new(
                tx.clone(),
                Some(proto),
            ));
        }
        self.image_zoom.applied_area = area;
        self.image_zoom.applied_zoom = scale;
        self.image_zoom.applied_pan = pan;
    }

    /// Build a `StatefulProtocol` showing the panned window of the zoomed image.
    ///
    /// The source image is scaled once per zoom level and cached in `self.image_zoom.scaled`,
    /// so changing only the pan re-crops the cached buffer instead of re-resizing the source.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::similar_names
    )]
    fn build_zoomed_protocol(
        &mut self,
        picker: &ratatui_image::picker::Picker,
        font: ratatui_image::FontSize,
        scale: f32,
    ) -> Option<StatefulProtocol> {
        let img = self.image_zoom.image.clone()?;
        let (rw, rh) = self.render_pixel_size(scale);
        let vw = (f32::from(self.render_area.width) * f32::from(font.width)).ceil() as u32;
        let vh = (f32::from(self.render_area.height) * f32::from(font.height)).ceil() as u32;
        let max_x = rw.saturating_sub(vw);
        let max_y = rh.saturating_sub(vh);
        let ox =
            ((f32::from(self.image_zoom.pan_x) * f32::from(font.width)).round() as u32).min(max_x);
        let oy =
            ((f32::from(self.image_zoom.pan_y) * f32::from(font.height)).round() as u32).min(max_y);

        let scaled = self.scale_for_zoom(&img, rw, rh, scale);
        let crop = scaled.crop_imm(ox, oy, vw, vh);

        let mut proto = picker.new_resize_protocol(crop);
        // The crop is already exactly the viewport's pixel size, so `Scale` resizing is a
        // no-op; only the encode runs. Encode synchronously so the same frame that pans also
        // renders the image, instead of sending the protocol through the async `ThreadProtocol`
        // which would leave a cleared (blank) frame until the worker returns.
        proto.resize_encode(
            &ratatui_image::Resize::Scale(Some(ratatui_image::FilterType::CatmullRom)),
            self.render_area.into(),
        );
        Some(proto)
    }

    /// Returns the source image resized to `(rw, rh)`, caching it for the given `scale`.
    #[allow(clippy::float_cmp)]
    fn scale_for_zoom(
        &mut self,
        img: &image::DynamicImage,
        rw: u32,
        rh: u32,
        scale: f32,
    ) -> image::DynamicImage {
        if self.image_zoom.scaled_zoom == scale
            && let Some(scaled) = &self.image_zoom.scaled
        {
            return scaled.clone();
        }
        let scaled = img.resize(
            rw.max(1),
            rh.max(1),
            image::imageops::FilterType::CatmullRom,
        );
        self.image_zoom.scaled = Some(scaled.clone());
        self.image_zoom.scaled_zoom = scale;
        scaled
    }

    #[must_use]
    pub fn get_selected_text(&self) -> Option<String> {
        let ((r1, c1), (r2, c2)) = self.selection?;
        let (start_r, start_c, end_r, end_c) = if r1 < r2 || (r1 == r2 && c1 <= c2) {
            (r1, c1, r2, c2)
        } else {
            (r2, c2, r1, c1)
        };

        let mut selected_lines = Vec::new();
        for r in start_r..=end_r {
            let line_opt = if let Some(indexer) = &self.large_file_indexer {
                self.large_file_reader.as_ref().and_then(|reader| {
                    indexer
                        .get_line_with_reader(r, reader)
                        .map(|(s, e)| reader.get_chunk(s, e))
                })
            } else {
                self.content.get(r).cloned()
            };

            if let Some(line) = line_opt {
                if start_r == end_r {
                    // Selection is within a single line
                    let s = line
                        .chars()
                        .skip(start_c)
                        .take(end_c.saturating_sub(start_c))
                        .collect::<String>();
                    selected_lines.push(s);
                } else if r == start_r {
                    // First line of multi-line selection
                    let s = line.chars().skip(start_c).collect::<String>();
                    selected_lines.push(s);
                } else if r == end_r {
                    // Last line of multi-line selection
                    let s = line.chars().take(end_c).collect::<String>();
                    selected_lines.push(s);
                } else {
                    // Intermediate lines
                    selected_lines.push(line.clone());
                }
            }
        }

        if selected_lines.is_empty() {
            None
        } else {
            Some(selected_lines.join("\n"))
        }
    }

    /// Converts a display column to a character index for a given row.
    /// Handles tab expansion (4 spaces) and wide characters.
    #[must_use]
    pub fn display_col_to_char_idx(&self, row: usize, display_col: usize) -> usize {
        let line_opt = if let Some(indexer) = &self.large_file_indexer {
            self.large_file_reader.as_ref().and_then(|reader| {
                indexer
                    .get_line_with_reader(row, reader)
                    .map(|(s, e)| reader.get_chunk(s, e))
            })
        } else {
            self.content.get(row).cloned()
        };

        let Some(line) = line_opt else {
            return display_col;
        };

        let mut current_display_pos = 0;
        for (idx, ch) in line.chars().enumerate() {
            let ch_width = if ch == '\t' {
                4 - (current_display_pos % 4)
            } else {
                unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1)
            };

            if current_display_pos + ch_width > display_col {
                return idx;
            }
            current_display_pos += ch_width;
        }
        line.chars().count()
    }

    /// Selects the word or syntax chunk at the given display coordinates.
    pub fn select_word_at(&mut self, row: usize, display_col: usize) {
        let line_opt = if let Some(indexer) = &self.large_file_indexer {
            self.large_file_reader.as_ref().and_then(|reader| {
                indexer
                    .get_line_with_reader(row, reader)
                    .map(|(s, e)| reader.get_chunk(s, e))
            })
        } else {
            self.content.get(row).cloned()
        };

        let Some(line) = line_opt else {
            self.selection = None;
            return;
        };

        let char_idx = self.display_col_to_char_idx(row, display_col);
        let char_count = line.chars().count();
        if char_idx >= char_count {
            self.selection = None;
            return;
        }

        let mut start = 0;
        let mut end = 0;
        let mut found = false;

        // Try syntax-aware selection first
        if self.large_file_indexer.is_none() {
            let highlighter = lumis::highlight::Highlighter::new(self.language, self.theme.clone());
            let segments = highlighter.highlight(&line).unwrap_or_default();

            if segments.len() > 1 {
                let mut current_char_idx = 0;
                for (_, text) in segments {
                    let segment_char_count = text.chars().count();
                    if char_idx >= current_char_idx
                        && char_idx < current_char_idx + segment_char_count
                    {
                        // Found the syntax chunk
                        start = current_char_idx;
                        end = current_char_idx + segment_char_count;
                        found = true;
                        break;
                    }
                    current_char_idx += segment_char_count;
                }
            }
        }

        let chars: Vec<char> = line.chars().collect();
        if !found {
            // Fallback: word boundaries (alphanumeric + underscore)
            let is_word_char = |c: char| c.is_alphanumeric() || c == '_';

            let target_is_word = is_word_char(chars[char_idx]);

            start = char_idx;
            while start > 0 && is_word_char(chars[start - 1]) == target_is_word {
                start -= 1;
            }

            end = char_idx;
            while end < char_count && is_word_char(chars[end]) == target_is_word {
                end += 1;
            }
        }

        // Refine selection to exclude surrounding quotes if present (supports triple and nested quotes)
        while end - start >= 2 {
            let s_char = chars[start];
            let e_char = chars[end - 1];
            if (s_char == '"' && e_char == '"')
                || (s_char == '\'' && e_char == '\'')
                || (s_char == '`' && e_char == '`')
            {
                start += 1;
                end -= 1;
            } else {
                break;
            }
        }

        self.selection = Some(((row, start), (row, end)));
    }

    #[must_use]
    pub fn total_lines(&self) -> usize {
        if let Some(indexer) = &self.large_file_indexer {
            indexer.total_lines()
        } else {
            self.content.len()
        }
    }

    /// Number of lines that fit in the viewer's visible area (borders excluded).
    #[must_use]
    pub fn visible_lines(&self) -> usize {
        self.area.height.saturating_sub(2) as usize
    }

    /// Maximum scroll offset so the last line sits at the bottom of the viewport.
    #[must_use]
    pub fn max_scroll_offset(&self) -> usize {
        self.total_lines().saturating_sub(self.visible_lines())
    }

    #[must_use]
    pub fn get_line(&self, idx: usize) -> Option<String> {
        if let Some(indexer) = &self.large_file_indexer
            && let Some(reader) = &self.large_file_reader
        {
            return indexer
                .get_line_with_reader(idx, reader)
                .map(|(s, e)| reader.get_chunk(s, e));
        }
        self.content.get(idx).cloned()
    }

    pub fn search(&mut self, query: &str) -> bool {
        if query.is_empty() {
            self.search_query = String::new();
            self.search_regex = None;
            self.current_search_match = None;
            return false;
        }

        let Ok(re) = regex::RegexBuilder::new(query)
            .case_insensitive(true)
            .build()
        else {
            return false;
        };

        self.search_query = query.to_string();
        self.search_regex = Some(re.clone());

        // Find first match at or after current scroll position
        if let Some(m) = self.find_next_match(self.scroll_offset, 0, &re) {
            self.current_search_match = Some(m);
            self.jump_to_match_with_context(m.0);
            true
        } else {
            self.current_search_match = None;
            false
        }
    }

    fn jump_to_match_with_context(&mut self, line_idx: usize) {
        let viewport_height = self.area.height as usize;
        // If the match is already visible on the current page, don't scroll.
        if line_idx >= self.scroll_offset
            && line_idx < self.scroll_offset + viewport_height.saturating_sub(1)
        {
            return;
        }
        // Leave 4 lines above for context, clamped so we never scroll past the content.
        self.scroll_offset = line_idx.saturating_sub(4).min(self.max_scroll_offset());
    }

    pub fn search_next(&mut self) -> bool {
        let Some(re) = self.search_regex.clone() else {
            return false;
        };

        let viewport_height = self.area.height as usize;
        let (start_line, start_char) = match self.current_search_match {
            Some((line, _, end_char)) => {
                // If current match is visible, search after it.
                // Otherwise, search from the current scroll offset.
                if line >= self.scroll_offset && line < self.scroll_offset + viewport_height {
                    (line, end_char)
                } else {
                    (self.scroll_offset, 0)
                }
            }
            None => (self.scroll_offset, 0),
        };

        if let Some(m) = self.find_next_match(start_line, start_char, &re) {
            if Some(m) == self.current_search_match {
                return false;
            }
            self.current_search_match = Some(m);
            self.jump_to_match_with_context(m.0);
            true
        } else {
            false
        }
    }

    pub fn search_prev(&mut self) -> bool {
        let Some(re) = self.search_regex.clone() else {
            return false;
        };

        let viewport_height = self.area.height as usize;
        let (start_line, start_char) = match self.current_search_match {
            Some((line, start_char, _)) => {
                // If current match is visible, search before it.
                if line >= self.scroll_offset && line < self.scroll_offset + viewport_height {
                    (line, start_char)
                } else {
                    (self.scroll_offset, 0)
                }
            }
            None => (self.scroll_offset, 0),
        };

        if let Some(m) = self.find_prev_match(start_line, start_char, &re) {
            if Some(m) == self.current_search_match {
                return false;
            }
            self.current_search_match = Some(m);
            self.jump_to_match_with_context(m.0);
            true
        } else {
            false
        }
    }

    fn find_next_match(
        &self,
        start_line: usize,
        start_char: usize,
        re: &regex::Regex,
    ) -> Option<(usize, usize, usize)> {
        let total = self.total_lines();
        if total == 0 {
            return None;
        }

        // 1. Current line after start_char
        if let Some(line) = self.get_line(start_line) {
            let byte_idx = line.chars().take(start_char).map(char::len_utf8).sum();
            if byte_idx < line.len()
                && let Some(m) = re.find(&line[byte_idx..])
            {
                let m_start = byte_idx + m.start();
                let m_end = byte_idx + m.end();
                let start_c = line[..m_start].chars().count();
                let end_c = start_c + line[m_start..m_end].chars().count();
                return Some((start_line, start_c, end_c));
            }
        }

        // 2. Subsequent lines
        for i in (start_line + 1)..total {
            if let Some(line) = self.get_line(i)
                && let Some(m) = re.find(&line)
            {
                let start_c = line[..m.start()].chars().count();
                let end_c = start_c + line[m.start()..m.end()].chars().count();
                return Some((i, start_c, end_c));
            }
        }

        // 3. Wrap around: 0 to start_line
        for i in 0..=start_line {
            if let Some(line) = self.get_line(i)
                && let Some(m) = re.find(&line)
            {
                // Check if this match is before our starting point if it's the same line
                let m_start_byte = m.start();
                let start_c = line[..m_start_byte].chars().count();

                if i < start_line || start_c < start_char {
                    let end_c = start_c + line[m_start_byte..m.end()].chars().count();
                    return Some((i, start_c, end_c));
                }
            }
        }

        None
    }

    fn find_prev_match(
        &self,
        start_line: usize,
        start_char: usize,
        re: &regex::Regex,
    ) -> Option<(usize, usize, usize)> {
        let total = self.total_lines();
        if total == 0 {
            return None;
        }

        // 1. Current line before start_char
        if let Some(line) = self.get_line(start_line) {
            let byte_limit = line.chars().take(start_char).map(char::len_utf8).sum();
            if byte_limit > 0
                && let Some(m) = re.find_iter(&line[..byte_limit]).last()
            {
                let start_c = line[..m.start()].chars().count();
                let end_c = start_c + line[m.start()..m.end()].chars().count();
                return Some((start_line, start_c, end_c));
            }
        }

        // 2. Previous lines
        for i in (0..start_line).rev() {
            if let Some(line) = self.get_line(i)
                && let Some(m) = re.find_iter(&line).last()
            {
                let start_c = line[..m.start()].chars().count();
                let end_c = start_c + line[m.start()..m.end()].chars().count();
                return Some((i, start_c, end_c));
            }
        }

        // 3. Wrap around: bottom to start_line
        for i in (start_line..total).rev() {
            if let Some(line) = self.get_line(i)
                && let Some(m) = re.find_iter(&line).last()
            {
                let m_start_byte = m.start();
                let start_c = line[..m_start_byte].chars().count();

                if i > start_line || start_c > start_char {
                    let end_c = start_c + line[m_start_byte..m.end()].chars().count();
                    return Some((i, start_c, end_c));
                }
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

    fn dummy_image() -> image::DynamicImage {
        image::DynamicImage::new_rgba8(100, 50)
    }

    #[test]
    fn zoom_starts_at_fit_and_is_not_zoomed() {
        let z = ImageZoomState {
            image: Some(dummy_image()),
            ..Default::default()
        };
        let fit = 0.5;
        assert!(!z.is_zoomed(fit));
        assert!((z.effective_zoom(fit) - fit).abs() < 1e-6);
    }

    #[test]
    fn zoom_in_moves_past_fit_and_caps_at_100_percent() {
        let mut z = ImageZoomState {
            image: Some(dummy_image()),
            ..Default::default()
        };
        let fit = 0.4;
        z.zoom_in(fit);
        assert!(z.is_zoomed(fit));
        assert!(z.zoom > fit && z.zoom <= 1.0);

        // Zoom in many times until capped at 100%.
        for _ in 0..50 {
            z.zoom_in(fit);
        }
        assert!((z.effective_zoom(fit) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn zoom_out_returns_to_fit() {
        let mut z = ImageZoomState {
            image: Some(dummy_image()),
            ..Default::default()
        };
        let fit = 0.5;
        z.zoom_in(fit);
        assert!(z.is_zoomed(fit));
        for _ in 0..50 {
            z.zoom_out(fit);
        }
        assert!(!z.is_zoomed(fit));
        assert!(z.zoom.abs() < 1e-6);
        assert_eq!((z.pan_x, z.pan_y), (0, 0));
    }

    #[test]
    fn pan_is_clamped_to_max() {
        let mut z = ImageZoomState {
            image: Some(dummy_image()),
            ..Default::default()
        };
        z.zoom_in(0.5);
        let max = (10, 5);

        z.pan(10000, 10000, max);
        assert_eq!((z.pan_x, z.pan_y), max);

        z.pan(-10000, -10000, max);
        assert_eq!((z.pan_x, z.pan_y), (0, 0));
    }

    #[test]
    fn zoom_in_from_already_100_percent_is_noop() {
        let mut z = ImageZoomState {
            image: Some(dummy_image()),
            ..Default::default()
        };
        let fit = 0.9;
        z.zoom = 1.0;
        z.zoom_in(fit);
        assert!((z.zoom - 1.0).abs() < 1e-6);
    }

    /// A viewer showing a 2000x2000 image in a 100x50 cell render area (halfblocks font
    /// is 10x20), so `fit_scale` is 0.5 and zooming is possible on both axes.
    fn test_viewer() -> FileViewerState {
        let mut fv = FileViewerState::new(true, "dark");
        fv.picker = Some(ratatui_image::picker::Picker::halfblocks());
        fv.image_zoom.image = Some(image::DynamicImage::new_rgba8(2000, 2000));
        fv.render_area = Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 50,
        };
        fv.init_channels();
        fv
    }

    #[test]
    fn fit_scale_uses_render_area() {
        let mut fv = test_viewer();
        // The bordered `area` is larger; zoom math must follow `render_area`.
        fv.area = Rect {
            x: 0,
            y: 0,
            width: 104,
            height: 54,
        };
        assert!((fv.fit_scale() - 0.5).abs() < 1e-4);
    }

    #[test]
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    fn pan_image_clamps_to_rendered_image_bounds() {
        let mut fv = test_viewer();
        let fit = fv.fit_scale();
        fv.zoom_image_in();
        assert!(fv.is_image_zoomed());

        let scale = fv.image_zoom.effective_zoom(fit);
        let (rw, rh) = fv.render_pixel_size(scale);
        let font = fv.font_size().unwrap();
        let max_x = (rw as f32 / f32::from(font.width)).ceil() as u16 - fv.render_area.width;
        let max_y = (rh as f32 / f32::from(font.height)).ceil() as u16 - fv.render_area.height;

        fv.pan_image(100_000, 100_000);
        assert_eq!((fv.image_zoom.pan_x, fv.image_zoom.pan_y), (max_x, max_y));

        fv.pan_image(-100_000, -100_000);
        assert_eq!((fv.image_zoom.pan_x, fv.image_zoom.pan_y), (0, 0));
    }

    #[test]
    fn prepare_image_protocol_installs_zoomed_protocol_and_caches_scaled() {
        let mut fv = test_viewer();
        fv.zoom_image_in();
        fv.prepare_image_protocol();
        assert!(fv.protocol.is_some());
        assert!(fv.image_zoom.scaled.is_some());
        let cached_zoom = fv.image_zoom.scaled_zoom;

        // Panning alone must reuse the cached scaled image (no re-resize).
        fv.pan_image(3, 3);
        fv.prepare_image_protocol();
        assert!((fv.image_zoom.scaled_zoom - cached_zoom).abs() < 1e-6);
        assert!(fv.image_zoom.scaled.is_some());

        // Zooming to a new level re-resizes.
        fv.zoom_image_in();
        fv.prepare_image_protocol();
        assert!(fv.image_zoom.scaled_zoom > cached_zoom);
    }

    #[test]
    fn prepare_image_protocol_clears_scaled_cache_when_back_at_fit() {
        let mut fv = test_viewer();
        fv.zoom_image_in();
        fv.prepare_image_protocol();
        assert!(fv.image_zoom.scaled.is_some());

        fv.zoom_image_out(); // fit*1.25*0.8 == fit -> resets to fit
        assert!(!fv.is_image_zoomed());
        fv.prepare_image_protocol();
        assert!(fv.image_zoom.scaled.is_none());
        assert!(fv.protocol.is_some());
    }

    #[test]
    fn prepare_image_protocol_handles_narrow_image_without_panic() {
        // A tall, narrow image whose zoomed width stays below the viewport width: the
        // crop must be clamped to the image rather than exceeding it.
        let mut fv = FileViewerState::new(true, "dark");
        fv.picker = Some(ratatui_image::picker::Picker::halfblocks());
        fv.image_zoom.image = Some(image::DynamicImage::new_rgba8(100, 4000));
        fv.render_area = Rect {
            x: 0,
            y: 0,
            width: 200,
            height: 100,
        };
        fv.init_channels();

        fv.zoom_image_in();
        assert!(fv.is_image_zoomed());
        fv.prepare_image_protocol();
        assert!(fv.image_zoom.scaled.is_some());
        assert!(fv.protocol.is_some());
    }

    #[test]
    fn zoomed_protocol_is_encoded_and_ready_to_render() {
        let mut fv = test_viewer();
        fv.zoom_image_in();
        fv.pan_image(3, 3);
        fv.prepare_image_protocol();
        let protocol = fv.protocol.as_mut().expect("protocol installed");
        let resize = ratatui_image::Resize::Scale(Some(ratatui_image::FilterType::CatmullRom));
        assert!(
            protocol
                .needs_resize(&resize, fv.render_area.into())
                .is_none(),
            "zoomed protocol must be already encoded so the next draw renders immediately"
        );
    }

    #[test]
    fn release_image_drops_decoded_image_and_protocol() {
        let mut fv = test_viewer();
        fv.zoom_image_in();
        fv.prepare_image_protocol();
        assert!(fv.image_zoom.image.is_some());

        fv.release_image();
        assert!(fv.image_zoom.image.is_none());
        assert!(fv.image_zoom.scaled.is_none());
        assert!(
            fv.protocol
                .as_ref()
                .is_none_or(|p| p.protocol_type().is_none())
        );
    }
}
