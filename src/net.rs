//! 本机局域网 IP 探测。

use std::net::UdpSocket;

/// 用 UDP connect 让路由表选出默认出口网卡的本地地址（不实际发包）。
/// 有外网路由时这是质量最高的信号；离线（如手机热点无流量）时返回 None。
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

/// 枚举本机所有非回环 IPv4（手机热点等无外网路由场景的兜底）
pub fn all_ipv4s() -> Vec<String> {
    if_addrs::get_if_addrs()
        .map(|ifs| {
            ifs.into_iter()
                .filter_map(|i| match i.addr {
                    if_addrs::IfAddr::V4(v4) => Some(v4.ip.to_string()),
                    _ => None,
                })
                .filter(|ip| !ip.starts_with("127."))
                .collect()
        })
        .unwrap_or_default()
}

/// 从候选中选"最像局域网"的：192.168 > 172.16-31 > 10.x > 其他；
/// 同分取先出现的（网卡枚举顺序）。无法解析的条目剔除。
pub fn prefer(ips: &[String]) -> Option<String> {
    fn score(ip: &str) -> Option<u8> {
        let oct: Vec<u32> = ip.split('.').filter_map(|s| s.parse().ok()).collect();
        if oct.len() != 4 {
            return None;
        }
        Some(match (oct[0], oct[1]) {
            (192, 168) => 4,
            (172, n) if (16..=31).contains(&n) => 3,
            (10, _) => 2,
            _ => 1,
        })
    }
    let mut best: Option<(u8, &String)> = None;
    for ip in ips {
        if let Some(s) = score(ip) {
            match best {
                Some((bs, _)) if bs >= s => {}
                _ => best = Some((s, ip)),
            }
        }
    }
    best.map(|(_, ip)| ip.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lan_ip_returns_something_or_none() {
        // 有网/无网都不得 panic
        let _ = super::lan_ip();
    }

    #[test]
    fn all_ipv4s_no_panic_and_no_loopback() {
        for ip in all_ipv4s() {
            assert!(!ip.starts_with("127."));
        }
    }

    #[test]
    fn prefer_rfc1918_order() {
        let ips: Vec<String> = ["10.0.0.2", "192.168.43.7", "8.8.8.8"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(prefer(&ips).unwrap(), "192.168.43.7");

        let ips: Vec<String> = ["172.17.0.1", "10.1.1.1"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(prefer(&ips).unwrap(), "172.17.0.1");

        let ips: Vec<String> = ["8.8.8.8", "100.64.0.1"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(prefer(&ips).unwrap(), "8.8.8.8");
    }

    #[test]
    fn prefer_skips_junk_and_empty() {
        let ips: Vec<String> = ["not-an-ip", "192.168.1.5"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(prefer(&ips).unwrap(), "192.168.1.5");
        assert_eq!(prefer(&[]), None);
        assert_eq!(prefer(&["junk".to_string()]), None);
    }
}
