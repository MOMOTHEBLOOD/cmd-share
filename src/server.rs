//! HTTP 服务：列表 / 下载 / 上传 / 双向文本，全部经过配对码校验。

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::multipart::Multipart;
use axum::extract::DefaultBodyLimit;
use axum::extract::{ConnectInfo, Path, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use percent_encoding::utf8_percent_encode;
use serde::Deserialize;
use serde_json::json;
use std::net::SocketAddr;
use tokio::io::AsyncWriteExt as _;
use tokio_util::io::ReaderStream;

use crate::util::{safe_join, sanitize_filename, unique_path};

const PAGE_HTML: &str = include_str!("page.html");
/// 上传体积上限 10 GB
const MAX_BODY: usize = 10 * 1024 * 1024 * 1024;

#[derive(Clone)]
pub struct AppState {
    pub dir: Arc<PathBuf>,
    pub token: Arc<String>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(page))
        .route("/api/list", get(list))
        .route("/d/{*name}", get(download))
        .route("/up", post(upload))
        .route("/api/text", post(text_in))
        .route("/api/clip", get(clip_out))
        .layer(middleware::from_fn_with_state(state.clone(), auth_mw))
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .with_state(state)
}

/// 配对码校验：除页面外的所有请求必须带 ?t=<token>
async fn auth_mw(State(st): State<AppState>, req: axum::extract::Request, next: Next) -> Response {
    let ok = req
        .uri()
        .query()
        .map(|q| {
            q.split('&')
                .any(|kv| kv == format!("t={}", st.token.as_str()))
        })
        .unwrap_or(false);
    if ok {
        next.run(req).await
    } else {
        (
            StatusCode::UNAUTHORIZED,
            "配对码缺失或错误——请用电脑终端显示的完整地址访问",
        )
            .into_response()
    }
}

async fn page() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        PAGE_HTML,
    )
}

#[derive(serde::Serialize)]
struct FileEntry {
    name: String,
    size: u64,
}

async fn list(State(st): State<AppState>) -> Response {
    let mut files: Vec<FileEntry> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(st.dir.as_ref()) {
        for e in rd.flatten() {
            let Ok(meta) = e.metadata() else { continue };
            if meta.is_file() {
                files.push(FileEntry {
                    name: e.file_name().to_string_lossy().into_owned(),
                    size: meta.len(),
                });
            }
        }
    }
    files.sort_by_key(|f| f.name.to_lowercase());
    Json(json!({ "files": files })).into_response()
}

async fn download(State(st): State<AppState>, Path(name): Path<String>) -> Response {
    let Some(path) = safe_join(st.dir.as_ref(), &name) else {
        return (StatusCode::NOT_FOUND, "文件不存在或路径非法").into_response();
    };
    let file = match tokio::fs::File::open(&path).await {
        Ok(f) => f,
        Err(_) => return (StatusCode::NOT_FOUND, "文件无法打开").into_response(),
    };
    let encoded = utf8_percent_encode(&name, percent_encoding::NON_ALPHANUMERIC);
    let mut headers = [(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    )];
    if let Ok(v) = HeaderValue::from_str(&format!("attachment; filename*=UTF-8''{encoded}")) {
        headers[0].1 = v;
    }
    (headers, Body::from_stream(ReaderStream::new(file))).into_response()
}

async fn upload(
    State(st): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    mut mp: Multipart,
) -> Response {
    let mut saved: Vec<(String, u64)> = Vec::new();
    while let Ok(Some(field)) = mp.next_field().await {
        let Some(raw_name) = field.file_name().map(str::to_owned) else {
            continue;
        };
        let clean = sanitize_filename(&raw_name);
        let target = unique_path(st.dir.as_ref(), &clean);
        let mut file = match tokio::fs::File::create(&target).await {
            Ok(f) => f,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("写文件失败：{e}"),
                )
                    .into_response()
            }
        };
        let mut size: u64 = 0;
        let mut field = field;
        while let Ok(Some(chunk)) = field.chunk().await {
            if file.write_all(&chunk).await.is_err() {
                return (StatusCode::INTERNAL_SERVER_ERROR, "写入磁盘失败").into_response();
            }
            size += chunk.len() as u64;
        }
        let _ = file.flush().await;
        let saved_name = target
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        println!(
            "[↑] {}  {} ({})  ← {}",
            saved_name,
            size,
            human(size),
            addr.ip()
        );
        saved.push((saved_name, size));
    }
    if saved.is_empty() {
        return (StatusCode::BAD_REQUEST, "没有收到文件").into_response();
    }
    Json(json!({ "ok": true, "saved": saved })).into_response()
}

#[derive(Deserialize)]
struct TextIn {
    text: String,
}

async fn text_in(ConnectInfo(addr): ConnectInfo<SocketAddr>, Json(t): Json<TextIn>) -> Response {
    let preview: String = t.text.chars().take(80).collect();
    println!(
        "[T] 文本 ({} 字) ← {}: {}",
        t.text.chars().count(),
        addr.ip(),
        preview
    );
    if let Ok(mut cb) = arboard::Clipboard::new() {
        let _ = cb.set_text(&t.text);
        println!("[T] 已复制到电脑剪贴板");
    }
    Json(json!({ "ok": true })).into_response()
}

async fn clip_out() -> Response {
    let text = arboard::Clipboard::new()
        .ok()
        .and_then(|mut c| c.get_text().ok())
        .unwrap_or_default();
    Json(json!({ "text": text })).into_response()
}

fn human(n: u64) -> String {
    if n >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", n as f64 / 1_073_741_824.0)
    } else if n >= 1024 * 1024 {
        format!("{:.1} MB", n as f64 / 1_048_576.0)
    } else if n >= 1024 {
        format!("{:.1} KB", n as f64 / 1024.0)
    } else {
        format!("{n} B")
    }
}

pub async fn serve(state: AppState, port: u16) -> anyhow::Result<()> {
    let app = router(state);
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}
