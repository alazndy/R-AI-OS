//! The wire format of the Language Server Protocol: a header block with `Content-Length`, a
//! blank line, then that many bytes of JSON.

use std::io::{BufRead, Read, Write};

use serde_json::Value;

use crate::error::{AppError, ErrorCode, Result};

/// Largest message accepted, in bytes. A server cannot make the client allocate more.
pub const MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;
/// Most header lines read before the block is refused.
const MAX_HEADER_LINES: usize = 16;
/// Longest header line read, in bytes.
const MAX_HEADER_LINE_BYTES: usize = 1024;

fn protocol(message: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::Io, message.into())
}

/// Reads one message. `Ok(None)` means the stream ended cleanly before a header started.
pub fn read_message(reader: &mut impl BufRead) -> Result<Option<Value>> {
    let mut length: Option<usize> = None;
    for line_number in 0..=MAX_HEADER_LINES {
        let mut line = Vec::new();
        let read = reader
            .by_ref()
            .take(MAX_HEADER_LINE_BYTES as u64 + 1)
            .read_until(b'\n', &mut line)?;
        if read == 0 {
            return if line_number == 0 {
                Ok(None)
            } else {
                Err(protocol("the server closed the stream inside a header"))
            };
        }
        if line.len() > MAX_HEADER_LINE_BYTES {
            return Err(protocol("a header line is too long"));
        }
        let text = String::from_utf8_lossy(&line);
        let text = text.trim_end_matches(['\r', '\n']);
        if text.is_empty() {
            let length = length.ok_or_else(|| protocol("a message has no Content-Length"))?;
            if length > MAX_MESSAGE_BYTES {
                return Err(protocol("a message is larger than the 16 MiB limit"));
            }
            let mut body = vec![0u8; length];
            reader.read_exact(&mut body)?;
            return serde_json::from_slice(&body)
                .map(Some)
                .map_err(|error| protocol(format!("a message is not JSON: {error}")));
        }
        if let Some((name, value)) = text.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                length = Some(
                    value
                        .trim()
                        .parse()
                        .map_err(|_| protocol("Content-Length is not a number"))?,
                );
            }
        }
    }
    Err(protocol("the header block has too many lines"))
}

/// Writes one message with its header and flushes.
pub fn write_message(writer: &mut impl Write, message: &Value) -> Result<()> {
    let body = serde_json::to_vec(message).map_err(|error| protocol(error.to_string()))?;
    if body.len() > MAX_MESSAGE_BYTES {
        return Err(protocol("the message is larger than the 16 MiB limit"));
    }
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(&body)?;
    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn framed(body: &str) -> Vec<u8> {
        format!("Content-Length: {}\r\n\r\n{body}", body.len()).into_bytes()
    }

    #[test]
    fn a_message_round_trips_and_two_in_a_row_are_read_one_at_a_time() {
        let mut wire = Vec::new();
        write_message(&mut wire, &json!({"id": 1, "method": "a"})).unwrap();
        write_message(&mut wire, &json!({"id": 2, "method": "b"})).unwrap();
        let mut reader = std::io::BufReader::new(wire.as_slice());
        assert_eq!(read_message(&mut reader).unwrap().unwrap()["id"], 1);
        assert_eq!(read_message(&mut reader).unwrap().unwrap()["id"], 2);
        assert!(read_message(&mut reader).unwrap().is_none());
    }

    #[test]
    fn other_headers_and_header_case_are_tolerated() {
        let body = r#"{"ok":true}"#;
        let wire = format!(
            "content-length: {}\r\nContent-Type: application/vscode-jsonrpc; charset=utf-8\r\n\r\n{body}",
            body.len()
        );
        let mut reader = std::io::BufReader::new(wire.as_bytes());
        assert_eq!(read_message(&mut reader).unwrap().unwrap()["ok"], true);
    }

    #[test]
    fn malformed_or_oversized_input_is_refused_not_allocated() {
        let refuse = |wire: Vec<u8>| {
            let mut reader = std::io::BufReader::new(wire.as_slice());
            read_message(&mut reader).is_err()
        };
        assert!(refuse(b"\r\n{}".to_vec()), "no length");
        assert!(refuse(b"Content-Length: x\r\n\r\n".to_vec()));
        assert!(refuse(
            format!("Content-Length: {}\r\n\r\n", MAX_MESSAGE_BYTES + 1).into_bytes()
        ));
        assert!(refuse(framed("not json")));
        assert!(
            refuse(b"Content-Length: 10\r\n\r\n{}".to_vec()),
            "short body"
        );
        let mut long = b"X: ".to_vec();
        long.extend(vec![b'a'; 2000]);
        long.extend(b"\r\n\r\n");
        assert!(refuse(long), "a very long header line");
        let many = "A: b\r\n".repeat(40) + "\r\n";
        assert!(refuse(many.into_bytes()), "too many header lines");
    }
}
