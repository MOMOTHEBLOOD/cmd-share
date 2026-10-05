use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use clap::Parser;
use uuid::Uuid;

/// 局域网文件互传：电脑跑起来，手机扫码即用
#[derive(Parser)]
#[command(name = "cmd-share", version)]
struct Cli {
    /// 要共享的目录（默认当前目录）
    dir: Option<PathBuf>,
    /// 监听端口
    #[arg(long, default_value_t = 8787)]
    port: u16,
    /// 固定配对码（默认每次随机生成）
    #[arg(long)]
    token: Option<String>,
    /// 手动指定本机局域网 IP（自动探测失败时用）
    #[arg(long)]
    ip: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let dir = cli
        .dir
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    if !dir.is_dir() {
        bail!("目录不存在：{}", dir.display());
    }
    let dir = dir.canonicalize().context("解析共享目录失败")?;

    let token = cli
        .token
        .unwrap_or_else(|| Uuid::new_v4().simple().to_string()[..6].to_string());

    // IP 解析优先级：--ip 手动 > 路由探测（有外网时最准） > 网卡枚举（热点等
    // 无外网路由场景的兜底，如手机热点网段 192.168.x）
    let candidates = cmd_share::net::all_ipv4s();
    let (ip, via) = match &cli.ip {
        Some(i) => (i.clone(), "手动指定"),
        None => match cmd_share::net::lan_ip() {
            Some(i) => (i, "路由探测"),
            None => match cmd_share::net::prefer(&candidates) {
                Some(i) => (i.clone(), "网卡枚举兜底"),
                None => bail!("无法获取局域网 IP——请确认已联网，或用 --ip 手动指定"),
            },
        },
    };
    let alternates: Vec<String> = candidates
        .iter()
        .filter(|c| **c != ip)
        .take(3)
        .cloned()
        .collect();
    let url = format!("http://{ip}:{}/?t={token}", cli.port);

    // ---- 统计共享目录 ----
    let (count, total) = dir_stats(&dir);
    println!("cmd-share v{}", env!("CARGO_PKG_VERSION"));
    println!(
        "共享目录：{}（{} 个文件，{}）",
        dir.display(),
        count,
        human(total)
    );
    println!();
    println!("手机浏览器打开（IP 来源：{via}）：");
    println!("  \x1b[1;36m{url}\x1b[0m");
    if !alternates.is_empty() {
        println!("连不上主地址时试试其他网卡：");
        for a in &alternates {
            println!("  \x1b[2mhttp://{a}:{}/?t={token}\x1b[0m", cli.port);
        }
    }
    println!();
    println!("或扫码（手机相机/微信扫一扫）：");
    println!("{}", cmd_share::qr::render_terminal(&url));
    println!("提示：手机需与电脑在同一局域网；首次运行如弹出防火墙窗口请点“允许访问”。");
    println!("上传的文件保存到共享目录；收到的文本会自动进入电脑剪贴板。Ctrl+C 退出。");
    println!();

    let state = cmd_share::server::AppState {
        dir: Arc::new(dir),
        token: Arc::new(token),
    };
    cmd_share::server::serve(state, cli.port).await
}

fn dir_stats(dir: &std::path::Path) -> (usize, u64) {
    let mut count = 0usize;
    let mut total = 0u64;
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            if let Ok(m) = e.metadata() {
                if m.is_file() {
                    count += 1;
                    total += m.len();
                }
            }
        }
    }
    (count, total)
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
