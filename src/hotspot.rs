//! Windows 移动热点控制：通过 PowerShell 调用系统 WinRT API
//! （NetworkOperatorTetheringManager，Win10 1607+ 原生功能）。
//!
//! 三种动作：status（只读查询）/ start（确保开启，返回 SSID/密码）/ stop（恢复关闭）。
//! 脚本经 `powershell -NoProfile -Command -` 从 stdin 注入，不落临时文件；
//! 参数以占位符替换进脚本（单引号转义），全程不经过 shell 解释。

use anyhow::{bail, Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct HotspotInfo {
    pub ssid: String,
    pub pass: String,
    /// Off / On / ...（PowerShell 枚举字符串）
    pub state: String,
    /// 本次调用是否真的执行了开启动作
    #[serde(default)]
    pub started: bool,
}

impl HotspotInfo {
    pub fn is_on(&self) -> bool {
        self.state.eq_ignore_ascii_case("On")
    }
}

/// 生成 Wi-Fi 入网二维码内容（WIFI: 格式，手机相机可直接扫码加入网络）
pub fn wifi_qr_string(ssid: &str, pass: &str) -> String {
    format!(
        "WIFI:T:WPA;S:{};P:{};;",
        escape_wifi(ssid),
        escape_wifi(pass)
    )
}

/// WIFI: 协议转义：\ ; , : " 四个字符前加反斜杠
fn escape_wifi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '\\' | ';' | ',' | ':' | '"') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn ps_quote(s: &str) -> String {
    s.replace('\'', "''")
}

const SCRIPT_TEMPLATE: &str = r#"
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
Add-Type -AssemblyName System.Runtime.WindowsRuntime
$action = '__ACTION__'
$wantSsid = '__SSID__'
$wantPass = '__PASS__'
try {
    [void][Windows.Networking.Connectivity.NetworkInformation,Windows.Networking.Connectivity,ContentType=WindowsRuntime]
    [void][Windows.Networking.NetworkOperators.NetworkOperatorTetheringManager,Windows.Networking.NetworkOperators,ContentType=WindowsRuntime]
    $asTask = ([System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {
        $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and
        $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' })[0]

    function Await($op, $t) {
        $m = $asTask.MakeGenericMethod($t)
        $task = $m.Invoke($null, @($op))
        $task.Wait(-1) | Out-Null
        $task.Result
    }

    $profile = [Windows.Networking.Connectivity.NetworkInformation]::GetInternetConnectionProfile()
    if ($null -eq $profile) {
        Write-Output ('{"ok":false,"error":"no-profile"}')
        exit 0
    }
    $mgr = [Windows.Networking.NetworkOperators.NetworkOperatorTetheringManager]::CreateFromConnectionProfile($profile)
    $cfg = $mgr.GetCurrentAccessPointConfiguration()
    $state = $mgr.TetheringOperationalState.ToString()
    $started = $false

    if ($action -eq 'start' -and $state -ne 'On') {
        if ($wantSsid -ne '') { $cfg.Ssid = $wantSsid }
        if ($wantPass -ne '') { $cfg.Passphrase = $wantPass }
        if ($wantSsid -ne '' -or $wantPass -ne '') {
            Await ($mgr.ConfigureAccessPointAsync($cfg)) ([Object]) | Out-Null
        }
        $r = Await ($mgr.StartTetheringAsync()) ([Windows.Networking.NetworkOperators.NetworkOperatorTetheringOperationResult])
        if ($r.Status.ToString() -ne 'Success') {
            Write-Output ('{"ok":false,"error":"start-failed","detail":"' + $r.AdditionalErrorMessage.Replace('"','''') + '"}')
            exit 0
        }
        $started = $true
        $state = $mgr.TetheringOperationalState.ToString()
        $cfg = $mgr.GetCurrentAccessPointConfiguration()
    }
    elseif ($action -eq 'stop' -and $state -eq 'On') {
        Await ($mgr.StopTetheringAsync()) ([Windows.Networking.NetworkOperators.NetworkOperatorTetheringOperationResult]) | Out-Null
        $state = $mgr.TetheringOperationalState.ToString()
    }

    $ssid = $cfg.Ssid -replace '"', '\"'
    $pass = $cfg.Passphrase -replace '"', '\"'
    Write-Output ('{"ok":true,"ssid":"' + $ssid + '","pass":"' + $pass + '","state":"' + $state + '","started":' + ($started.ToString().ToLower()) + '}')
}
catch {
    $msg = $_.Exception.Message.Replace('"', '''').Replace("`n", ' ')
    Write-Output ('{"ok":false,"error":"exception","detail":"' + $msg + '"}')
}
"#;

fn run_script(action: &str, ssid: &str, pass: &str) -> Result<HotspotInfo> {
    use std::io::Write as _;
    use std::process::{Command, Stdio};

    let script = SCRIPT_TEMPLATE
        .replace("__ACTION__", &ps_quote(action))
        .replace("__SSID__", &ps_quote(ssid))
        .replace("__PASS__", &ps_quote(pass));

    let mut child = Command::new("powershell")
        // 脚本经 stdin 注入；Out-String 先把多行脚本拼成整体再 iex
        // （逐行 iex 会把 try/catch 拆碎；-Command - 会静默吞脚本）
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$input | Out-String | iex",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("启动 powershell 失败")?;
    child
        .stdin
        .take()
        .context("无法写入 powershell stdin")?
        .write_all(script.as_bytes())?;

    let out = child.wait_with_output().context("执行 powershell 失败")?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let line = stdout
        .lines()
        .rev()
        .find(|l| l.trim_start().starts_with('{'))
        .context("powershell 未返回 JSON")?;

    #[derive(Deserialize)]
    struct Raw {
        ok: bool,
        #[serde(default)]
        ssid: String,
        #[serde(default)]
        pass: String,
        #[serde(default)]
        state: String,
        #[serde(default)]
        started: bool,
        #[serde(default)]
        error: String,
        #[serde(default)]
        detail: String,
    }
    let raw: Raw =
        serde_json::from_str(line.trim()).with_context(|| format!("解析输出失败：{line}"))?;
    if !raw.ok {
        let hint = match raw.error.as_str() {
            "no-profile" => "电脑当前没有已联网的网络连接，移动热点需要一个可共享的网络来源",
            "start-failed" => "热点启动失败（可能是网卡不支持，或被组策略禁用）",
            _ => "WinRT 调用失败",
        };
        let detail = if raw.detail.is_empty() {
            String::new()
        } else {
            format!("：{}", raw.detail)
        };
        bail!("{hint}{detail}——可改用手机开热点、电脑连入的方式（无需本功能）");
    }
    Ok(HotspotInfo {
        ssid: raw.ssid,
        pass: raw.pass,
        state: raw.state,
        started: raw.started,
    })
}

/// 只读查询热点状态（不动系统状态）
pub fn status() -> Result<HotspotInfo> {
    run_script("status", "", "")
}

/// 确保热点开启（已开则原样返回），返回配置与是否由本次启动
pub fn ensure_running(ssid: Option<&str>, pass: Option<&str>) -> Result<HotspotInfo> {
    run_script("start", ssid.unwrap_or_default(), pass.unwrap_or_default())
}

/// 恢复热点为关闭状态（仅当本次由我们启动时调用）
pub fn restore() -> Result<()> {
    run_script("stop", "", "").map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wifi_qr_escapes_special_chars() {
        // WIFI: 协议只转义 \ ; , : " 四类字符（空格不需要）
        assert_eq!(
            wifi_qr_string("My Net", "p;a:ss\"w\\"),
            "WIFI:T:WPA;S:My Net;P:p\\;a\\:ss\\\"w\\\\;;"
        );
        assert_eq!(
            wifi_qr_string("plain", "12345678"),
            "WIFI:T:WPA;S:plain;P:12345678;;"
        );
    }

    #[test]
    fn ps_quote_escapes_single_quotes() {
        assert_eq!(ps_quote("ab'cd"), "ab''cd");
        assert_eq!(ps_quote("plain"), "plain");
    }

    #[test]
    fn parses_script_json_output() {
        let line = r#"{"ok":true,"ssid":"DESK-5G","pass":"secret1","state":"On","started":true}"#;
        let info: HotspotInfo = serde_json::from_str(line).unwrap();
        assert!(info.is_on());
        assert!(info.started);
        assert_eq!(info.ssid, "DESK-5G");
    }
}
