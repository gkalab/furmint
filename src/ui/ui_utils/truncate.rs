#[must_use]
pub fn truncate_middle_with_ellipsis(name: &str, max_width: usize) -> String {
    let ellipsis = "…"; // Unicode ellipsis
    let ellipsis_len = ellipsis.chars().count();
    let name_len = name.chars().count();
    if name_len <= max_width {
        return name.to_string();
    }
    if max_width <= ellipsis_len {
        return ellipsis.repeat(max_width);
    }
    let keep = max_width - ellipsis_len;
    let left = keep / 2;
    let right = keep - left;
    let left_str: String = name.chars().take(left).collect();
    let right_str: String = name.chars().skip(name_len - right).collect();
    let mut result = format!("{left_str}{ellipsis}{right_str}");
    let result_len = result.chars().count();
    if result_len > max_width {
        result = result.chars().take(max_width).collect();
    } else if result_len < max_width {
        result = format!("{result:<max_width$}");
    }
    result
}

/// Shared core: rebuild `raw` keeping the first `left_count` and last `right_count`
/// segments joined by `sep`, inserting an ellipsis between them, while staying within
/// `max_width` chars. Returns `Some(result)` when a combination fits, `None` if even
/// the minimal split is too wide (callers then fall back to a middle truncation).
fn truncate_segments_with_ellipsis(raw: &str, sep: char, max_width: usize) -> Option<String> {
    let sep_str = sep.to_string();
    let has_leading_sep = raw.starts_with(sep);
    let segments: Vec<&str> = raw.split(sep).filter(|s| !s.is_empty()).collect();
    let total = segments.len();

    if total == 0 {
        return Some(String::new());
    }

    let ellipsis = "…";
    let mut left_count = 1;
    let mut right_count = 1;
    let mut last_result = None;

    // Iterate to find the best fit
    while left_count + right_count < total {
        let mut result = String::new();
        if has_leading_sep {
            result.push(sep);
        }
        result.push_str(&segments[..left_count].join(&sep_str));
        result.push(sep);
        result.push_str(ellipsis);
        result.push(sep);
        result.push_str(&segments[total.saturating_sub(right_count)..].join(&sep_str));

        if result.chars().count() > max_width {
            break;
        }

        last_result = Some(result);

        // Try to add more segments
        if left_count <= right_count {
            left_count += 1;
        } else {
            right_count += 1;
        }
    }

    last_result
}

#[must_use]
pub fn truncate_path_with_ellipsis(path: &std::path::Path, max_width: usize) -> String {
    let path_str = path.to_string_lossy();
    if path_str.chars().count() <= max_width {
        return path_str.to_string();
    }

    // Prefer the separator actually present in the configured path so that strings
    // using forward slashes (remote-style paths) keep them instead of being rebuilt
    // with the platform separator.
    let sep = if path_str.contains('\\') { '\\' } else { '/' };

    match truncate_segments_with_ellipsis(&path_str, sep, max_width) {
        Some(result) => result,
        None => truncate_middle_with_ellipsis(&path_str, max_width),
    }
}

/// Truncates a `/`-separated path string (as used for remote/SSH paths) to fit
/// `max_width` characters, keeping the first and last path segments and inserting
/// an ellipsis in the middle. Any backslashes are normalized to forward slashes
/// first so the result never mixes separators.
#[must_use]
pub fn truncate_path_str_with_ellipsis(path: &str, max_width: usize) -> String {
    let normalized = path.replace('\\', "/");
    if normalized.chars().count() <= max_width {
        return normalized;
    }

    match truncate_segments_with_ellipsis(&normalized, '/', max_width) {
        Some(result) => result,
        None => truncate_middle_with_ellipsis(&normalized, max_width),
    }
}

/// Truncates an arbitrary path for display, choosing the separator handling per
/// path: native local paths containing backslashes (e.g. Windows `C:\...`) keep
/// their separators, while remote-style paths (no backslashes, typically with a
/// leading `/`) are rendered with forward slashes, normalizing any stray
/// backslashes. This lets mixed lists (e.g. the bookmark list spanning local and
/// remote contexts) render each entry correctly without a per-context flag.
#[must_use]
pub fn truncate_path_for_display(path: &std::path::Path, max_width: usize) -> String {
    let path_str = path.to_string_lossy();
    if path_str.contains('\\') && !path_str.starts_with('/') {
        truncate_path_with_ellipsis(path, max_width)
    } else {
        truncate_path_str_with_ellipsis(&path_str, max_width)
    }
}

/// Truncates a `/`-separated path string to fit `max_width` characters, keeping
/// the first and last path segments and inserting an ellipsis in the middle.
/// This is used for remote paths, which are plain strings (forward-slash) rather
/// than `Path` values, so the platform's `Path`/`PathBuf` separator handling is
/// avoided.
#[must_use]
pub fn truncate_path_str(path: &str, max_width: usize) -> String {
    if path.chars().count() <= max_width {
        return path.to_string();
    }
    truncate_middle_with_ellipsis(path, max_width)
}
