#![forbid(unsafe_code)]

//! cmd-share：局域网文件互传。电脑跑服务，手机扫码（浏览器零安装）。
//!
//! 分层约定：
//! - `util.rs`   文件名安全/重名处理（纯函数）
//! - `net.rs`    本机局域网 IP 探测
//! - `qr.rs`     URL → 终端二维码（半块 + 反色）
//! - `server.rs` axum 路由与处理器（token 校验中间件）

pub mod hotspot;
pub mod net;
pub mod qr;
pub mod server;
pub mod util;
