use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::model_registry::{InferenceProvider, resolve_provider_model_id};

// Both Ollama (>= 0.5) and LM Studio expose `/v1/chat/completions` with the
// OpenAI request/response shape. We talk to localhost only — no TLS path,
// hence no need for a heavyweight HTTP client.
const CHAT_COMPLETIONS_PATH: &str = "/v1/chat/completions";
const DEFAULT_TIMEOUT_SECS: u64 = 120;
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    pub stream: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatChoice {
    pub index: Option<u32>,
    pub message: ChatMessage,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatUsage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatResponse {
    pub id: Option<String>,
    pub model: Option<String>,
    pub choices: Vec<ChatChoice>,
    pub usage: Option<ChatUsage>,
}

impl ChatResponse {
    pub fn first_text(&self) -> Option<&str> {
        self.choices
            .first()
            .map(|choice| choice.message.content.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct ChatCompletionOptions {
    pub provider: InferenceProvider,
    pub endpoint: String,
    pub model_key_or_id: String,
    pub messages: Vec<ChatMessage>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub top_p: Option<f32>,
    pub timeout: Duration,
}

impl ChatCompletionOptions {
    pub fn new(
        provider: InferenceProvider,
        model_key_or_id: impl Into<String>,
        messages: Vec<ChatMessage>,
    ) -> Self {
        Self {
            provider,
            endpoint: provider.default_endpoint().to_string(),
            model_key_or_id: model_key_or_id.into(),
            messages,
            temperature: Some(0.0),
            max_tokens: None,
            top_p: None,
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
        }
    }
}

pub fn complete_chat(options: &ChatCompletionOptions) -> Result<ChatResponse> {
    if options.messages.is_empty() {
        bail!("chat completion requires at least one message");
    }

    let model_id = resolve_provider_model_id(options.provider, &options.model_key_or_id)
        .map(|id| id.to_string())
        .unwrap_or_else(|| options.model_key_or_id.clone());

    let request = ChatRequest {
        model: model_id,
        messages: options.messages.clone(),
        temperature: options.temperature,
        max_tokens: options.max_tokens,
        top_p: options.top_p,
        stream: false,
    };

    let body = serde_json::to_string(&request).context("failed to serialize chat request")?;

    let endpoint = ParsedEndpoint::parse(&options.endpoint)
        .with_context(|| format!("failed to parse endpoint `{}`", options.endpoint))?;

    let response = http_post_json(&endpoint, CHAT_COMPLETIONS_PATH, &body, options.timeout)
        .with_context(|| {
            format!(
                "failed to POST to {} ({} provider)",
                options.endpoint,
                options.provider.name()
            )
        })?;

    if !(200..300).contains(&response.status) {
        bail!(
            "chat completion call returned HTTP {}: {}",
            response.status,
            String::from_utf8_lossy(&response.body)
        );
    }

    serde_json::from_slice::<ChatResponse>(&response.body).with_context(|| {
        format!(
            "failed to parse chat completion response: {}",
            String::from_utf8_lossy(&response.body)
        )
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedEndpoint {
    pub host: String,
    pub port: u16,
    pub host_header: String,
}

impl ParsedEndpoint {
    pub fn parse(raw: &str) -> Result<Self> {
        let trimmed = raw.trim().trim_end_matches('/');
        let after_scheme = trimmed
            .strip_prefix("http://")
            .ok_or_else(|| anyhow!("only http:// endpoints are supported, got `{raw}`"))?;

        if after_scheme.is_empty() {
            bail!("endpoint host is empty");
        }

        // Strip path component if present.
        let authority = after_scheme.split('/').next().unwrap_or("");
        if authority.is_empty() {
            bail!("endpoint authority is empty");
        }

        let (host, port) = if let Some((host, port_str)) = authority.rsplit_once(':') {
            let port: u16 = port_str
                .parse()
                .with_context(|| format!("invalid port `{port_str}` in `{raw}`"))?;
            (host.to_string(), port)
        } else {
            (authority.to_string(), 80)
        };

        if host.is_empty() {
            bail!("endpoint host is empty");
        }

        Ok(Self {
            host_header: format!("{host}:{port}"),
            host,
            port,
        })
    }
}

#[derive(Debug)]
pub(crate) struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

pub(crate) fn http_post_json(
    endpoint: &ParsedEndpoint,
    path: &str,
    body: &str,
    timeout: Duration,
) -> Result<HttpResponse> {
    let address = (endpoint.host.as_str(), endpoint.port)
        .to_socket_addrs()
        .with_context(|| format!("failed to resolve {}:{}", endpoint.host, endpoint.port))?
        .next()
        .ok_or_else(|| {
            anyhow!(
                "no addresses resolved for {}:{}",
                endpoint.host,
                endpoint.port
            )
        })?;

    let mut stream = TcpStream::connect_timeout(&address, timeout)
        .with_context(|| format!("failed to connect to {address}"))?;
    stream
        .set_read_timeout(Some(timeout))
        .context("failed to set read timeout")?;
    stream
        .set_write_timeout(Some(timeout))
        .context("failed to set write timeout")?;

    let request = format!(
        "POST {path} HTTP/1.1\r\n\
         Host: {host}\r\n\
         User-Agent: mutagen-harness/0.1\r\n\
         Accept: application/json\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {len}\r\n\
         Connection: close\r\n\
         \r\n",
        host = endpoint.host_header,
        len = body.len(),
    );

    stream
        .write_all(request.as_bytes())
        .context("failed to write HTTP request headers")?;
    stream
        .write_all(body.as_bytes())
        .context("failed to write HTTP request body")?;
    stream.flush().ok();

    let mut raw = Vec::with_capacity(8 * 1024);
    let mut buf = [0u8; 8 * 1024];
    loop {
        let read = stream
            .read(&mut buf)
            .context("failed to read HTTP response")?;
        if read == 0 {
            break;
        }
        if raw.len() + read > MAX_RESPONSE_BYTES {
            bail!(
                "HTTP response exceeded {} bytes — refusing to buffer further",
                MAX_RESPONSE_BYTES
            );
        }
        raw.extend_from_slice(&buf[..read]);
    }

    parse_http_response(&raw)
}

pub(crate) fn parse_http_response(raw: &[u8]) -> Result<HttpResponse> {
    let split = find_header_terminator(raw)
        .ok_or_else(|| anyhow!("malformed HTTP response: no header terminator"))?;

    let header_bytes = &raw[..split];
    let body_bytes = &raw[split + 4..];

    let header_text =
        std::str::from_utf8(header_bytes).context("HTTP response headers are not valid UTF-8")?;
    let mut lines = header_text.split("\r\n");
    let status_line = lines
        .next()
        .ok_or_else(|| anyhow!("missing HTTP status line"))?;

    let status = parse_status_code(status_line)?;

    let mut transfer_encoding: Option<String> = None;
    let mut content_length: Option<usize> = None;
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            let name_lower = name.trim().to_ascii_lowercase();
            let value = value.trim();
            match name_lower.as_str() {
                "transfer-encoding" => transfer_encoding = Some(value.to_ascii_lowercase()),
                "content-length" => {
                    content_length = Some(
                        value
                            .parse::<usize>()
                            .with_context(|| format!("invalid Content-Length `{value}`"))?,
                    );
                }
                _ => {}
            }
        }
    }

    let body = if matches!(transfer_encoding.as_deref(), Some(te) if te.contains("chunked")) {
        decode_chunked(body_bytes)?
    } else if let Some(len) = content_length {
        if body_bytes.len() < len {
            bail!(
                "HTTP body truncated: expected {} bytes, got {}",
                len,
                body_bytes.len()
            );
        }
        body_bytes[..len].to_vec()
    } else {
        // Connection: close framing — read everything we got.
        body_bytes.to_vec()
    };

    Ok(HttpResponse { status, body })
}

fn find_header_terminator(raw: &[u8]) -> Option<usize> {
    raw.windows(4).position(|window| window == b"\r\n\r\n")
}

fn parse_status_code(status_line: &str) -> Result<u16> {
    // e.g. "HTTP/1.1 200 OK"
    let mut parts = status_line.splitn(3, ' ');
    let _version = parts
        .next()
        .ok_or_else(|| anyhow!("missing HTTP version in `{status_line}`"))?;
    let code = parts
        .next()
        .ok_or_else(|| anyhow!("missing HTTP status code in `{status_line}`"))?;
    code.parse::<u16>()
        .with_context(|| format!("invalid HTTP status code `{code}`"))
}

fn decode_chunked(body: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(body.len());
    let mut idx = 0usize;

    loop {
        let line_end = body[idx..]
            .windows(2)
            .position(|window| window == b"\r\n")
            .ok_or_else(|| anyhow!("malformed chunked body: missing chunk size terminator"))?;
        let size_line = std::str::from_utf8(&body[idx..idx + line_end])
            .context("chunk size line is not valid UTF-8")?;
        // strip optional chunk-extensions (`size; ext=val`)
        let size_str = size_line.split(';').next().unwrap_or("").trim();
        let chunk_size = usize::from_str_radix(size_str, 16)
            .with_context(|| format!("invalid chunk size `{size_str}`"))?;

        idx += line_end + 2;

        if chunk_size == 0 {
            break;
        }

        if idx + chunk_size > body.len() {
            bail!("malformed chunked body: chunk runs past buffer");
        }

        out.extend_from_slice(&body[idx..idx + chunk_size]);
        idx += chunk_size;

        if body.get(idx..idx + 2) != Some(b"\r\n") {
            bail!("malformed chunked body: missing CRLF after chunk data");
        }
        idx += 2;
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_endpoint_with_explicit_port() {
        let parsed = ParsedEndpoint::parse("http://127.0.0.1:11434").unwrap();
        assert_eq!(parsed.host, "127.0.0.1");
        assert_eq!(parsed.port, 11434);
        assert_eq!(parsed.host_header, "127.0.0.1:11434");
    }

    #[test]
    fn parses_endpoint_with_trailing_slash_and_path() {
        let parsed = ParsedEndpoint::parse("http://localhost:1234/v1").unwrap();
        assert_eq!(parsed.host, "localhost");
        assert_eq!(parsed.port, 1234);
    }

    #[test]
    fn parses_endpoint_without_explicit_port() {
        let parsed = ParsedEndpoint::parse("http://example.test").unwrap();
        assert_eq!(parsed.port, 80);
    }

    #[test]
    fn rejects_https_endpoints() {
        let err = ParsedEndpoint::parse("https://localhost:1234").unwrap_err();
        assert!(err.to_string().contains("only http://"));
    }

    #[test]
    fn parses_simple_content_length_response() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 14\r\n\r\n{\"hello\":true}";
        let response = parse_http_response(raw).unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"{\"hello\":true}");
    }

    #[test]
    fn parses_chunked_response() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n";
        let response = parse_http_response(raw).unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"hello world");
    }

    #[test]
    fn parses_connection_close_response_without_content_length() {
        // Some servers (notably under HTTP/1.0) just close the connection.
        let raw = b"HTTP/1.1 200 OK\r\n\r\nbody-bytes";
        let response = parse_http_response(raw).unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"body-bytes");
    }

    #[test]
    fn rejects_malformed_response_without_header_terminator() {
        let raw = b"HTTP/1.1 200 OK\nContent-Length: 0";
        let err = parse_http_response(raw).unwrap_err();
        assert!(err.to_string().contains("header terminator"));
    }

    #[test]
    fn surfaces_non_2xx_status() {
        let raw = b"HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\n\r\nnot here.";
        let response = parse_http_response(raw).unwrap();
        assert_eq!(response.status, 404);
    }

    #[test]
    fn chat_message_helpers_set_role() {
        assert_eq!(ChatMessage::user("hi").role, "user");
        assert_eq!(ChatMessage::system("rules").role, "system");
    }

    #[test]
    fn chat_response_first_text_returns_initial_choice() {
        let json = br#"{
            "id": "x",
            "model": "qwen",
            "choices": [
                {"index": 0, "message": {"role": "assistant", "content": "hello"}, "finish_reason": "stop"}
            ]
        }"#;
        let response: ChatResponse = serde_json::from_slice(json).unwrap();
        assert_eq!(response.first_text(), Some("hello"));
    }
}
