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
    /// 一条龙：自动开启 Windows 移动热点，退出时恢复
    #[arg(long)]
    hotspot: bool,
    /// 热点 SSID（默认沿用系统现有配置）
    #[arg(long)]
    hotspot_ssid: Option<String>,
    /// 热点密码（默认沿用系统现有配置）
    #[arg(long)]
    hotspot_pass: Option<String>,
    /// 只查询热点状态，不做任何改动
    #[arg(long)]
    hotspot_status: bool,
    /// 关闭移动热点并退出（--hotspot 进程被强杀后用它收尾）
    #[arg(long)]
    hotspot_stop: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // ---- 热点诊断模式（只读）----
    if cli.hotspot_status {
        let info = cmd_share::hotspot::status()?;
        println!(
            "移动热点：{}  SSID={}  密码={}",
            if info.is_on() { "开启" } else { "关闭" },
            info.ssid,
            info.pass
        );
        return Ok(());
    }

    // ---- 关闭热点 ----
    if cli.hotspot_stop {
        cmd_share::hotspot::restore()?;
        println!("移动热点已关闭。");
        return Ok(());
    }

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

    // ---- 热点一条龙：先拉热点，再决定展示哪个 IP ----
    let mut restore_hotspot = false;
    let mut hotspot_info = None;
    if cli.hotspot {
        let info = cmd_share::hotspot::ensure_running(
            cli.hotspot_ssid.as_deref(),
            cli.hotspot_pass.as_deref(),
        )?;
        restore_hotspot = info.started;
        if !info.is_on() {
            bail!(
                "热点未能开启（状态 {}）——可改用手机开热点、电脑连入",
                info.state
            );
        }
        // ICS 虚拟网卡拿地址需要几秒：轮询等待 192.168.137.* 出现（最多 10s）
        let mut ics_ip = None;
        for _ in 0..20 {
            ics_ip = cmd_share::net::all_ipv4s()
                .into_iter()
                .find(|c| c.starts_with("192.168.137."));
            if ics_ip.is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        if ics_ip.is_none() {
            println!("注意：热点已开，但尚未检测到 192.168.137.x 地址（Windows 网段可能随机化），");
            println!("稍后请从候选地址里选择能打开页面的那个。");
        }
        hotspot_info = Some(info);
    }

    // IP 解析优先级：--ip 手动 > 热点网卡（ICS 默认 192.168.137.x）> 路由探测 >
    // 网卡枚举兜底
    let candidates = cmd_share::net::all_ipv4s();
    let (ip, via) = match &cli.ip {
        Some(i) => (i.clone(), "手动指定"),
        None => {
            let ics = candidates
                .iter()
                .find(|c| c.starts_with("192.168.137."))
                .filter(|_| cli.hotspot);
            match ics {
                Some(i) => (i.clone(), "热点网卡"),
                None => match cmd_share::net::lan_ip() {
                    Some(i) => (i, "路由探测"),
                    None => match cmd_share::net::prefer(&candidates) {
                        Some(i) => (i.clone(), "网卡枚举兜底"),
                        None => bail!("无法获取局域网 IP——请确认已联网，或用 --ip 手动指定"),
                    },
                },
            }
        }
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
    if let Some(info) = &hotspot_info {
        println!(
            "\x1b[1;33m移动热点已就绪\x1b[0m：SSID = {}   密码 = {}   （{}）",
            info.ssid,
            info.pass,
            if info.started {
                "本次启动"
            } else {
                "原本已开启"
            }
        );
        println!("第一步：手机扫下面的码加入热点 Wi-Fi：");
        println!(
            "{}",
            cmd_share::qr::render_terminal(&cmd_share::hotspot::wifi_qr_string(
                &info.ssid, &info.pass
            ))
        );
        println!("第二步：扫地址码打开互传页面：");
    } else {
        println!("或扫码（手机相机/微信扫一扫）：");
    }
    println!("{}", cmd_share::qr::render_terminal(&url));
    println!("提示：手机需与电脑在同一网络；首次运行如弹出防火墙窗口请点“允许访问”。");
    println!("上传的文件保存到共享目录；收到的文本会自动进入电脑剪贴板。Ctrl+C 退出。");
    println!();

    let state = cmd_share::server::AppState {
        dir: Arc::new(dir),
        token: Arc::new(token),
    };
    let server = cmd_share::server::serve(state, cli.port);
    tokio::pin!(server);
    tokio::select! {
        res = &mut server => res,
        _ = tokio::signal::ctrl_c() => {
            println!("\n收到 Ctrl+C，正在退出…");
            if restore_hotspot {
                match cmd_share::hotspot::restore() {
                    Ok(_) => println!("移动热点已恢复为关闭。"),
                    Err(e) => println!("恢复热点失败（可手动在系统设置里关闭）：{e:#}"),
                }
            }
            Ok(())
        }
    }
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
