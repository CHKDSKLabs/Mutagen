use std::cell::Cell;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::extract::Request;
use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use tracing::Instrument;

pub const X_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

#[derive(Debug, Clone)]
pub struct RequestId(pub String);

/// Strict UUID-shape validator. Accepts the canonical 8-4-4-4-12 lowercase hex
/// form and additionally requires version nibble == 7 (DSD-623).
pub fn parse_uuid_v7_strict(s: &str) -> Option<String> {
    if s.len() != 36 {
        return None;
    }
    let bytes = s.as_bytes();
    let dashes = [8usize, 13, 18, 23];
    for (i, &b) in bytes.iter().enumerate() {
        let ok = if dashes.contains(&i) {
            b == b'-'
        } else {
            b.is_ascii_hexdigit() && !b.is_ascii_uppercase()
        };
        if !ok {
            return None;
        }
    }
    // version nibble lives at byte index 14 (0-indexed within string)
    if bytes[14] != b'7' {
        return None;
    }
    // variant nibble at index 19 must be 8, 9, a, or b
    if !matches!(bytes[19], b'8' | b'9' | b'a' | b'b') {
        return None;
    }
    Some(s.to_owned())
}

pub fn generate_uuid_v7() -> String {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let mut b = [0u8; 16];
    b[0] = (ms >> 40) as u8;
    b[1] = (ms >> 32) as u8;
    b[2] = (ms >> 24) as u8;
    b[3] = (ms >> 16) as u8;
    b[4] = (ms >> 8) as u8;
    b[5] = ms as u8;

    let r1 = next_random();
    let r2 = next_random();
    b[6] = ((r1 >> 56) as u8 & 0x0f) | 0x70; // version 7
    b[7] = (r1 >> 48) as u8;
    b[8] = ((r1 >> 40) as u8 & 0x3f) | 0x80; // variant
    b[9] = (r1 >> 32) as u8;
    b[10] = (r1 >> 24) as u8;
    b[11] = (r1 >> 16) as u8;
    b[12] = (r1 >> 8) as u8;
    b[13] = r1 as u8;
    b[14] = (r2 >> 8) as u8;
    b[15] = r2 as u8;

    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0],
        b[1],
        b[2],
        b[3],
        b[4],
        b[5],
        b[6],
        b[7],
        b[8],
        b[9],
        b[10],
        b[11],
        b[12],
        b[13],
        b[14],
        b[15]
    )
}

thread_local! {
    static RNG: Cell<u64> = Cell::new(seed_for_thread());
}

fn seed_for_thread() -> u64 {
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0xa5a5_a5a5_a5a5_a5a5);
    let tid = thread_id_hash();
    let mono = Instant::now().elapsed().as_nanos() as u64;
    t.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ tid ^ mono.rotate_left(17)
}

fn thread_id_hash() -> u64 {
    let id = std::thread::current().id();
    // ThreadId's only stable trait is Debug; hash its formatted form
    let s = format!("{id:?}");
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

fn next_random() -> u64 {
    RNG.with(|c| {
        let mut x = c.get();
        if x == 0 {
            x = 0xdead_beef_cafe_f00d;
        }
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        c.set(x);
        x
    })
}

pub async fn request_id_middleware(mut req: Request<Body>, next: Next) -> Response {
    let supplied = req
        .headers()
        .get(&X_REQUEST_ID)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_uuid_v7_strict);

    let id = supplied.unwrap_or_else(generate_uuid_v7);
    let header_value =
        HeaderValue::from_str(&id).unwrap_or_else(|_| HeaderValue::from_static("invalid"));

    req.extensions_mut().insert(RequestId(id.clone()));

    let method = req.method().clone();
    let path = req
        .uri()
        .path_and_query()
        .map(|p| p.as_str().to_owned())
        .unwrap_or_else(|| req.uri().path().to_owned());

    let span = tracing::info_span!(
        "request",
        method = %method,
        path = %path,
        request_id = %id,
        status = tracing::field::Empty,
        latency_ms = tracing::field::Empty,
    );

    let started = Instant::now();
    let mut response = next.run(req).instrument(span.clone()).await;
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    let status: StatusCode = response.status();

    span.record("status", status.as_u16());
    span.record("latency_ms", elapsed_ms);
    let _enter = span.enter();
    tracing::info!(
        status = status.as_u16(),
        latency_ms = elapsed_ms,
        request_id = %id,
        "request.completed"
    );

    response.headers_mut().insert(X_REQUEST_ID, header_value);
    response
}
