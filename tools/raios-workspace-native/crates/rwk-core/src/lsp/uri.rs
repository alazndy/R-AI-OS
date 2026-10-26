//! Conversion between project-relative paths and the `file://` URIs a language server speaks.
//! A URI that leaves the project is never turned back into a path.

use std::path::Path;

use crate::project::validate_relative_path;

fn unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/')
}

/// The `file://` URI of `relative` inside `root`.
pub fn path_to_uri(root: &Path, relative: &str) -> String {
    let full = root.join(relative);
    let mut uri = String::from("file://");
    let text = full.to_string_lossy();
    // On Windows a drive path needs a leading slash; elsewhere the path already has one.
    if !text.starts_with('/') {
        uri.push('/');
    }
    for byte in text.replace('\\', "/").bytes() {
        if unreserved(byte) {
            uri.push(char::from(byte));
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri
}

fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = text.get(index + 1..index + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// The project-relative path a URI names, or `None` when it is not a `file://` URI, names
/// something outside `root`, or is not a plain relative path.
pub fn uri_to_relative(root: &Path, uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file://")?;
    // An authority other than empty or `localhost` is another machine.
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let decoded = percent_decode(rest)?;
    let decoded = decoded
        .strip_prefix('/')
        .filter(|_| cfg!(windows))
        .unwrap_or(&decoded);
    let path = Path::new(decoded);
    let relative = path.strip_prefix(root).ok()?;
    let relative = relative.to_str()?.replace('\\', "/");
    validate_relative_path(&relative, false).ok()?;
    Some(relative)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn paths_round_trip_with_spaces_and_non_ascii() {
        let root = Path::new("/work/my app");
        let uri = path_to_uri(root, "src/ünï code.rs");
        assert_eq!(uri, "file:///work/my%20app/src/%C3%BCn%C3%AF%20code.rs");
        assert_eq!(
            uri_to_relative(root, &uri).as_deref(),
            Some("src/ünï code.rs")
        );
    }

    #[cfg(unix)]
    #[test]
    fn uris_outside_the_project_or_with_tricks_are_dropped() {
        let root = Path::new("/work/app");
        for uri in [
            "file:///work/other/x.rs",
            "file:///work/app/../other/x.rs",
            "file:///work/appx/x.rs",
            "file:///work/app",
            "file:///work/app/",
            "file:///etc/passwd",
            "http://example.com/x.rs",
            "file://host/work/app/x.rs",
            "file:///work/app/%2e%2e/x.rs",
            "file:///work/app/a%00b",
            "file:///work/app/%zz",
            "file:///work/app/%C3",
        ] {
            assert_eq!(uri_to_relative(root, uri), None, "{uri}");
        }
        assert_eq!(
            uri_to_relative(root, "file://localhost/work/app/x.rs").as_deref(),
            Some("x.rs")
        );
    }
}
