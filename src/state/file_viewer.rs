use std::path::PathBuf;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

#[derive(Default)]
pub struct FileViewerState {
    pub path: PathBuf,
    pub content: Vec<String>,
    pub scroll_offset: usize,
    pub horizontal_scroll_offset: usize,
    pub is_visible: bool,
    pub syntax_set: syntect::parsing::SyntaxSet,
    pub theme: syntect::highlighting::Theme,
    pub syntax_name: Option<String>,
    pub focused: bool,
    pub protocol: Option<ratatui_image::thread::ThreadProtocol>,
    pub resize_rx: Option<UnboundedReceiver<ratatui_image::thread::ResizeRequest>>,
    pub resize_tx: Option<UnboundedSender<ratatui_image::thread::ResizeRequest>>,
    pub picker: Option<ratatui_image::picker::Picker>,
    pub image_load_tx: Option<UnboundedSender<ImageLoadResult>>,
    pub is_loading: bool,
    pub current_load_id: usize,
}

pub struct ImageLoadResult {
    pub load_id: usize,
    pub path: PathBuf,
    pub result: Result<ratatui_image::protocol::StatefulProtocol, String>,
    pub picker: Option<ratatui_image::picker::Picker>,
}

impl FileViewerState {
    pub fn new(is_dark_theme: bool, app_theme_name: &str) -> Self {
        let syntax_set = syntect::parsing::SyntaxSet::load_defaults_newlines();
        let theme_set = syntect::highlighting::ThemeSet::load_defaults();
        let theme_name = if app_theme_name == "solarized light" {
            "Solarized (light)"
        } else if is_dark_theme {
            "base16-eighties.dark"
        } else {
            "InspiredGitHub"
        };
        let theme = theme_set.themes[theme_name].clone();

        Self {
            path: PathBuf::new(),
            content: Vec::new(),
            scroll_offset: 0,
            horizontal_scroll_offset: 0,
            is_visible: false,
            syntax_set,
            theme,
            syntax_name: None,
            focused: false,
            protocol: None,
            resize_rx: None,
            resize_tx: None,
            picker: None,
            image_load_tx: None,
            is_loading: false,
            current_load_id: 0,
        }
    }

    pub fn reset(&mut self) {
        if let Some(tp) = &mut self.protocol {
            tp.empty_protocol();
        }
        self.content = Vec::new();
        self.scroll_offset = 0;
        self.horizontal_scroll_offset = 0;
        self.syntax_name = None;
    }

    pub fn init_picker(&mut self) {
        if self.picker.is_some() {
            return;
        }
        // Initialize channels once
        if self.resize_tx.is_none() {
            let (tx, rx) = unbounded_channel();
            self.resize_tx = Some(tx);
            self.resize_rx = Some(rx);
        }

        let tx = self.image_load_tx.clone();
        tokio::spawn(async move {
            let picker = tokio::task::spawn_blocking(|| {
                ratatui_image::picker::Picker::from_query_stdio()
                    .unwrap_or_else(|_| ratatui_image::picker::Picker::halfblocks())
            })
            .await
            .unwrap_or_else(|_| ratatui_image::picker::Picker::halfblocks());

            if let Some(tx) = tx {
                let _ = tx.send(ImageLoadResult {
                    load_id: 0,
                    path: PathBuf::new(),
                    result: Err("PICKER_INIT".to_string()),
                    picker: Some(picker),
                });
            }
        });
    }

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

    pub fn load_content(
        &mut self,
        path: PathBuf,
        provider: std::sync::Arc<dyn crate::fs::fs_provider::FileSystemProvider>,
        size: Option<u64>,
        limit_bytes: u64,
    ) {
        self.reset();
        self.path.clone_from(&path);
        if provider.is_dir(&path) {
            self.content = vec!["Directory".to_string()];
            return;
        }

        if Self::is_image(&path) {
            // Ensure channels and picker initialization are kicked off
            if self.resize_tx.is_none() {
                self.init_picker();
            }

            if let (Some(image_tx), Some(_resize_tx)) = (&self.image_load_tx, &self.resize_tx) {
                self.is_loading = true;
                self.current_load_id += 1;
                let load_id = self.current_load_id;
                let path_clone = path.clone();
                let provider_clone = provider.clone();
                let image_tx_clone = image_tx.clone();
                let picker = self.picker.clone();

                tokio::spawn(async move {
                    let path_for_result = path_clone.clone();
                    let result = tokio::task::spawn_blocking(move || {
                        let p = picker.unwrap_or_else(|| {
                            ratatui_image::picker::Picker::from_query_stdio()
                                .unwrap_or_else(|_| ratatui_image::picker::Picker::halfblocks())
                        });

                        provider_clone
                            .read_file(&path_clone)
                            .map_err(|e| e.to_string())
                            .and_then(|data| {
                                use image::ImageReader;
                                use std::io::Cursor;
                                ImageReader::new(Cursor::new(data))
                                    .with_guessed_format()
                                    .map_err(|e| e.to_string())
                                    .and_then(|r| r.decode().map_err(|e| e.to_string()))
                            })
                            .map(|image| p.new_resize_protocol(image))
                    })
                    .await
                    .unwrap_or_else(|e| Err(e.to_string()));

                    let _ = image_tx_clone.send(ImageLoadResult {
                        load_id,
                        path: path_for_result,
                        result,
                        picker: None,
                    });
                });
                return;
            }
        }

        let syntax = self
            .syntax_set
            .find_syntax_for_file(&self.path)
            .unwrap_or(None)
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text());
        self.syntax_name = Some(syntax.name.clone());

        let limit = limit_bytes as usize;
        let limit_u64 = limit_bytes;

        if let Some(s) = size
            && s > limit_u64
        {
            self.content = vec![format!(
                "File too large to display (size: {}, limit: {})",
                crate::fs::utils::format_size(Some(s), false, false),
                crate::fs::utils::format_size(Some(limit_u64), false, false)
            )];
            return;
        }

        // Read small chunk to check for binary
        let chunk = match provider.read_file_at(&self.path, 0u64, 8192usize) {
            Ok(buf) => buf,
            Err(e) => {
                self.content = vec![format!("Error reading file: {e}")];
                return;
            }
        };
        if chunk.contains(&0) {
            self.content = vec!["Binary file detected".to_string()];
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
        if res.load_id != self.current_load_id {
            return;
        }
        self.is_loading = false;
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
}
