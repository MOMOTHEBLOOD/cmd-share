//! 本机局域网 IP 探测。

use std::net::UdpSocket;

/// 用 UDP connect 让路由表选出默认出口网卡的本地地址（不实际发包）。
/// 离线或无默认路由时返回 None（用户可用 --ip 手动指定）。
pub fn lan_ip() -> Option<String> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("223.5.5.5:80").ok()?;
    let ip = s.local_addr().ok()?.ip().to_string();
    if ip == "127.0.0.1" || ip == "0.0.0.0" {
        None
    } else {
        Some(ip)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn lan_ip_returns_something_or_none() {
        // 有网/无网都不得 panic
        let _ = super::lan_ip();
    }
}
