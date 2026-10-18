use axum::{
    body::Bytes,
    extract::DefaultBodyLimit,
    http::{header, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use rbx_binary::{from_reader, to_writer};
use rbx_dom_weak::{InstanceBuilder, WeakDom};
use std::{io::Cursor, net::SocketAddr};
use tower_http::cors::CorsLayer;

const MAX_UPLOAD_BYTES: usize = 100 * 1024 * 1024;

async fn health() -> impl IntoResponse {
    (StatusCode::OK, "PERMA RBXL Parser online")
}

fn is_service(class_name: &str, name: &str) -> bool {
    const SERVICES: &[&str] = &[
        "Workspace", "ReplicatedStorage", "ReplicatedFirst", "ServerStorage",
        "ServerScriptService", "StarterGui", "StarterPack", "StarterPlayer",
        "Lighting", "SoundService", "Teams", "Chat", "TextChatService",
        "LocalizationService", "MaterialService", "HttpService",
        "CollectionService", "TestService", "VoiceChatService",
    ];
    SERVICES.contains(&class_name)
        || SERVICES.contains(&name)
        || class_name.ends_with("Service")
        || name.ends_with("Service")
}

/// Converts a binary place into a model whose service contents are represented
/// by ordinary Folders. Roblox services themselves cannot safely be loaded as
/// ordinary model instances through game:GetObjects().
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

    // Do not serialize engine service instances (Workspace, Lighting, TimerService,
    // etc.) as model roots. Instead, create ordinary Folders with the service names
    // and clone their contents beneath those folders. Non-service roots are retained.
    let mut output_dom = WeakDom::new(InstanceBuilder::new("DataModel"));
    let output_root = output_dom.root_ref();
    // RBXM consumers in lightweight executors commonly expect one model root.
    // A place contains many service roots, so package all converted services and
    // non-service roots beneath one ordinary Model before serializing.
    let model_root = output_dom.insert(
        output_root,
        InstanceBuilder::new("Model").with_name("PERMA_RBXL_IMPORT"),
    );
    let mut source_refs = Vec::new();
    let mut destinations = Vec::new();
    let mut service_count = 0usize;
    let mut content_count = 0usize;

    for root_ref in roots.iter().copied() {
        let Some(root_instance) = parsed.get_by_ref(root_ref) else { continue };
        let class_name = root_instance.class.to_string();
        let name = root_instance.name.clone();

        if is_service(&class_name, &name) {
            let folder_name = if !name.is_empty() { name } else { class_name.clone() };
            let folder_ref = output_dom.insert(
                model_root,
                InstanceBuilder::new("Folder").with_name(folder_name),
            );
            service_count += 1;

            let children: Vec<_> = root_instance.children().iter().copied().collect();
            content_count += children.len();
            for child_ref in children {
                source_refs.push(child_ref);
                destinations.push(folder_ref);
            }
        } else {
            source_refs.push(root_ref);
            destinations.push(model_root);
        }
    }

    if source_refs.is_empty() {
        eprintln!(
            "RBXL decoded but contained no serializable children: top_level={}, services={}, service_children={}",
            roots.len(), service_count, content_count
        );
        return (StatusCode::UNPROCESSABLE_ENTITY, "RBXL decoded, but no serializable objects were found inside its service roots").into_response();
    }

    // Clone all selected subtrees in one operation so references between objects
    // selected from different services can be preserved where possible.
    let cloned_refs = parsed.clone_multiple_into_external(&source_refs, &mut output_dom);
    if cloned_refs.is_empty() {
        return (StatusCode::UNPROCESSABLE_ENTITY, "RBXL decoded, but no objects could be cloned for model output").into_response();
    }

    for (cloned_ref, destination_ref) in cloned_refs.into_iter().zip(destinations.into_iter()) {
        output_dom.transfer_within(cloned_ref, destination_ref);
    }

    let serialized_roots = vec![model_root];
    if serialized_roots.is_empty() {
        return (StatusCode::INTERNAL_SERVER_ERROR, "Converted model has no root objects").into_response();
    }

    let mut output = Vec::with_capacity(body.len().saturating_add(1024));
    if let Err(err) = to_writer(&mut output, &output_dom, &serialized_roots) {
        eprintln!("RBXM encode failed: {err}");
        return (StatusCode::INTERNAL_SERVER_ERROR, "Could not encode converted RBXM").into_response();
    }

    eprintln!(
        "RBXL converted: bytes_in={}, top_level={}, service_folders={}, service_children={}, model_roots={}, bytes_out={}",
        body.len(), roots.len(), service_count, content_count, serialized_roots.len(), output.len()
    );

    let mut response = output.into_response();
    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("application/octet-stream"));
    response.headers_mut().insert(header::CONTENT_DISPOSITION, HeaderValue::from_static("attachment; filename=\"converted.rbxm\""));
    response.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert("x-perma-parser-roots", HeaderValue::from_str(&roots.len().to_string()).unwrap());
    response.headers_mut().insert("x-perma-parser-model-roots", HeaderValue::from_str(&serialized_roots.len().to_string()).unwrap());
    response.headers_mut().insert("x-perma-parser-service-children", HeaderValue::from_str(&content_count.to_string()).unwrap());
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
