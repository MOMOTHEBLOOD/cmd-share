//! 路由集成测试：token 校验、列表、下载、上传、路径安全。
//! 用 tower::ServiceExt::oneshot 直接打 Router，不需要真开端口。

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use cmd_share::server::{router, AppState};
use http_body_util::BodyExt as _;
use tower::ServiceExt as _;

struct Fixture {
    _dir: PathBuf,
    token: &'static str,
    router: axum::Router,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self._dir);
    }
}

fn addr_ext() -> axum::extract::ConnectInfo<std::net::SocketAddr> {
    use std::net::SocketAddr;
    axum::extract::ConnectInfo("127.0.0.1:1111".parse::<SocketAddr>().unwrap())
}

fn fixture() -> Fixture {
    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("cmd-share-it-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("你好.txt"), "中文内容123").unwrap();
    std::fs::write(dir.join("data.bin"), [1u8, 2, 3, 4]).unwrap();
    let token = "tok3n";
    let state = AppState {
        dir: Arc::new(dir.clone()),
        token: Arc::new(token.to_string()),
    };
    Fixture {
        _dir: dir,
        token,
        router: router(state),
    }
}

async fn body_string(resp: axum::response::Response) -> String {
    String::from_utf8(
        resp.into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap()
}

#[tokio::test]
async fn missing_or_wrong_token_is_401() {
    let fx = fixture();
    for uri in ["/", "/api/list", "/api/clip"] {
        let resp = fx
            .router
            .clone()
            .oneshot(
                Request::get(uri)
                    .extension(addr_ext())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "{uri}");
    }
    let resp = fx
        .router
        .clone()
        .oneshot(
            Request::get("/api/list?t=wrong")
                .extension(addr_ext())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn page_served_with_token() {
    let fx = fixture();
    let resp = fx
        .router
        .clone()
        .oneshot(
            Request::get(format!("/?t={}", fx.token))
                .extension(addr_ext())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("文件互传"));
}

#[tokio::test]
async fn list_returns_seeded_files() {
    let fx = fixture();
    let resp = fx
        .router
        .clone()
        .oneshot(
            Request::get(format!("/api/list?t={}", fx.token))
                .extension(addr_ext())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("你好.txt"));
    assert!(body.contains("data.bin"));
}

#[tokio::test]
async fn download_roundtrip_and_traversal_rejected() {
    let fx = fixture();
    let resp = fx
        .router
        .clone()
        .oneshot(
            Request::get(format!("/d/{}?t={}", "你好.txt", fx.token))
                .extension(addr_ext())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body_string(resp).await, "中文内容123");

    // 路径穿越：全部拒绝
    for name in ["..%2F..%2Fsecret.txt", "a%2Fb", "a%5Cb", "..", "%2E%2E"] {
        let resp = fx
            .router
            .clone()
            .oneshot(
                Request::get(format!("/d/{name}?t={}", fx.token))
                    .extension(addr_ext())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND, "{name}");
    }
}

#[tokio::test]
async fn upload_saves_sanitized_unique_file() {
    let fx = fixture();
    let boundary = "XyZzY123";
    let body = format!(
        "--{b}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"报 告:最终版.txt\"\r\nContent-Type: text/plain\r\n\r\n上传内容\r\n--{b}--\r\n",
        b = boundary
    );
    let resp = fx
        .router
        .clone()
        .oneshot(
            Request::post(format!("/up?t={}", fx.token))
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .extension(addr_ext())
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("报 告_最终版.txt"), "响应: {body}");

    // 再传同名 → 自动重名
    let resp = fx.router.clone().oneshot(
        Request::post(format!("/up?t={}", fx.token))
            .header("content-type", format!("multipart/form-data; boundary={boundary}"))
            .extension(addr_ext())
            .body(Body::from(format!(
                "--{b}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"报 告:最终版.txt\"\r\n\r\n第二份\r\n--{b}--\r\n",
                b = boundary
            )))
            .unwrap(),
    ).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(body_string(resp).await.contains("报 告_最终版(1).txt"));

    // 落盘内容正确
    let saved = std::fs::read_to_string(fx._dir.join("报 告_最终版.txt")).unwrap();
    assert_eq!(saved, "上传内容");
    let saved2 = std::fs::read_to_string(fx._dir.join("报 告_最终版(1).txt")).unwrap();
    assert_eq!(saved2, "第二份");
}

#[tokio::test]
async fn upload_without_file_field_is_400() {
    let fx = fixture();
    let boundary = "B2";
    let resp = fx
        .router
        .clone()
        .oneshot(
            Request::post(format!("/up?t={}", fx.token))
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .extension(addr_ext())
                .body(Body::from(format!("--{b}--\r\n", b = boundary)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn text_in_replies_ok() {
    let fx = fixture();
    let resp = fx
        .router
        .clone()
        .oneshot(
            Request::post(format!("/api/text?t={}", fx.token))
                .header("content-type", "application/json")
                .extension(addr_ext())
                .body(Body::from(r#"{"text":"手机传来的话"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(body_string(resp).await.contains("\"ok\":true"));
}
