//! Conflict/error decisions answered by the user during a transfer.

pub struct DecisionState {
    pub overwrite_all: bool,
    pub skip_all: bool,
    pub last_update: std::time::Instant,
}

impl Default for DecisionState {
    fn default() -> Self {
        Self::new()
    }
}

impl DecisionState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            overwrite_all: false,
            skip_all: false,
            last_update: std::time::Instant::now(),
        }
    }

    #[must_use]
    pub fn should_overwrite(&self) -> bool {
        self.overwrite_all
    }

    #[must_use]
    pub fn should_skip(&self) -> bool {
        self.skip_all
    }
}

/// Which prompt a transfer is waiting on. The conflict and error popups put
/// different buttons behind the same decision channel, so knowing the prompt's
/// kind — not just the raw [`crate::tasks::TaskDecision`] — is what gives an
/// answer its meaning.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DecisionKind {
    /// The destination already exists: overwrite it, skip it, or cancel.
    Conflict,
    /// A transfer failed: retry it, skip the rest, or cancel.
    Error,
}

/// A [`crate::tasks::TaskDecision`] normalised to the transfer-wide vocabulary.
/// Both the recursive walker and the rsync path act on these, so a button means
/// the same thing no matter which path raised the prompt.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Decision {
    Overwrite,
    OverwriteAll,
    Skip,
    SkipAll,
    Retry,
    Cancel,
}

impl Decision {
    /// Translate the UI's answer for `kind`. Every variant is matched
    /// explicitly — a new [`crate::tasks::TaskDecision`] must be given a meaning
    /// here rather than silently falling through a wildcard — and a dropped
    /// channel (the task is gone) reads as "skip".
    // The repeated `Self::Skip` arms are deliberate: each names the variant of
    // one prompt, so adding a `TaskDecision` fails to compile here instead of
    // being absorbed into a wildcard.
    #[allow(clippy::match_same_arms)]
    pub(super) fn from_task_decision(
        kind: DecisionKind,
        raw: Option<crate::tasks::TaskDecision>,
    ) -> Self {
        use crate::tasks::TaskDecision as D;
        match (kind, raw) {
            (_, None) => Self::Skip,

            // The conflict popup maps 'm' to Merge; the recursive walker has
            // already descended into the destination by the time a file
            // conflict is raised, so merging is the same as overwriting.
            (DecisionKind::Conflict, Some(D::Overwrite | D::Merge)) => Self::Overwrite,
            (DecisionKind::Conflict, Some(D::OverwriteAll)) => Self::OverwriteAll,
            (DecisionKind::Conflict, Some(D::Skip)) => Self::Skip,
            (DecisionKind::Conflict, Some(D::SkipAll)) => Self::SkipAll,
            (DecisionKind::Conflict, Some(D::Cancel)) => Self::Cancel,
            // Retry belongs to the error prompt; ignore one that arrives here.
            (DecisionKind::Conflict, Some(D::Retry)) => Self::Skip,

            (DecisionKind::Error, Some(D::Retry)) => Self::Retry,
            (DecisionKind::Error, Some(D::Skip)) => Self::Skip,
            (DecisionKind::Error, Some(D::SkipAll)) => Self::SkipAll,
            (DecisionKind::Error, Some(D::Cancel)) => Self::Cancel,
            // Overwrite/Merge belong to the conflict prompt; ignore them here.
            (DecisionKind::Error, Some(D::Overwrite | D::OverwriteAll | D::Merge)) => Self::Skip,
        }
    }
}
