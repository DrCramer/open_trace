//! gzip on the wire, both directions.
//!
//! An analysed sample is ~1.2 MB of JSON (numbers as text); gzip makes it ~8 times smaller. Projects are already stored
//! gzipped on disk (`samples/<id>.json.gz`), so this only removes the network cost:
//!
//! * responses: compressed when the client sends `Accept-Encoding: gzip` (browsers always do, the desktop does) and the
//!   body is JSON, text, JavaScript, SVG or an FSA file of at least 1 KiB — except for a client on the server machine
//!   itself without a proxy in between (nothing to save, only CPU to spend); level 1 (fast);
//! * requests: a body with `Content-Encoding: gzip` is decompressed before the handlers see it (the desktop and the web
//!   UI send large saves this way once they have seen `X-OT-Accepts-Gzip` from this server). The decompressed size is
//!   capped (`max_decoded`), so a small compressed body cannot expand into an unbounded one; route limits apply on top.
use axum::body::{to_bytes, Body};
use axum::extract::{Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::io::{Read, Write};

#[derive(Clone, Copy)]
pub struct Limits {
    pub max_decoded: usize,
    pub max_response: usize,
}

pub const ADVERTISE: &str = "x-ot-accepts-gzip";
const MIN_SIZE: usize = 1024;

fn compressible(content_type: &str) -> bool {
    let t = content_type.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    t.starts_with("text/") || t == "application/json" || t == "application/javascript" || t == "image/svg+xml" || t == "application/octet-stream"
}

/// Level 1: about four times faster than level 5 on large analysis JSON, still 6-8x smaller than plain text.
pub fn gzip(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::GzEncoder::new(Vec::with_capacity(data.len() / 5 + 64), flate2::Compression::new(1));
    let _ = e.write_all(data);
    e.finish().unwrap_or_default()
}

/// Decompresses at most `max` bytes; `None` when the stream is corrupt or larger.
pub fn gunzip_limited(data: &[u8], max: usize) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut d = flate2::read::GzDecoder::new(data).take(max as u64 + 1);
    d.read_to_end(&mut out).ok()?;
    (out.len() <= max).then_some(out)
}

fn accepts_gzip(req: &Request) -> bool {
    req.headers().get_all(header::ACCEPT_ENCODING).iter().filter_map(|v| v.to_str().ok()).any(|v| {
        v.split(',').any(|p| {
            let mut it = p.trim().split(';');
            it.next().map(|e| e.trim().eq_ignore_ascii_case("gzip")).unwrap_or(false) && !it.any(|q| q.trim().replace(' ', "") == "q=0")
        })
    })
}

fn err(status: StatusCode, code: &str, msg: &str) -> Response {
    (status, axum::Json(serde_json::json!({"error": msg, "code": code}))).into_response()
}

/// A client on this machine talking to the server directly (no reverse proxy in between): compressing costs CPU on
/// both sides and saves nothing, so responses are sent as they are.
fn direct_local(req: &Request) -> bool {
    let loopback = req.extensions().get::<axum::extract::ConnectInfo<std::net::SocketAddr>>().map(|c| c.0.ip().is_loopback()).unwrap_or(false);
    loopback && !req.headers().contains_key("x-forwarded-for") && !req.headers().contains_key("forwarded")
}

pub async fn layer(State(lim): State<Limits>, req: Request, next: Next) -> Response {
    let want_gzip = accepts_gzip(&req) && !direct_local(&req);
    let req = match req.headers().get(header::CONTENT_ENCODING).and_then(|v| v.to_str().ok()).map(|s| s.trim().to_ascii_lowercase()) {
        None => req,
        Some(e) if e == "identity" => req,
        Some(e) if e == "gzip" => {
            let (mut parts, body) = req.into_parts();
            let Ok(raw) = to_bytes(body, lim.max_decoded).await else {
                return err(StatusCode::PAYLOAD_TOO_LARGE, "too_large", "compressed request body too large");
            };
            let max = lim.max_decoded;
            let Ok(Some(plain)) = tokio::task::spawn_blocking(move || gunzip_limited(&raw, max)).await else {
                return err(StatusCode::BAD_REQUEST, "bad_gzip", "request body is not valid gzip or expands beyond the limit");
            };
            parts.headers.remove(header::CONTENT_ENCODING);
            parts.headers.insert(header::CONTENT_LENGTH, HeaderValue::from(plain.len()));
            Request::from_parts(parts, Body::from(plain))
        }
        Some(_) => return err(StatusCode::UNSUPPORTED_MEDIA_TYPE, "unsupported_encoding", "only gzip request bodies are supported"),
    };
    let mut res = next.run(req).await;
    res.headers_mut().insert(ADVERTISE, HeaderValue::from_static("1"));
    let ct = res.headers().get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    if !want_gzip || res.headers().contains_key(header::CONTENT_ENCODING) || !compressible(&ct) || res.status() == StatusCode::NO_CONTENT {
        return res;
    }
    if let Some(n) = res.headers().get(header::CONTENT_LENGTH).and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<usize>().ok()) {
        if n < MIN_SIZE || n > lim.max_response {
            return res;
        }
    }
    let (mut parts, body) = res.into_parts();
    let Ok(bytes) = to_bytes(body, lim.max_response).await else {
        return err(StatusCode::INTERNAL_SERVER_ERROR, "too_large", "response too large to compress");
    };
    parts.headers.append(header::VARY, HeaderValue::from_static("Accept-Encoding"));
    if bytes.len() < MIN_SIZE {
        parts.headers.insert(header::CONTENT_LENGTH, HeaderValue::from(bytes.len()));
        return Response::from_parts(parts, Body::from(bytes));
    }
    let z = if bytes.len() > 256 * 1024 { tokio::task::spawn_blocking(move || gzip(&bytes)).await.unwrap_or_default() } else { gzip(&bytes) };
    parts.headers.insert(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
    parts.headers.insert(header::CONTENT_LENGTH, HeaderValue::from(z.len()));
    Response::from_parts(parts, Body::from(z))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_bomb_limit() {
        let data = br#"{"values":[1,2,3,4,5,6,7,8,9,10]}"#.repeat(1000);
        let z = gzip(&data);
        assert!(z.len() * 5 < data.len());
        assert_eq!(gunzip_limited(&z, data.len()).unwrap(), data);
        assert!(gunzip_limited(&z, data.len() - 1).is_none()); // expands beyond the cap
        assert!(gunzip_limited(b"not gzip", 100).is_none());
        let bomb = gzip(&vec![0u8; 50 * 1024 * 1024]);
        assert!(bomb.len() < 1024 * 1024);
        assert!(gunzip_limited(&bomb, 1024 * 1024).is_none());
    }

    #[test]
    fn types() {
        assert!(compressible("application/json; charset=utf-8"));
        assert!(compressible("text/html"));
        assert!(compressible("application/octet-stream"));
        assert!(!compressible("image/png"));
    }
}
