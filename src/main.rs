use axum::{
    body::Bytes,
    extract::DefaultBodyLimit,
    http::{header, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use rbx_binary::{from_reader, to_writer};
use std::{io::Cursor, net::SocketAddr};
use tower_http::cors::CorsLayer;

const MAX_UPLOAD_BYTES: usize = 100 * 1024 * 1024;

async fn health() -> impl IntoResponse {
    (StatusCode::OK, "PERMA RBXL Parser online")
}

/// Converts a binary .rbxl place into an .rbxm containing its top-level children.
async fn convert(body: Bytes) -> Response {
    if body.is_empty() {
        return (StatusCode::BAD_REQUEST, "Empty request body").into_response();
    }
    if body.len() > MAX_UPLOAD_BYTES {
        return (StatusCode::PAYLOAD_TOO_LARGE, "File exceeds parser limit (100 MiB)").into_response();
    }
    let parsed = match from_reader(Cursor::new(body.as_ref())) {
        Ok(dom) => dom,
        Err(err) => {
            eprintln!("RBXL decode failed: {err}");
            return (StatusCode::UNPROCESSABLE_ENTITY, "Invalid or unsupported RBXL binary file").into_response();
        }
    };
    let roots: Vec<_> = parsed.root().children().iter().copied().collect();
    if roots.is_empty() {
        return (StatusCode::UNPROCESSABLE_ENTITY, "RBXL contains no top-level instances").into_response();
    }
    let mut output = Vec::with_capacity(body.len().saturating_add(1024));
    if let Err(err) = to_writer(&mut output, &parsed, &roots) {
        eprintln!("RBXM encode failed: {err}");
        return (StatusCode::INTERNAL_SERVER_ERROR, "Could not encode converted RBXM").into_response();
    }
    let mut response = output.into_response();
    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("application/octet-stream"));
    response.headers_mut().insert(header::CONTENT_DISPOSITION, HeaderValue::from_static("attachment; filename=\"converted.rbxm\""));
    response.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(health))
        .route("/health", get(health))
        .route("/convert", post(convert))
        .layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES))
        .layer(CorsLayer::new()
            .allow_origin(tower_http::cors::Any)
            .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
            .allow_headers([header::CONTENT_TYPE]));
    let port: u16 = std::env::var("PORT").ok().and_then(|v| v.parse().ok()).unwrap_or(8080);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr).await.expect("failed to bind parser service");
    println!("PERMA RBXL Parser listening on {addr}");
    axum::serve(listener, app).await.expect("parser service failed");
      }
                
