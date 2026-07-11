// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::io::AsyncWriteExt;
use serde::{Serialize, Deserialize};
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;
use axum::{
    Router,
    routing::{get, post},
    extract::{Path, State as AxumState, Request},
    response::{Html, IntoResponse, Sse},
    http::{HeaderMap, StatusCode, header},
    body::Body,
};
use axum::response::sse::{Event, KeepAlive};
use tokio_util::io::ReaderStream;
use tokio_stream::wrappers::BroadcastStream;
use futures_util::StreamExt;
use base64::Engine;

// ─── Data Models ────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SharedFile {
    pub id:   String,
    pub name: String,
    pub path: String,
    pub size: u64,
}

#[derive(Serialize, Clone, Debug)]
pub struct UploadProgress {
    pub file_name:      String,
    pub bytes_received: u64,
    pub total_bytes:    u64,
    pub speed_mbps:     f64,
    pub status:         String,   // "active" | "completed" | "error"
    pub message:        String,
    pub saved_path:     Option<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct ServerInfo {
    pub url:        String,
    pub qr_png_b64: String,
}

// ─── Application State ──────────────────────────────────────────────────────

/// State shared between Tauri commands (managed by Tauri)
pub struct AppState {
    pub shared_files: Arc<Mutex<Vec<SharedFile>>>,
    pub server_info:  Arc<Mutex<Option<ServerInfo>>>,
}

/// State injected into axum handlers
struct HttpState {
    shared_files: Arc<Mutex<Vec<SharedFile>>>,
    upload_tx:    tokio::sync::broadcast::Sender<UploadProgress>,
    app_handle:   AppHandle,
}

// ─── Helpers ────────────────────────────────────────────────────────────────

fn get_local_ip() -> Option<String> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    socket.local_addr().ok().map(|a| a.ip().to_string())
}

fn generate_qr_png_base64(url: &str) -> Option<String> {
    let code  = qrcode::QrCode::new(url.as_bytes()).ok()?;
    let img   = code.render::<image::Luma<u8>>().min_dimensions(280, 280).build();
    let dyn_img = image::DynamicImage::ImageLuma8(img);
    let mut png_bytes = Vec::new();
    dyn_img
        .write_to(
            &mut std::io::Cursor::new(&mut png_bytes),
            image::ImageFormat::Png,
        )
        .ok()?;
    Some(base64::engine::general_purpose::STANDARD.encode(&png_bytes))
}

// ─── HTTP Handlers ──────────────────────────────────────────────────────────

/// Mobile web page (embedded at compile time)
static MOBILE_HTML: &str = include_str!("mobile.html");

async fn handle_root() -> Html<&'static str> {
    Html(MOBILE_HTML)
}

/// GET /files  →  JSON array of SharedFile
async fn handle_files_list(
    AxumState(state): AxumState<Arc<HttpState>>,
) -> axum::Json<Vec<SharedFile>> {
    let files = state.shared_files.lock().await;
    axum::Json(files.clone())
}

/// GET /files/:id  →  stream the file from disk
async fn handle_file_download(
    AxumState(state): AxumState<Arc<HttpState>>,
    Path(file_id): Path<String>,
) -> impl IntoResponse {
    let files = state.shared_files.lock().await;
    let found = files.iter().find(|f| f.id == file_id).cloned();
    drop(files);

    let shared = match found {
        Some(f) => f,
        None    => return StatusCode::NOT_FOUND.into_response(),
    };

    match tokio::fs::File::open(&shared.path).await {
        Ok(file) => {
            let stream      = ReaderStream::new(file);
            let body        = Body::from_stream(stream);
            let mime        = mime_guess::from_path(&shared.path).first_or_octet_stream().to_string();
            let disposition = format!("attachment; filename=\"{}\"", shared.name);

            let mut hdrs = HeaderMap::new();
            hdrs.insert(header::CONTENT_TYPE,        mime.parse().unwrap_or(header::HeaderValue::from_static("application/octet-stream")));
            hdrs.insert(header::CONTENT_DISPOSITION, disposition.parse().unwrap());
            hdrs.insert(header::CONTENT_LENGTH,      shared.size.to_string().parse().unwrap());
            hdrs.insert(header::CACHE_CONTROL,       "no-cache".parse().unwrap());

            (StatusCode::OK, hdrs, body).into_response()
        }
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

/// POST /upload  →  stream body directly to Downloads folder, emit progress
async fn handle_upload(
    AxumState(state): AxumState<Arc<HttpState>>,
    headers: HeaderMap,
    request: Request,
) -> impl IntoResponse {
    // Decode filename from X-File-Name header (URL-encoded by phone)
    let raw_name = headers
        .get("x-file-name")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("upload");
    let file_name = urlencoding::decode(raw_name)
        .map(|s| s.into_owned())
        .unwrap_or_else(|_| raw_name.to_owned());

    // Content-Length for progress percentage (may be absent)
    let total_bytes: u64 = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    // Build a safe save path in the user's Downloads folder
    let mut save_dir = dirs::download_dir()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    let safe_name = std::path::Path::new(&file_name)
        .file_name()
        .unwrap_or(std::ffi::OsStr::new("upload"))
        .to_string_lossy()
        .to_string();

    save_dir.push(&safe_name);

    // Avoid overwriting existing files
    let mut final_path = save_dir.clone();
    let mut suffix = 1u32;
    while final_path.exists() {
        let stem = save_dir.file_stem().unwrap_or_default().to_string_lossy();
        let ext  = save_dir
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        final_path = save_dir.with_file_name(format!("{}_{}{}", stem, suffix, ext));
        suffix += 1;
    }

    let mut file = match tokio::fs::File::create(&final_path).await {
        Ok(f)  => f,
        Err(e) => {
            eprintln!("[upload] create file error: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to create destination file").into_response();
        }
    };

    let start          = std::time::Instant::now();
    let mut bytes_recv: u64 = 0;

    // Stream body chunks directly to disk
    let body   = request.into_body();
    let mut stream = body.into_data_stream();

    while let Some(chunk_result) = stream.next().await {
        let data = match chunk_result {
            Ok(d)  => d,
            Err(e) => {
                eprintln!("[upload] stream error: {}", e);
                drop(file);
                let _ = tokio::fs::remove_file(&final_path).await;
                return (StatusCode::BAD_REQUEST, "Stream error during upload").into_response();
            }
        };

        if let Err(e) = file.write_all(&data).await {
            eprintln!("[upload] write error: {}", e);
            drop(file);
            let _ = tokio::fs::remove_file(&final_path).await;
            return (StatusCode::INTERNAL_SERVER_ERROR, "Disk write error").into_response();
        }

        bytes_recv += data.len() as u64;

        // Throttle progress events to ~10 per second
        let elapsed_secs = start.elapsed().as_secs_f64();
        let speed        = if elapsed_secs > 0.0 { (bytes_recv as f64 / 1_048_576.0) / elapsed_secs } else { 0.0 };

        let progress = UploadProgress {
            file_name:      safe_name.clone(),
            bytes_received: bytes_recv,
            total_bytes,
            speed_mbps:     speed,
            status:         "active".to_string(),
            message:        format!("Receiving {} ({} MB/s)…", safe_name, format!("{:.1}", speed)),
            saved_path:     None,
        };

        let _ = state.app_handle.emit("upload-progress", &progress);
        let _ = state.upload_tx.send(progress);   // broadcast to SSE subscribers
    }

    // Flush file to disk
    if let Err(e) = file.flush().await { eprintln!("[upload] flush error: {}", e); }
    drop(file);

    let elapsed_secs = start.elapsed().as_secs_f64();
    let speed        = if elapsed_secs > 0.0 { (bytes_recv as f64 / 1_048_576.0) / elapsed_secs } else { 0.0 };
    let saved_str    = final_path.to_string_lossy().to_string();

    let done = UploadProgress {
        file_name:      safe_name.clone(),
        bytes_received: bytes_recv,
        total_bytes:    bytes_recv,   // use actual bytes as total when Content-Length was absent
        speed_mbps:     speed,
        status:         "completed".to_string(),
        message:        format!("Saved to: {}", saved_str),
        saved_path:     Some(saved_str),
    };

    let _ = state.app_handle.emit("upload-progress", &done);
    let _ = state.upload_tx.send(done);

    (StatusCode::OK, "Upload complete").into_response()
}

/// GET /events  →  SSE stream of upload progress events (for future phone-side use)
async fn handle_sse(
    AxumState(state): AxumState<Arc<HttpState>>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let rx = state.upload_tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|result| async move {
        match result {
            Ok(progress) => {
                let data = serde_json::to_string(&progress).unwrap_or_default();
                Some(Ok(Event::default().data(data)))
            }
            Err(_) => None,
        }
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

// ─── HTTP Server ─────────────────────────────────────────────────────────────

async fn run_http_server(
    app_handle:   AppHandle,
    shared_files: Arc<Mutex<Vec<SharedFile>>>,
    server_info:  Arc<Mutex<Option<ServerInfo>>>,
) {
    let ip   = get_local_ip().unwrap_or_else(|| "127.0.0.1".to_string());
    let port = 5000u16;
    let url  = format!("http://{}:{}", ip, port);

    let qr_b64 = generate_qr_png_base64(&url).unwrap_or_default();
    let info   = ServerInfo { url: url.clone(), qr_png_b64: qr_b64 };

    // Store in shared state so Tauri commands can read it even before the event fires
    *server_info.lock().await = Some(info.clone());

    // Emit to desktop frontend
    let _ = app_handle.emit("server-started", &info);

    let (upload_tx, _) = tokio::sync::broadcast::channel::<UploadProgress>(128);

    let http_state = Arc::new(HttpState {
        shared_files,
        upload_tx,
        app_handle,
    });

    let router = Router::new()
        .route("/",          get(handle_root))
        .route("/files",     get(handle_files_list))
        .route("/files/{id}", get(handle_file_download))
        .route("/upload",    post(handle_upload))
        .route("/events",    get(handle_sse))
        .with_state(http_state);

    let bind_addr = format!("0.0.0.0:{}", port);
    let listener  = match tokio::net::TcpListener::bind(&bind_addr).await {
        Ok(l)  => l,
        Err(e) => {
            eprintln!("[server] Failed to bind {}: {}", bind_addr, e);
            return;
        }
    };

    println!("[server] Listening on {}", url);
    if let Err(e) = axum::serve(listener, router).await {
        eprintln!("[server] Error: {}", e);
    }
}

// ─── Tauri Commands ──────────────────────────────────────────────────────────

/// Returns the current server URL + QR code PNG (base64). Returns None while
/// the server is still starting up (usually < 100 ms).
#[tauri::command]
async fn get_server_info(
    state: State<'_, AppState>,
) -> Result<Option<ServerInfo>, String> {
    Ok(state.server_info.lock().await.clone())
}

/// Returns the list of files currently shared for download.
#[tauri::command]
async fn get_shared_files(
    state: State<'_, AppState>,
) -> Result<Vec<SharedFile>, String> {
    Ok(state.shared_files.lock().await.clone())
}

/// Opens a native file picker and adds the chosen file to the shared-file list.
#[tauri::command]
async fn add_shared_file(
    state: State<'_, AppState>,
    path:  String,
) -> Result<SharedFile, String> {
    let meta = tokio::fs::metadata(&path)
        .await
        .map_err(|e| e.to_string())?;

    let name = std::path::Path::new(&path)
        .file_name()
        .ok_or_else(|| "Invalid file path".to_string())?
        .to_string_lossy()
        .to_string();

    let sf = SharedFile {
        id:   Uuid::new_v4().to_string(),
        name,
        path,
        size: meta.len(),
    };

    state.shared_files.lock().await.push(sf.clone());
    Ok(sf)
}

/// Removes a file from the shared list by its UUID.
#[tauri::command]
async fn remove_shared_file(
    state: State<'_, AppState>,
    id:    String,
) -> Result<(), String> {
    state.shared_files.lock().await.retain(|f| f.id != id);
    Ok(())
}

/// Opens a native file-picker dialog and returns all chosen paths (empty vec if cancelled).
#[tauri::command]
async fn select_file() -> Result<Vec<String>, String> {
    let files = rfd::FileDialog::new()
        .set_title("Select Files to Share")
        .pick_files();   // ← multi-select
    Ok(files
        .unwrap_or_default()
        .into_iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect())
}

// ─── Entrypoint ──────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let shared_files = Arc::new(Mutex::new(Vec::<SharedFile>::new()));
    let server_info: Arc<Mutex<Option<ServerInfo>>> = Arc::new(Mutex::new(None));

    let shared_files_clone = shared_files.clone();
    let server_info_clone  = server_info.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            shared_files: shared_files.clone(),
            server_info:  server_info.clone(),
        })
        .invoke_handler(tauri::generate_handler![
            get_server_info,
            get_shared_files,
            add_shared_file,
            remove_shared_file,
            select_file,
        ])
        .setup(move |app| {
            let app_handle = app.handle().clone();

            // Spawn the HTTP server on the Tauri async runtime
            tauri::async_runtime::spawn(async move {
                run_http_server(app_handle, shared_files_clone, server_info_clone).await;
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
