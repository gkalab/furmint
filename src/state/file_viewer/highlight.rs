//! Syntax highlighting off the render thread.
//!
//! Highlighting runs on its own thread. The viewer keeps the styled runs it
//! has already computed and asks the worker for the lines it is missing; a line
//! that has not come back yet simply renders unstyled. Nothing on the render
//! path ever waits for a compile.

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, PoisonError};

use lumis::highlight::Highlighter;
use lumis::languages::Language;
use lumis::themes::Theme;
use ratatui::style::Color;
use tokio::sync::mpsc::UnboundedSender;

/// One line split into styled runs.
///
/// Each run is the foreground Lumis resolved for the scope plus the run's own
/// text. `None` means the scope carries no colour of its own, in which case the
/// viewer's theme foreground applies — the same fallback the renderer used when
/// it parsed the hex colour per character.
pub type LineSegments = Vec<(Option<Color>, String)>;

/// A request for the highlight worker.
///
/// Only one request is ever in flight: a newer one replaces whatever is waiting,
/// so scrolling quickly or switching files never queues work whose result is no
/// longer wanted.
pub enum HighlightRequest {
    /// Compile a language's queries without highlighting anything, so the first
    /// real request for that language does not pay for the compile.
    Warm {
        language: Language,
        theme: Option<String>,
    },
    /// Highlight these `(line index, line text)` pairs.
    Lines {
        generation: usize,
        language: Language,
        theme: Option<String>,
        lines: Vec<(usize, String)>,
    },
}

/// Styled runs for some lines, as produced by the worker.
pub struct HighlightBatch {
    /// The viewer generation the work was requested for. Results from an older
    /// generation belong to a file that is no longer open and are dropped.
    pub generation: usize,
    pub lines: Vec<(usize, LineSegments)>,
}

/// Parses a `#rrggbb` or bare `rrggbb` colour.
///
/// Returns `None` for anything that is not a complete six-digit hex colour, so
/// a malformed theme value falls back to the caller's default rather than
/// producing a wrong colour.
#[must_use]
pub fn parse_hex_color(hex: Option<&str>) -> Option<Color> {
    let hex = hex?;
    let hex = hex.strip_prefix('#').unwrap_or(hex);
    if hex.len() < 6 {
        return None;
    }
    let r = u8::from_str_radix(hex.get(0..2)?, 16).ok()?;
    let g = u8::from_str_radix(hex.get(2..4)?, 16).ok()?;
    let b = u8::from_str_radix(hex.get(4..6)?, 16).ok()?;
    Some(Color::Rgb(r, g, b))
}

/// Handle to the highlight worker, held by the viewer state.
#[derive(Clone)]
pub struct HighlightWorker {
    slot: Slot,
}

/// The worker's mailbox: the pending request plus the doorbell that wakes it.
type Slot = Arc<(Mutex<Option<HighlightRequest>>, Condvar)>;

impl HighlightWorker {
    /// Starts the worker thread and returns a handle to it.
    ///
    /// `tx` carries finished batches back to the event loop. The thread owns
    /// every [`Highlighter`] it builds: Lumis's highlighter is `Send` but not
    /// `Sync`, so one thread keeping them all is both correct and the reason a
    /// parser and its query cursors are reused instead of rebuilt per line.
    ///
    /// The thread is detached. It blocks on the mailbox when idle and the
    /// process exits immediately after the event loop returns, so there is
    /// nothing to shut down.
    #[must_use]
    pub fn start(tx: UnboundedSender<HighlightBatch>) -> Self {
        let slot: Slot = Arc::new((Mutex::new(None), Condvar::new()));
        let worker_slot = Arc::clone(&slot);
        std::thread::Builder::new()
            .name("fm-highlight".to_string())
            .spawn(move || run(&worker_slot, &tx))
            .ok();
        Self { slot }
    }

    /// Hands a request to the worker, replacing any request still waiting.
    pub fn submit(&self, request: HighlightRequest) {
        let (lock, doorbell) = &*self.slot;
        *lock.lock().unwrap_or_else(PoisonError::into_inner) = Some(request);
        doorbell.notify_one();
    }
}

fn run(slot: &Slot, tx: &UnboundedSender<HighlightBatch>) {
    let mut worker = Worker::default();
    let (lock, doorbell) = &**slot;

    loop {
        let mut pending = lock.lock().unwrap_or_else(PoisonError::into_inner);
        while pending.is_none() {
            pending = doorbell
                .wait(pending)
                .unwrap_or_else(PoisonError::into_inner);
        }
        let Some(request) = pending.take() else {
            continue;
        };
        drop(pending);

        // A closed receiver means the app is shutting down; stop doing work.
        if tx.is_closed() {
            return;
        }

        match request {
            HighlightRequest::Warm { language, theme } => worker.warm(language, theme),
            HighlightRequest::Lines {
                generation,
                language,
                theme,
                lines,
            } => {
                let mut done: Vec<(usize, LineSegments)> = Vec::new();
                for (index, line) in lines {
                    // Stop as soon as something newer is wanted: the lines
                    // already done are still worth sending, the rest are not.
                    if lock
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .is_some()
                    {
                        break;
                    }
                    done.push((index, worker.highlight_line(language, theme.clone(), &line)));
                }
                if !done.is_empty()
                    && tx
                        .send(HighlightBatch {
                            generation,
                            lines: done,
                        })
                        .is_err()
                {
                    return;
                }
            }
        }
    }
}

/// Highlighter and theme caches owned by the worker thread.
#[derive(Default)]
struct Worker {
    /// One highlighter per `(language, theme)` pair.
    ///
    /// Lumis binds both when a highlighter is built, so a theme change needs its
    /// own entry. Rebuilding one is cheap once the language's queries exist,
    /// which is why a warm request can be answered by highlighting an empty line.
    highlighters: HashMap<(Language, Option<String>), Highlighter>,
    /// Parsed themes, kept so a new highlighter does not re-read one.
    themes: HashMap<String, Theme>,
}

impl Worker {
    /// Compiles a language's queries by highlighting an empty line.
    fn warm(&mut self, language: Language, theme: Option<String>) {
        let _ = self.highlighter(language, theme).highlight("");
    }

    fn theme(&mut self, name: &str) -> Theme {
        if let Some(theme) = self.themes.get(name) {
            return theme.clone();
        }
        let theme = lumis::themes::get(name).unwrap_or_default();
        self.themes.insert(name.to_string(), theme.clone());
        theme
    }

    fn highlighter(&mut self, language: Language, theme: Option<String>) -> &mut Highlighter {
        let resolved = theme.as_deref().map(|name| self.theme(name));
        self.highlighters
            .entry((language, theme))
            .or_insert_with(|| Highlighter::new(language, resolved))
    }

    /// Highlights one line into owned, colour-resolved runs.
    ///
    /// A line that fails to highlight renders as plain text rather than
    /// disappearing: the runs Lumis returns always cover the whole source, so an
    /// empty result would leave a blank line where the text should be.
    fn highlight_line(
        &mut self,
        language: Language,
        theme: Option<String>,
        line: &str,
    ) -> LineSegments {
        let segments = self
            .highlighter(language, theme)
            .highlight(line)
            .unwrap_or_default();
        if segments.is_empty() {
            return vec![(None, line.to_string())];
        }
        segments
            .into_iter()
            .map(|(style, text)| (parse_hex_color(style.fg.as_deref()), text.to_string()))
            .collect()
    }
}
