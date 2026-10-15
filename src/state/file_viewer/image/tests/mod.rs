use super::*;
use crate::layout::ViewerGeometry;
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
///
/// Returns the viewer together with the frame geometry its zoom/pan methods
/// take. The bordered `area` is deliberately larger than the render area.
fn test_viewer() -> (FileViewerState, ViewerGeometry) {
    let mut fv = FileViewerState::new(true, "dark");
    fv.image.picker = Some(ratatui_image::picker::Picker::halfblocks());
    fv.image.image_zoom.image = Some(image::DynamicImage::new_rgba8(2000, 2000));
    fv.image.init_channels();
    let geometry = ViewerGeometry {
        area: Rect {
            x: 0,
            y: 0,
            width: 104,
            height: 54,
        },
        render_area: Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 50,
        },
    };
    (fv, geometry)
}

#[test]
fn fit_scale_uses_render_area() {
    let (fv, geometry) = test_viewer();
    assert!((fv.fit_scale(&geometry) - 0.5).abs() < 1e-4);
}

#[test]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn pan_image_clamps_to_rendered_image_bounds() {
    let (mut fv, geometry) = test_viewer();
    let fit = fv.fit_scale(&geometry);
    fv.zoom_image_in(&geometry);
    assert!(fv.is_image_zoomed(&geometry));

    let scale = fv.image.image_zoom.effective_zoom(fit);
    let (rw, rh) = fv.image.render_pixel_size(scale);
    let font = fv.image.font_size().unwrap();
    let max_x = (rw as f32 / f32::from(font.width)).ceil() as u16 - geometry.render_area.width;
    let max_y = (rh as f32 / f32::from(font.height)).ceil() as u16 - geometry.render_area.height;

    fv.pan_image(100_000, 100_000, &geometry);
    assert_eq!(
        (fv.image.image_zoom.pan_x, fv.image.image_zoom.pan_y),
        (max_x, max_y)
    );

    fv.pan_image(-100_000, -100_000, &geometry);
    assert_eq!(
        (fv.image.image_zoom.pan_x, fv.image.image_zoom.pan_y),
        (0, 0)
    );
}

#[test]
fn prepare_image_protocol_installs_zoomed_protocol_and_caches_scaled() {
    let (mut fv, geometry) = test_viewer();
    fv.zoom_image_in(&geometry);
    fv.prepare_image_protocol(&geometry);
    assert!(fv.image.protocol.is_some());
    assert!(fv.image.image_zoom.scaled.is_some());
    let cached_zoom = fv.image.image_zoom.scaled_zoom;

    // Panning alone must reuse the cached scaled image (no re-resize).
    fv.pan_image(3, 3, &geometry);
    fv.prepare_image_protocol(&geometry);
    assert!((fv.image.image_zoom.scaled_zoom - cached_zoom).abs() < 1e-6);
    assert!(fv.image.image_zoom.scaled.is_some());

    // Zooming to a new level re-resizes.
    fv.zoom_image_in(&geometry);
    fv.prepare_image_protocol(&geometry);
    assert!(fv.image.image_zoom.scaled_zoom > cached_zoom);
}

#[test]
fn prepare_image_protocol_clears_scaled_cache_when_back_at_fit() {
    let (mut fv, geometry) = test_viewer();
    fv.zoom_image_in(&geometry);
    fv.prepare_image_protocol(&geometry);
    assert!(fv.image.image_zoom.scaled.is_some());

    fv.zoom_image_out(&geometry); // fit*1.25*0.8 == fit -> resets to fit
    assert!(!fv.is_image_zoomed(&geometry));
    fv.prepare_image_protocol(&geometry);
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
    fv.image.init_channels();
    let geometry = ViewerGeometry {
        area: Rect {
            x: 0,
            y: 0,
            width: 204,
            height: 104,
        },
        render_area: Rect {
            x: 0,
            y: 0,
            width: 200,
            height: 100,
        },
    };

    fv.zoom_image_in(&geometry);
    assert!(fv.is_image_zoomed(&geometry));
    fv.prepare_image_protocol(&geometry);
    assert!(fv.image.image_zoom.scaled.is_some());
    assert!(fv.image.protocol.is_some());
}

#[test]
fn zoomed_protocol_is_encoded_and_ready_to_render() {
    let (mut fv, geometry) = test_viewer();
    fv.zoom_image_in(&geometry);
    fv.pan_image(3, 3, &geometry);
    fv.prepare_image_protocol(&geometry);
    let protocol = fv.image.protocol.as_mut().expect("protocol installed");
    let resize = ratatui_image::Resize::Scale(Some(ratatui_image::FilterType::CatmullRom));
    assert!(
        protocol
            .needs_resize(&resize, geometry.render_area.into())
            .is_none(),
        "zoomed protocol must be already encoded so the next draw renders immediately"
    );
}

#[test]
fn release_image_drops_decoded_image_and_protocol() {
    let (mut fv, geometry) = test_viewer();
    fv.zoom_image_in(&geometry);
    fv.prepare_image_protocol(&geometry);
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
