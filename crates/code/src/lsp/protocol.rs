//! Bounded JSON-RPC stdio framing and private temporary workspace helpers for [`LspAnalyzer`].

use std::collections::hash_map::RandomState;
use std::fs::{DirBuilder, OpenOptions};
use std::hash::{BuildHasher, Hasher};
use std::io::{BufRead, ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

pub(super) const MAX_LSP_MESSAGE_BYTES: usize = 16 * 1024 * 1024;
const MAX_LSP_HEADER_LINE_BYTES: usize = 8 * 1024;
const MAX_LSP_HEADERS: usize = 64;
const WORKSPACE_ATTEMPTS: usize = 16;

static NEXT_LSP_SESSION: AtomicU64 = AtomicU64::new(0);

/// Incoming JSON-RPC frame classified into client response, server request, or notification.
#[derive(Debug, PartialEq)]
pub(super) enum IncomingFrame {
    Response(Value),
    ServerRequest { id: Value },
    Notification,
}

pub(super) fn classify_frame(message: Value) -> IncomingFrame {
    if let Some(id) = message.get("id").cloned() {
        if message.get("method").is_some() {
            IncomingFrame::ServerRequest { id }
        } else {
            IncomingFrame::Response(message)
        }
    } else {
        IncomingFrame::Notification
    }
}

pub(super) fn write_msg(writer: &mut impl Write, msg: &Value) -> Result<(), String> {
    let body = serde_json::to_vec(msg).map_err(|e| e.to_string())?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len()).map_err(|e| e.to_string())?;
    writer.write_all(&body).map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())
}

pub(super) fn write_null_response(writer: &mut impl Write, id: &Value) -> Result<(), String> {
    let reply = json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": Value::Null,
    });
    write_msg(writer, &reply)
}

pub(super) fn read_msg(stdout: &mut impl BufRead) -> Result<Value, String> {
    let mut content_len = None;
    let mut header_count = 0usize;
    loop {
        let mut header = String::new();
        let read = stdout
            .take((MAX_LSP_HEADER_LINE_BYTES + 1) as u64)
            .read_line(&mut header)
            .map_err(|e| e.to_string())?;
        if read == 0 {
            return Err("LSP EOF".into());
        }
        if read > MAX_LSP_HEADER_LINE_BYTES {
            return Err("LSP header line exceeds limit".into());
        }
        let trimmed = header.trim();
        if trimmed.is_empty() {
            break;
        }
        header_count += 1;
        if header_count > MAX_LSP_HEADERS {
            return Err("LSP header count exceeds limit".into());
        }
        if let Some((name, value)) = trimmed.split_once(':')
            && name.trim().eq_ignore_ascii_case("content-length")
        {
            content_len = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| "invalid Content-Length".to_owned())?,
            );
        }
    }
    let len = content_len.ok_or("missing Content-Length")?;
    if len > MAX_LSP_MESSAGE_BYTES {
        return Err(format!("LSP message exceeds limit ({len} bytes)"));
    }
    let mut buf = vec![0u8; len];
    stdout.read_exact(&mut buf).map_err(|e| e.to_string())?;
    serde_json::from_slice(&buf).map_err(|e| e.to_string())
}

/// Restricts a language ID or extension to safe ASCII path characters.
pub(super) fn sanitize_path_token(raw: &str, fallback: &str) -> String {
    let clean: String = raw
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
        .take(32)
        .collect();
    if clean.is_empty() {
        fallback.to_owned()
    } else {
        clean
    }
}

/// Creates a private, unpredictable workspace directory under `base_dir` (0700 on Unix).
pub(super) fn create_private_workspace_in(
    base_dir: &Path,
    language_id: &str,
) -> Result<PathBuf, String> {
    let lang = sanitize_path_token(language_id, "lang");
    let pid = std::process::id();
    for _ in 0..WORKSPACE_ATTEMPTS {
        let seq = NEXT_LSP_SESSION.fetch_add(1, Ordering::Relaxed);
        let token = random_u64(seq);
        let candidate = base_dir.join(format!("lotype-lsp-{lang}-{pid}-{token:016x}"));
        let mut builder = DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        match builder.create(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(err) if err.kind() == ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err.to_string()),
        }
    }
    Err("failed to allocate private LSP workspace directory".into())
}

pub(super) fn create_private_workspace(language_id: &str) -> Result<PathBuf, String> {
    create_private_workspace_in(&std::env::temp_dir(), language_id)
}

/// Writes `content` to `path` with owner-only (`0600`) permissions on Unix.
pub(super) fn write_private_file(path: &Path, content: &str) -> Result<(), String> {
    let mut opts = OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(path).map_err(|e| e.to_string())?;
    file.write_all(content.as_bytes())
        .map_err(|e| e.to_string())
}

/// Converts a filesystem path into an RFC 8089 `file://` URI with percent-encoding.
pub(super) fn path_to_file_uri(path: &Path) -> String {
    #[cfg(windows)]
    {
        format_windows_file_uri(&path.to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        format_unix_file_uri(path.as_os_str().as_encoded_bytes())
    }
}

#[cfg_attr(windows, allow(dead_code))]
pub(super) fn format_unix_file_uri(bytes: &[u8]) -> String {
    let mut uri = String::from("file://");
    if !bytes.starts_with(b"/") {
        uri.push('/');
    }
    encode_uri_path_bytes(&mut uri, bytes);
    uri
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(super) fn format_windows_file_uri(raw: &str) -> String {
    if let Some(unc) = raw.strip_prefix(r"\\?\UNC\") {
        return format_unc_file_uri(unc);
    }
    let stripped = raw.strip_prefix(r"\\?\").unwrap_or(raw);
    if let Some(unc) = stripped
        .strip_prefix(r"\\")
        .or_else(|| stripped.strip_prefix("//"))
    {
        return format_unc_file_uri(unc);
    }
    let normalized = stripped.replace('\\', "/");
    let mut uri = String::from("file://");
    if !normalized.starts_with('/') {
        uri.push('/');
    }
    let bytes = normalized.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        uri.push(bytes[0] as char);
        uri.push(':');
        encode_uri_path_bytes(&mut uri, &bytes[2..]);
    } else {
        encode_uri_path_bytes(&mut uri, bytes);
    }
    uri
}

#[cfg_attr(not(windows), allow(dead_code))]
fn format_unc_file_uri(unc: &str) -> String {
    let normalized = unc.replace('\\', "/");
    let trimmed = normalized.trim_start_matches('/');
    let mut uri = String::from("file://");
    match trimmed.split_once('/') {
        Some((host, rest)) => {
            encode_uri_path_bytes(&mut uri, host.as_bytes());
            uri.push('/');
            encode_uri_path_bytes(&mut uri, rest.as_bytes());
        }
        None => encode_uri_path_bytes(&mut uri, trimmed.as_bytes()),
    }
    uri
}

fn encode_uri_path_bytes(out: &mut String, bytes: &[u8]) {
    use std::fmt::Write as _;
    for &b in bytes {
        if b == b'/' || b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
}

/// Extracts `(signature, docs)` from an LSP `textDocument/hover` result,
/// splitting on `---` and keeping only the first fenced code block as the signature.
pub(super) fn extract_hover(result: &Value) -> Option<(String, Option<String>)> {
    let contents = result.get("contents")?;
    let raw = if let Some(val) = contents.get("value").and_then(Value::as_str) {
        val.to_owned()
    } else if let Some(s) = contents.as_str() {
        s.to_owned()
    } else {
        contents
            .as_array()?
            .iter()
            .filter_map(|v| {
                v.get("value")
                    .and_then(Value::as_str)
                    .or_else(|| v.as_str())
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    };

    let mut head_lines = Vec::new();
    let mut tail_lines = Vec::new();
    let mut saw_rule = false;
    for line in raw.lines() {
        if !saw_rule && matches!(line.trim(), "---" | "***") {
            saw_rule = true;
            continue;
        }
        if saw_rule {
            tail_lines.push(line);
        } else {
            head_lines.push(line);
        }
    }

    let has_fence = head_lines
        .iter()
        .any(|line| line.trim_start().starts_with("```"));
    let mut code_lines = Vec::new();
    let mut doc_lines = Vec::new();

    if has_fence {
        let mut in_first_fence = false;
        let mut captured_first_fence = false;
        for line in head_lines {
            if line.trim_start().starts_with("```") {
                if !captured_first_fence {
                    if in_first_fence {
                        in_first_fence = false;
                        captured_first_fence = true;
                    } else {
                        in_first_fence = true;
                    }
                }
                continue;
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if in_first_fence {
                code_lines.push(trimmed);
            } else if captured_first_fence {
                doc_lines.push(trimmed);
            }
        }
    } else if saw_rule {
        for line in head_lines {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                code_lines.push(trimmed);
            }
        }
    } else {
        let mut in_docs = false;
        for line in head_lines {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                if !code_lines.is_empty() {
                    in_docs = true;
                }
                continue;
            }
            if in_docs {
                doc_lines.push(trimmed);
            } else {
                code_lines.push(trimmed);
            }
        }
    }

    for line in tail_lines {
        if line.trim_start().starts_with("```") {
            continue;
        }
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            doc_lines.push(trimmed);
        }
    }

    let sig = code_lines.join(" ");
    (!sig.is_empty()).then(|| {
        let docs = (!doc_lines.is_empty()).then(|| doc_lines.join(" "));
        (sig, docs)
    })
}

fn random_u64(seq: u64) -> u64 {
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u64(seq);
    hasher.write_u32(std::process::id());
    if let Ok(elapsed) = SystemTime::now().duration_since(UNIX_EPOCH) {
        hasher.write_u128(elapsed.as_nanos());
    }
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn read_msg_rejects_oversized_content_length_and_headers_without_allocating() {
        let huge = b"Content-Length: 1099511627776\r\n\r\n";
        let err = read_msg(&mut Cursor::new(&huge[..])).unwrap_err();
        assert!(err.contains("exceeds limit"), "{err}");

        let long_header = format!(
            "X-Pad: {}\r\n\r\n",
            "a".repeat(MAX_LSP_HEADER_LINE_BYTES + 16)
        );
        let err = read_msg(&mut Cursor::new(long_header.as_bytes())).unwrap_err();
        assert!(err.contains("header line exceeds limit"), "{err}");

        let mut many_headers = String::new();
        for i in 0..=MAX_LSP_HEADERS {
            many_headers.push_str(&format!("X-H-{i}: 1\r\n"));
        }
        many_headers.push_str("Content-Length: 2\r\n\r\n{}");
        let err = read_msg(&mut Cursor::new(many_headers.as_bytes())).unwrap_err();
        assert!(err.contains("header count exceeds limit"), "{err}");

        let lowercase = b"content-length: 11\r\n\r\n{\"id\":1,\"x\":2}";
        let parsed = read_msg(&mut Cursor::new(
            &b"content-length: 14\r\n\r\n{\"id\":1,\"x\":2}"[..],
        ))
        .expect("case-insensitive Content-Length");
        assert_eq!(parsed["id"], 1);
        assert!(read_msg(&mut Cursor::new(&lowercase[..])).is_err());
    }

    #[test]
    fn classify_frame_distinguishes_server_requests_from_responses() {
        let srv_req = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "window/workDoneProgress/create",
            "params": { "token": "1" }
        });
        assert_eq!(
            classify_frame(srv_req),
            IncomingFrame::ServerRequest { id: json!(2) }
        );

        let resp = json!({ "jsonrpc": "2.0", "id": 2, "result": { "contents": "let x: i32" } });
        assert!(matches!(classify_frame(resp), IncomingFrame::Response(_)));

        let notif = json!({ "jsonrpc": "2.0", "method": "textDocument/publishDiagnostics" });
        assert_eq!(classify_frame(notif), IncomingFrame::Notification);
    }

    #[test]
    fn workspace_path_sanitizes_tokens_and_uses_owner_only_permissions() {
        assert_eq!(sanitize_path_token("pl/sql/../x", "lang"), "plsqlx");
        assert_eq!(sanitize_path_token("../../../", "txt"), "txt");

        let dir = create_private_workspace("pl/sql/../rust").expect("private workspace");
        assert!(dir.is_dir());
        let file = dir.join(format!("snippet.{}", sanitize_path_token("../rs", "txt")));
        write_private_file(&file, "let x = 1;").expect("write snippet");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let dir_mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
            let file_mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
            assert_eq!(dir_mode, 0o700);
            assert_eq!(file_mode, 0o600);
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_uri_encodes_spaces_special_chars_utf8_and_windows_paths() {
        assert_eq!(
            format_unix_file_uri("/tmp/my dir/foo#1%20bar/日本語.ts".as_bytes()),
            "file:///tmp/my%20dir/foo%231%2520bar/%E6%97%A5%E6%9C%AC%E8%AA%9E.ts"
        );
        assert_eq!(
            format_windows_file_uri(r"C:\Users\Alice Smith\dir #1\snippet.rs"),
            "file:///C:/Users/Alice%20Smith/dir%20%231/snippet.rs"
        );
        assert_eq!(
            format_windows_file_uri(r"\\?\C:\tmp\a b.ts"),
            "file:///C:/tmp/a%20b.ts"
        );
        assert_eq!(
            format_windows_file_uri(r"\\srv\share\a b.ts"),
            "file://srv/share/a%20b.ts"
        );
    }

    #[test]
    fn extract_hover_splits_on_hr_takes_first_fence_and_keeps_plaintext_signatures() {
        let md_with_example = json!({
            "contents": {
                "kind": "markdown",
                "value": "```ts\nfunction add(a: number, b: number): number\n```\n\nAdds two numbers.\n\n```ts\nadd(1, 2)\n```"
            }
        });
        let (sig, docs) = extract_hover(&md_with_example).expect("markdown hover");
        assert_eq!(sig, "function add(a: number, b: number): number");
        assert_eq!(docs.as_deref(), Some("Adds two numbers. add(1, 2)"));

        let ra_hover = json!({
            "contents": {
                "kind": "markdown",
                "value": "```rust\npub fn len(&self) -> usize\n```\n---\nReturns the number of elements.\n```rust\nassert_eq!(v.len(), 0);\n```"
            }
        });
        let (sig, docs) = extract_hover(&ra_hover).expect("rust-analyzer hover");
        assert_eq!(sig, "pub fn len(&self) -> usize");
        assert_eq!(
            docs.as_deref(),
            Some("Returns the number of elements. assert_eq!(v.len(), 0);")
        );

        let plain_hr = json!({
            "contents": {
                "kind": "plaintext",
                "value": "fn add(\n    a: i32,\n    b: i32,\n) -> i32\n---\nAdds two integers."
            }
        });
        let (sig, docs) = extract_hover(&plain_hr).expect("plaintext hr hover");
        assert_eq!(sig, "fn add( a: i32, b: i32, ) -> i32");
        assert_eq!(docs.as_deref(), Some("Adds two integers."));

        let plain_para = json!({
            "contents": "fn sub(\n    a: i32,\n) -> i32\n\nSubtracts."
        });
        let (sig, docs) = extract_hover(&plain_para).expect("plaintext paragraph hover");
        assert_eq!(sig, "fn sub( a: i32, ) -> i32");
        assert_eq!(docs.as_deref(), Some("Subtracts."));
    }
}
