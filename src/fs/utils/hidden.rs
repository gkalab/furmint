/// Returns true if the name is "." or "..".
#[must_use]
pub fn is_dot_or_dotdot(name: &str) -> bool {
    name == "." || name == ".."
}

/// True when the entries of a filesystem follow the Unix conventions, i.e. they
/// carry Unix-style permissions (`drwxr-xr-x`) and mark hidden entries with a
/// leading dot.
#[must_use]
pub fn has_unix_attributes(local: bool) -> bool {
    cfg!(not(windows)) || !local
}

/// Returns true if a name marks a hidden entry on a Unix-like filesystem, i.e.
/// it starts with a dot. `unix_attributes` is [`has_unix_attributes`] for the
/// filesystem the entry was listed from.
///
/// `.` and `..` are navigation entries rather than hidden ones, so they are
/// never reported as hidden.
#[must_use]
pub fn has_hidden_name(name: &str, unix_attributes: bool) -> bool {
    unix_attributes && !is_dot_or_dotdot(name) && name.starts_with('.')
}

/// Returns true if an attribute string carries the Windows
/// `FILE_ATTRIBUTE_HIDDEN` flag.
#[must_use]
pub fn has_hidden_attribute(attributes: &str) -> bool {
    attributes.as_bytes().get(2) == Some(&b'h')
}

/// Returns true if an entry is hidden on the filesystem it was listed from.
///
/// Unix-like filesystems only have the leading-dot convention, whereas Windows
/// marks hidden entries with `FILE_ATTRIBUTE_HIDDEN` instead, so a dot-prefixed
/// name alone is not enough there. Windows therefore relies solely on the DOS
/// flag, while Unix relies solely on the leading dot.
#[must_use]
pub fn is_hidden(name: &str, attributes: &str, unix_attributes: bool) -> bool {
    has_hidden_attribute(attributes) || has_hidden_name(name, unix_attributes)
}
