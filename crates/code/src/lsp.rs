//! Language Server Protocol (`stdio` JSON-RPC 2.0) bridge implementing [`CodeAnalyzer`].
//!
//! Spawns configured language servers (such as `typescript-language-server --stdio`,
//! `vtsls --stdio`, `deno lsp`, or `rust-analyzer`) on a background thread and merges
//! their hover/completion results with [`BuiltInAnalyzer`] fallback.

mod protocol;

use std::collections::HashMap;
use std::future::Future;
use std::io::BufReader;
use std::path::PathBuf;
use std::pin::Pin;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use serde_json::{Value, json};

use crate::analyzer::{
    AnalysisFuture, AnalysisRequest, CodeAnalysis, CodeAnalyzer, CompletionItem, CompletionKind,
    HoverAnnotation,
};
use crate::infer::BuiltInAnalyzer;
use crate::languages::canonical_language;
use protocol::{
    IncomingFrame, classify_frame, create_private_workspace, read_msg, sanitize_path_token,
    write_msg, write_null_response, write_private_file,
};

const LSP_RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);

/// Command-line configuration for launching a stdio LSP server for a language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspServerConfig {
    /// Executable name or path (e.g. `"typescript-language-server"` or `"rust-analyzer"`).
    pub program: String,
    /// Arguments passed to the executable (e.g. `["--stdio"]`).
    pub args: Vec<String>,
    /// File extension used for temporary virtual documents (e.g. `"ts"` or `"rs"`).
    pub extension: String,
    /// LSP `languageId` string (e.g. `"typescript"` or `"rust"`).
    pub language_id: String,
}

impl LspServerConfig {
    /// Creates a new stdio LSP server configuration.
    pub fn new(
        program: impl Into<String>,
        args: impl IntoIterator<Item = impl Into<String>>,
        extension: impl Into<String>,
        language_id: impl Into<String>,
    ) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
            extension: extension.into(),
            language_id: language_id.into(),
        }
    }

    /// Standard `typescript-language-server --stdio` configuration.
    pub fn typescript_stdio() -> Self {
        Self::new(
            "typescript-language-server",
            ["--stdio"],
            "ts",
            "typescript",
        )
    }

    /// Standard `deno lsp` configuration for TypeScript/JavaScript snippets.
    pub fn deno_lsp() -> Self {
        Self::new("deno", ["lsp"], "ts", "typescript")
    }

    /// Standard `rust-analyzer` configuration.
    pub fn rust_analyzer() -> Self {
        Self::new("rust-analyzer", Vec::<String>::new(), "rs", "rust")
    }
}

type ReplySlot = Arc<Mutex<(Option<Result<CodeAnalysis, String>>, Option<Waker>)>>;

struct ReplyFuture {
    slot: ReplySlot,
}

impl Future for ReplyFuture {
    type Output = Result<CodeAnalysis, String>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut guard = self
            .slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(res) = guard.0.take() {
            Poll::Ready(res)
        } else {
            guard.1 = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

/// Multi-language stdio LSP [`CodeAnalyzer`] with [`BuiltInAnalyzer`] fallback.
pub struct LspAnalyzer {
    configs: HashMap<String, LspServerConfig>,
    workers: Mutex<HashMap<String, mpsc::SyncSender<(AnalysisRequest, ReplySlot)>>>,
}

impl Default for LspAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl LspAnalyzer {
    /// Creates an empty LSP analyzer with [`BuiltInAnalyzer`] fallback.
    pub fn new() -> Self {
        Self {
            configs: HashMap::new(),
            workers: Mutex::new(HashMap::new()),
        }
    }

    /// Registers an [`LspServerConfig`] for a canonical language name (such as `"typescript"` or `"rust"`).
    pub fn with_server(mut self, language: &str, config: LspServerConfig) -> Self {
        self.configs.insert(canonical_language(language), config);
        self
    }
}

impl CodeAnalyzer for LspAnalyzer {
    fn analyze(&self, request: AnalysisRequest) -> AnalysisFuture {
        let canonical = request
            .language
            .as_deref()
            .map(canonical_language)
            .unwrap_or_else(|| "text".to_owned());
        let Some(config) = self.configs.get(&canonical).cloned() else {
            return BuiltInAnalyzer.analyze(request);
        };

        let sender = {
            let mut guard = self
                .workers
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(tx) = guard.get(&canonical) {
                tx.clone()
            } else {
                let (tx, rx) = mpsc::sync_channel::<(AnalysisRequest, ReplySlot)>(1);
                let cfg = config;
                std::thread::spawn(move || run_lsp_worker(cfg, rx));
                guard.insert(canonical, tx.clone());
                tx
            }
        };

        let slot: ReplySlot = Arc::new(Mutex::new((None, None)));
        match sender.try_send((request.clone(), slot.clone())) {
            Ok(()) => {}
            Err(mpsc::TrySendError::Full(_) | mpsc::TrySendError::Disconnected(_)) => {
                return BuiltInAnalyzer.analyze(request);
            }
        }
        Box::pin(ReplyFuture { slot })
    }
}

struct LspSession {
    child: Child,
    stdin: Arc<Mutex<ChildStdin>>,
    responses: mpsc::Receiver<Result<Value, String>>,
    workspace: PathBuf,
    next_id: i64,
    doc_version: i64,
    opened: bool,
    config: LspServerConfig,
}

impl Drop for LspSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.workspace);
    }
}

fn run_lsp_worker(config: LspServerConfig, rx: mpsc::Receiver<(AnalysisRequest, ReplySlot)>) {
    let mut session: Option<LspSession> = None;
    while let Ok((request, slot)) = rx.recv() {
        let mut analysis = BuiltInAnalyzer::analyze_sync(&request);
        if session.is_none() {
            session = start_session(&config).ok();
        }
        let lsp_result = session.as_mut().map(|sess| query_session(sess, &request));
        if let Some(Ok(lsp_out)) = lsp_result.as_ref() {
            for (range, sig) in &lsp_out.queries {
                if let Some(existing) = analysis.queries.iter_mut().find(|(r, _)| *r == *range) {
                    existing.1 = sig.clone();
                } else {
                    analysis.queries.push((range.clone(), sig.clone()));
                }
            }
            for (offset, items) in &lsp_out.completions {
                if !items.is_empty() {
                    analysis.completions.retain(|(o, _)| o != offset);
                    analysis.completions.push((*offset, items.clone()));
                }
            }
            for hover in &lsp_out.hovers {
                analysis.hovers.retain(|h| h.range != hover.range);
                analysis.hovers.push(hover.clone());
            }
        } else if lsp_result.is_some() {
            session = None;
        }
        let mut guard = slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.0 = Some(Ok(analysis));
        if let Some(waker) = guard.1.take() {
            waker.wake();
        }
    }
}

fn start_session(config: &LspServerConfig) -> Result<LspSession, String> {
    let workspace = create_private_workspace(&config.language_id)?;
    let mut child = match Command::new(&config.program)
        .args(&config.args)
        .current_dir(&workspace)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(err) => {
            let _ = std::fs::remove_dir_all(&workspace);
            return Err(err.to_string());
        }
    };
    let stdin = Arc::new(Mutex::new(child.stdin.take().ok_or("missing stdin")?));
    let stdout = child.stdout.take().ok_or("missing stdout")?;
    let (response_tx, responses) = mpsc::channel();
    let reply_stdin = stdin.clone();
    std::thread::spawn(move || {
        let mut stdout = BufReader::new(stdout);
        loop {
            match read_msg(&mut stdout) {
                Ok(message) => match classify_frame(message) {
                    IncomingFrame::Response(resp) => {
                        if response_tx.send(Ok(resp)).is_err() {
                            break;
                        }
                    }
                    IncomingFrame::ServerRequest { id } => {
                        let mut guard = reply_stdin
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        let _ = write_null_response(&mut *guard, &id);
                    }
                    IncomingFrame::Notification => {}
                },
                Err(error) => {
                    let _ = response_tx.send(Err(error));
                    break;
                }
            }
        }
    });
    let mut sess = LspSession {
        child,
        stdin,
        responses,
        workspace,
        next_id: 1,
        doc_version: 0,
        opened: false,
        config: config.clone(),
    };
    let root_uri = format!("file://{}", sess.workspace.display());
    let _ = sess.request(
        "initialize",
        json!({
            "processId": std::process::id(),
            "rootUri": root_uri,
            "capabilities": {
                "textDocument": {
                    "hover": { "contentFormat": ["plaintext", "markdown"] },
                    "completion": { "completionItem": { "snippetSupport": false } }
                }
            }
        }),
    )?;
    sess.notify("initialized", json!({}))?;
    Ok(sess)
}

fn query_session(sess: &mut LspSession, req: &AnalysisRequest) -> Result<CodeAnalysis, String> {
    let ext = sanitize_path_token(&sess.config.extension, "txt");
    let filename = format!("snippet.{ext}");
    let path = sess.workspace.join(&filename);
    write_private_file(&path, &req.code)?;
    let uri = format!("file://{}", path.display());
    sess.doc_version += 1;
    if !sess.opened {
        sess.notify(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": sess.config.language_id,
                    "version": sess.doc_version,
                    "text": req.code,
                }
            }),
        )?;
        sess.opened = true;
    } else {
        sess.notify(
            "textDocument/didChange",
            json!({
                "textDocument": { "uri": uri, "version": sess.doc_version },
                "contentChanges": [{ "text": req.code }]
            }),
        )?;
    }

    let mut out = CodeAnalysis::default();
    for query in &req.queries {
        let resp = sess.request(
            "textDocument/hover",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": query.line, "character": lsp_character(&req.code, query.line, query.character) }
            }),
        )?;
        if let Some((sig, docs)) = extract_hover(&resp) {
            out.queries.push((query.target_range.clone(), sig.clone()));
            out.hovers.push(HoverAnnotation {
                range: query.target_range.clone(),
                symbol: query.symbol.clone(),
                signature: sig,
                docs,
            });
        }
    }

    for comp in &req.completions {
        let resp = sess.request(
            "textDocument/completion",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": comp.line, "character": lsp_character(&req.code, comp.line, comp.character) }
            }),
        )?;
        let items = extract_completions(&resp, &comp.prefix);
        if !items.is_empty() {
            out.completions.push((comp.offset, items));
        }
    }
    Ok(out)
}

impl LspSession {
    fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        let msg = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        self.send(&msg)
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let msg = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        self.send(&msg)?;
        for _ in 0..64 {
            let incoming = self.responses.recv_timeout(LSP_RESPONSE_TIMEOUT).map_err(
                |error| match error {
                    mpsc::RecvTimeoutError::Timeout => "LSP response timed out".to_owned(),
                    mpsc::RecvTimeoutError::Disconnected => "LSP response stream closed".to_owned(),
                },
            )??;
            if incoming.get("method").is_none()
                && incoming.get("id").and_then(Value::as_i64) == Some(id)
            {
                if let Some(error) = incoming.get("error") {
                    return Err(format!("LSP request failed: {error}"));
                }
                return Ok(incoming.get("result").cloned().unwrap_or(Value::Null));
            }
        }
        Err("LSP response limit exceeded".into())
    }

    fn send(&mut self, msg: &Value) -> Result<(), String> {
        let mut guard = self
            .stdin
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        write_msg(&mut *guard, msg)
    }
}

fn lsp_character(code: &str, line: usize, character: usize) -> usize {
    code.split('\n')
        .nth(line)
        .unwrap_or_default()
        .chars()
        .take(character)
        .map(char::len_utf16)
        .sum()
}

fn extract_hover(result: &Value) -> Option<(String, Option<String>)> {
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
    // Strip markdown code fences from LSP hover text.
    let mut code_lines = Vec::new();
    let mut doc_lines = Vec::new();
    let mut in_fence = false;
    for line in raw.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if line.trim() == "---" {
            continue;
        }
        if in_fence || code_lines.is_empty() {
            if !line.trim().is_empty() {
                code_lines.push(line.trim());
            }
        } else if !line.trim().is_empty() {
            doc_lines.push(line.trim());
        }
    }
    let sig = code_lines.join(" ");
    (!sig.is_empty()).then(|| {
        let docs = (!doc_lines.is_empty()).then(|| doc_lines.join(" "));
        (sig, docs)
    })
}

fn extract_completions(result: &Value, prefix: &str) -> Vec<CompletionItem> {
    let arr = result
        .get("items")
        .and_then(Value::as_array)
        .or_else(|| result.as_array());
    let Some(arr) = arr else {
        return Vec::new();
    };
    let mut items: Vec<CompletionItem> = arr
        .iter()
        .filter_map(|item| {
            let label = item.get("label")?.as_str()?.to_owned();
            if !label.starts_with(prefix) {
                return None;
            }
            let kind = match item.get("kind").and_then(Value::as_u64).unwrap_or(0) {
                2 | 3 => CompletionKind::Function,
                5 | 10 => CompletionKind::Property,
                6 | 21 => CompletionKind::Variable,
                7 | 8 | 22 => CompletionKind::Type,
                14 => CompletionKind::Keyword,
                _ => CompletionKind::Property,
            };
            let detail = item
                .get("detail")
                .and_then(Value::as_str)
                .map(str::to_owned);
            Some(CompletionItem {
                label,
                kind,
                detail,
            })
        })
        .collect();
    items.truncate(8);
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lsp_character_counts_utf16_code_units() {
        assert_eq!(lsp_character("a😀b\nçx", 0, 2), 3);
        assert_eq!(lsp_character("a😀b\nçx", 1, 1), 1);
    }

    #[cfg(unix)]
    #[test]
    fn server_request_with_colliding_id_is_replied_and_does_not_corrupt_hover() {
        // Minimal stdio mock LSP server that sends a server-to-client request (`id: 2`)
        // and blocks waiting for our null reply before returning the real `id: 2` hover.
        let script = r#"
import json, sys

def read_msg():
    length = 0
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        s = line.decode('utf-8').strip()
        if not s:
            break
        if s.lower().startswith('content-length:'):
            length = int(s.split(':', 1)[1].strip())
    return json.loads(sys.stdin.buffer.read(length).decode('utf-8'))

def write_msg(obj):
    data = json.dumps(obj).encode('utf-8')
    sys.stdout.buffer.write(f"Content-Length: {len(data)}\r\n\r\n".encode('utf-8') + data)
    sys.stdout.buffer.flush()

while True:
    msg = read_msg()
    if msg is None:
        break
    method = msg.get('method')
    if method == 'initialize':
        write_msg({'jsonrpc': '2.0', 'id': msg['id'], 'result': {'capabilities': {}}})
    elif method == 'textDocument/hover':
        # Emit server-to-client request using the SAME id as the hover request and wait for reply.
        write_msg({'jsonrpc': '2.0', 'id': msg['id'], 'method': 'window/workDoneProgress/create', 'params': {'token': 't'}})
        reply = read_msg()
        if reply is None or reply.get('id') != msg['id'] or 'result' not in reply:
            sys.exit(2)
        write_msg({'jsonrpc': '2.0', 'id': msg['id'], 'result': {'contents': {'kind': 'markdown', 'value': '```ts\nconst answer: 42\n```'}}})
"#;
        let config = LspServerConfig::new("python3", ["-c", script], "ts", "typescript");
        let mut sess = start_session(&config).expect("mock LSP session starts");
        let req = AnalysisRequest {
            language: Some("typescript".into()),
            title: None,
            code: "const answer = 42;".into(),
            queries: vec![crate::QuerySite {
                target_range: 6..12,
                line: 0,
                character: 6,
                symbol: "answer".into(),
            }],
            completions: Vec::new(),
            files: Vec::new(),
            no_errors: false,
            expected_errors: Vec::new(),
        };
        let out = query_session(&mut sess, &req).expect("hover succeeds after server request");
        assert_eq!(out.queries, vec![(6..12, "const answer: 42".to_owned())]);
    }
}
