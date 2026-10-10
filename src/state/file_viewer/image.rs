use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use ratatui_image::ResizeEncodeRender;
use ratatui_image::protocol::StatefulProtocol;

const ZOOM_IN_FACTOR: f32 = 1.25;
const ZOOM_OUT_FACTOR: f32 = 0.8;

/// Maximum decoded size (width * height) of an image we are willing to display.
///
/// Larger images are rejected up front (after a cheap header-only dimension probe) so we never
/// allocate, on the runtime worker or at rest, a full-size decode that exceeds this many pixels.
/// Each retained full-size copy costs ~4 bytes per pixel (RGBA), so this bounds steady memory.
const MAX_IMAGE_PIXELS: u64 = 50_000_000; // ~50 MP

/// State for the image side of the file viewer.
#[derive(Default)]
pub struct ImageViewerState {
    pub protocol: Option<ratatui_image::thread::ThreadProtocol>,
    pub image_zoom: ImageZoomState,
    pub picker: Option<ratatui_image::picker::Picker>,
    resize_rx: Option<UnboundedReceiver<ratatui_image::thread::ResizeRequest>>,
    resize_tx: Option<UnboundedSender<ratatui_image::thread::ResizeRequest>>,
    image_load_tx: Option<UnboundedSender<ImageLoadResult>>,
    pub(super) current_load_id: usize,
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

pub struct ImageLoadResult {
    pub load_id: usize,
    pub path: PathBuf,
    pub result: Result<ratatui_image::protocol::StatefulProtocol, String>,
    pub picker: Option<ratatui_image::picker::Picker>,
    pub image: Option<image::DynamicImage>,
}

/// Decode an image from in-memory bytes, refusing images larger than [`MAX_IMAGE_PIXELS`].
///
/// The dimension probe only reads the image header (via a borrowed `Cursor`), so oversized
/// images are rejected before any full-size decode allocates memory.
fn decode_image_capped(data: Vec<u8>) -> Result<image::DynamicImage, String> {
    use image::ImageReader;
    use std::io::Cursor;

    let (width, height) = ImageReader::new(Cursor::new(&data[..]))
        .with_guessed_format()
        .map_err(|e| e.to_string())
        .and_then(|r| r.into_dimensions().map_err(|e| e.to_string()))?;
    check_image_size(width, height)?;

    ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .map_err(|e| e.to_string())
        .and_then(|r| r.decode().map_err(|e| e.to_string()))
}

/// Reject images whose pixel count exceeds [`MAX_IMAGE_PIXELS`].
fn check_image_size(width: u32, height: u32) -> Result<(), String> {
    let pixels = u64::from(width) * u64::from(height);
    if pixels > MAX_IMAGE_PIXELS {
        return Err(format!(
            "image too large to display: {width}x{height} ({pixels} pixels, max {MAX_IMAGE_PIXELS})"
        ));
    }
    Ok(())
}

impl ImageViewerState {
    pub fn set_image_load_channel(&mut self, tx: UnboundedSender<ImageLoadResult>) {
        self.image_load_tx = Some(tx);
    }

    pub fn resize_receiver(
        &mut self,
    ) -> Option<&mut UnboundedReceiver<ratatui_image::thread::ResizeRequest>> {
        self.resize_rx.as_mut()
    }

    /// Whether a decoded image is currently held in memory.
    #[must_use]
    pub fn has_image(&self) -> bool {
        self.image_zoom.image.is_some()
    }

    /// Drop the decoded image, its scaled cache and any installed protocol.
    pub fn release_decoded(&mut self) {
        self.empty_protocol();
        self.image_zoom.reset();
    }

    fn empty_protocol(&mut self) {
        if let Some(tp) = &mut self.protocol {
            tp.empty_protocol();
        }
    }

    pub(super) fn install_protocol(&mut self, proto: StatefulProtocol) {
        if let Some(tp) = &mut self.protocol {
            tp.replace_protocol(proto);
        } else if let Some(tx) = &self.resize_tx {
            self.protocol = Some(ratatui_image::thread::ThreadProtocol::new(
                tx.clone(),
                Some(proto),
            ));
        }
    }

    pub(super) fn init_channels(&mut self) {
        let (tx, rx) = unbounded_channel();
        self.resize_tx = Some(tx);
        self.resize_rx = Some(rx);
    }

    fn ensure_channels(&mut self) {
        if self.resize_tx.is_none() {
            self.init_channels();
        }
    }

    pub fn init_picker_detached(&mut self) {
        if self.picker.is_some() {
            return;
        }
        self.ensure_channels();
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
        self.ensure_channels();
        self.picker = Some(Self::create_picker().await);
    }

    async fn create_picker() -> ratatui_image::picker::Picker {
        tokio::task::spawn_blocking(|| {
            ratatui_image::picker::Picker::from_query_stdio()
                .unwrap_or_else(|_| ratatui_image::picker::Picker::halfblocks())
        })
        .await
        .unwrap_or_else(|_| ratatui_image::picker::Picker::halfblocks())
    }

    fn font_size(&self) -> Option<ratatui_image::FontSize> {
        self.picker
            .as_ref()
            .map(ratatui_image::picker::Picker::font_size)
    }

    pub fn load_image(
        &mut self,
        path: &std::path::Path,
        provider: &std::sync::Arc<dyn crate::fs::provider::FileSystemProvider>,
        cancel_flag: Arc<AtomicBool>,
    ) {
        // Ensure channels and picker initialization are kicked off
        if self.resize_tx.is_none() {
            self.init_picker_detached();
        }

        if let (Some(image_tx), Some(_resize_tx)) = (&self.image_load_tx, &self.resize_tx) {
            self.current_load_id += 1;
            let load_id = self.current_load_id;
            let image_tx_clone = image_tx.clone();
            let path_clone = path.to_path_buf();
            let provider_clone = std::sync::Arc::clone(provider);
            let picker = self.picker.clone();

            tokio::spawn(async move {
                // Resolve the picker off the runtime worker: `from_query_stdio` performs
                // blocking stdio (and a blocking `tmux` process launch) and must not run on an
                // async worker. This only happens while the detached picker init is in flight.
                let p = match picker {
                    Some(p) => p,
                    None => tokio::task::spawn_blocking(|| {
                        ratatui_image::picker::Picker::from_query_stdio()
                            .unwrap_or_else(|_| ratatui_image::picker::Picker::halfblocks())
                    })
                    .await
                    .unwrap_or_else(|_| ratatui_image::picker::Picker::halfblocks()),
                };

                let data = match provider_clone.read_file(&path_clone).await {
                    Ok(data) => data,
                    Err(e) => {
                        let _ = image_tx_clone.send(ImageLoadResult {
                            load_id,
                            path: path_clone,
                            result: Err(e.to_string()),
                            image: None,
                            picker: None,
                        });
                        return;
                    }
                };

                // Bail out before the expensive decode if a newer load superseded this one.
                if cancel_flag.load(Ordering::Relaxed) {
                    return;
                }

                // Decode off the runtime workers: image decode is CPU-bound and can be
                // hundreds of MB; it must not block a tokio worker (a few large images can
                // starve the runtime).
                let (result, image) =
                    tokio::task::spawn_blocking(move || match decode_image_capped(data) {
                        Ok(dyn_image) => (
                            Ok(p.new_resize_protocol(dyn_image.clone())),
                            Some(dyn_image),
                        ),
                        Err(e) => (Err(e), None),
                    })
                    .await
                    .unwrap_or_else(|_| (Err("image decode task failed".to_string()), None));

                if cancel_flag.load(Ordering::Relaxed) {
                    return;
                }
                let _ = image_tx_clone.send(ImageLoadResult {
                    load_id,
                    path: path_clone,
                    result,
                    image,
                    picker: None,
                });
            });
        }
    }

    /// Scale factor that fits the image proportionally into the viewport (capped at 1.0).
    #[must_use]
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    pub fn fit_scale(&self, render_area: ratatui::layout::Rect) -> f32 {
        let (Some(img), Some(font)) = (&self.image_zoom.image, self.font_size()) else {
            return 1.0;
        };
        if render_area.width == 0 || render_area.height == 0 {
            return 1.0;
        }
        let nw = img.width() as f32 / f32::from(font.width);
        let nh = img.height() as f32 / f32::from(font.height);
        (f32::from(render_area.width) / nw)
            .min(f32::from(render_area.height) / nh)
            .min(1.0)
    }

    /// Whether the image is currently displayed zoomed in beyond "fit" (panning is active).
    #[must_use]
    pub fn is_zoomed(&self, render_area: ratatui::layout::Rect) -> bool {
        self.image_zoom.is_zoomed(self.fit_scale(render_area))
    }

    /// Zoom in one step towards 100%.
    pub fn zoom_in(&mut self, render_area: ratatui::layout::Rect) {
        self.image_zoom.zoom_in(self.fit_scale(render_area));
    }

    /// Zoom out one step towards "fit".
    pub fn zoom_out(&mut self, render_area: ratatui::layout::Rect) {
        self.image_zoom.zoom_out(self.fit_scale(render_area));
    }

    /// Pan the displayed window by `(dx, dy)` viewport cells (clamped to image bounds).
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::similar_names
    )]
    pub fn pan(&mut self, dx: i64, dy: i64, render_area: ratatui::layout::Rect) {
        if !self.is_zoomed(render_area) {
            return;
        }
        let Some(font) = self.font_size() else {
            return;
        };
        let fit = self.fit_scale(render_area);
        let scale = self.image_zoom.effective_zoom(fit);
        let (rw, rh) = self.render_pixel_size(scale);
        let rw_cells = (rw as f32 / f32::from(font.width)).ceil() as u16;
        let rh_cells = (rh as f32 / f32::from(font.height)).ceil() as u16;
        let max_x = rw_cells.saturating_sub(render_area.width);
        let max_y = rh_cells.saturating_sub(render_area.height);
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
    pub fn prepare_protocol(&mut self, render_area: ratatui::layout::Rect) {
        let (Some(font), Some(picker)) = (self.font_size(), self.picker.clone()) else {
            return;
        };
        if self.image_zoom.image.is_none() {
            return;
        }

        let area = render_area;
        let fit = self.fit_scale(area);
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
            self.build_zoomed_protocol(&picker, font, scale, area)
        } else {
            self.image_zoom.scaled = None;
            self.image_zoom.scaled_zoom = 0.0;
            self.image_zoom
                .image
                .clone()
                .map(|img| picker.new_resize_protocol(img))
        };

        let Some(proto) = proto else { return };

        self.install_protocol(proto);
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
        area: ratatui::layout::Rect,
    ) -> Option<StatefulProtocol> {
        let (rw, rh) = self.render_pixel_size(scale);
        let vw = (f32::from(area.width) * f32::from(font.width)).ceil() as u32;
        let vh = (f32::from(area.height) * f32::from(font.height)).ceil() as u32;
        let max_x = rw.saturating_sub(vw);
        let max_y = rh.saturating_sub(vh);
        let ox =
            ((f32::from(self.image_zoom.pan_x) * f32::from(font.width)).round() as u32).min(max_x);
        let oy =
            ((f32::from(self.image_zoom.pan_y) * f32::from(font.height)).round() as u32).min(max_y);

        let scaled = self.scale_for_zoom(rw, rh, scale)?;
        let crop = scaled.crop_imm(ox, oy, vw, vh);

        let mut proto = picker.new_resize_protocol(crop);
        // The crop is already exactly the viewport's pixel size, so `Scale` resizing is a
        // no-op; only the encode runs. Encode synchronously so the same frame that pans also
        // renders the image, instead of sending the protocol through the async `ThreadProtocol`
        // which would leave a cleared (blank) frame until the worker returns.
        proto.resize_encode(
            &ratatui_image::Resize::Scale(Some(ratatui_image::FilterType::CatmullRom)),
            area.into(),
        );
        Some(proto)
    }

    /// Returns the source image resized to `(rw, rh)`, caching it for the given `scale`.
    ///
    /// Reads the source from `self.image_zoom.image` directly (by reference) rather than
    /// cloning the full-size source on every pan/zoom event.
    #[allow(clippy::float_cmp)]
    fn scale_for_zoom(&mut self, rw: u32, rh: u32, scale: f32) -> Option<image::DynamicImage> {
        if self.image_zoom.scaled_zoom == scale
            && let Some(scaled) = &self.image_zoom.scaled
        {
            return Some(scaled.clone());
        }
        let img = self.image_zoom.image.as_ref()?;
        let scaled = img.resize(
            rw.max(1),
            rh.max(1),
            image::imageops::FilterType::CatmullRom,
        );
        self.image_zoom.scaled = Some(scaled.clone());
        self.image_zoom.scaled_zoom = scale;
        Some(scaled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::file_viewer::FileViewerState;
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
        fv.image.picker = Some(ratatui_image::picker::Picker::halfblocks());
        fv.image.image_zoom.image = Some(image::DynamicImage::new_rgba8(2000, 2000));
        fv.render_area = Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 50,
        };
        fv.image.init_channels();
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

        let scale = fv.image.image_zoom.effective_zoom(fit);
        let (rw, rh) = fv.image.render_pixel_size(scale);
        let font = fv.image.font_size().unwrap();
        let max_x = (rw as f32 / f32::from(font.width)).ceil() as u16 - fv.render_area.width;
        let max_y = (rh as f32 / f32::from(font.height)).ceil() as u16 - fv.render_area.height;

        fv.pan_image(100_000, 100_000);
        assert_eq!(
            (fv.image.image_zoom.pan_x, fv.image.image_zoom.pan_y),
            (max_x, max_y)
        );

        fv.pan_image(-100_000, -100_000);
        assert_eq!(
            (fv.image.image_zoom.pan_x, fv.image.image_zoom.pan_y),
            (0, 0)
        );
    }

    #[test]
    fn prepare_image_protocol_installs_zoomed_protocol_and_caches_scaled() {
        let mut fv = test_viewer();
        fv.zoom_image_in();
        fv.prepare_image_protocol();
        assert!(fv.image.protocol.is_some());
        assert!(fv.image.image_zoom.scaled.is_some());
        let cached_zoom = fv.image.image_zoom.scaled_zoom;

        // Panning alone must reuse the cached scaled image (no re-resize).
        fv.pan_image(3, 3);
        fv.prepare_image_protocol();
        assert!((fv.image.image_zoom.scaled_zoom - cached_zoom).abs() < 1e-6);
        assert!(fv.image.image_zoom.scaled.is_some());

        // Zooming to a new level re-resizes.
        fv.zoom_image_in();
        fv.prepare_image_protocol();
        assert!(fv.image.image_zoom.scaled_zoom > cached_zoom);
    }

    #[test]
    fn prepare_image_protocol_clears_scaled_cache_when_back_at_fit() {
        let mut fv = test_viewer();
        fv.zoom_image_in();
        fv.prepare_image_protocol();
        assert!(fv.image.image_zoom.scaled.is_some());

        fv.zoom_image_out(); // fit*1.25*0.8 == fit -> resets to fit
        assert!(!fv.is_image_zoomed());
        fv.prepare_image_protocol();
        assert!(fv.image.image_zoom.scaled.is_none());
        assert!(fv.image.protocol.is_some());
    }

    #[test]
    fn prepare_image_protocol_handles_narrow_image_without_panic() {
        // A tall, narrow image whose zoomed width stays below the viewport width: the
        // crop must be clamped to the image rather than exceeding it.
        let mut fv = FileViewerState::new(true, "dark");
        fv.image.picker = Some(ratatui_image::picker::Picker::halfblocks());
        fv.image.image_zoom.image = Some(image::DynamicImage::new_rgba8(100, 4000));
        fv.render_area = Rect {
            x: 0,
            y: 0,
            width: 200,
            height: 100,
        };
        fv.image.init_channels();

        fv.zoom_image_in();
        assert!(fv.is_image_zoomed());
        fv.prepare_image_protocol();
        assert!(fv.image.image_zoom.scaled.is_some());
        assert!(fv.image.protocol.is_some());
    }

    #[test]
    fn zoomed_protocol_is_encoded_and_ready_to_render() {
        let mut fv = test_viewer();
        fv.zoom_image_in();
        fv.pan_image(3, 3);
        fv.prepare_image_protocol();
        let protocol = fv.image.protocol.as_mut().expect("protocol installed");
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
        assert!(fv.image.image_zoom.image.is_some());

        fv.release_image();
        assert!(fv.image.image_zoom.image.is_none());
        assert!(fv.image.image_zoom.scaled.is_none());
        assert!(
            fv.image
                .protocol
                .as_ref()
                .is_none_or(|p| p.protocol_type().is_none())
        );
    }

    #[test]
    fn decode_image_capped_decodes_small_image() {
        let img = image::DynamicImage::ImageRgba8(image::RgbaImage::new(3, 2));
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Png).unwrap();

        let decoded = decode_image_capped(buf.into_inner()).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (3, 2));
    }

    #[test]
    fn check_image_size_enforces_pixel_cap() {
        // Well under the cap.
        assert!(check_image_size(100, 100).is_ok());
        // Exactly at the cap (10000 * 5000 == MAX_IMAGE_PIXELS) is allowed.
        assert!(check_image_size(10_000, 5_000).is_ok());
        // Just over the cap is rejected.
        assert!(check_image_size(10_000, 5_001).is_err());
        // A pathological 20000x20000 (~400 MP) is rejected.
        assert!(check_image_size(20_000, 20_000).is_err());
    }
}
