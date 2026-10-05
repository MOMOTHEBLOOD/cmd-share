# Changelog

格式基于 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，
版本遵循 [SemVer](https://semver.org/lang/zh-CN/)。

## [0.3.0] - 2026-10-05

### Added

- **`--hotspot` 一条龙**：通过系统原生 WinRT API（PowerShell 调
  NetworkOperatorTetheringManager，无新增依赖）自动拉起 Windows 移动热点，
  终端给出 Wi-Fi 入网二维码（WIFI: 协议，特殊字符转义）+ 互传地址码
- 主地址自动锁定热点 ICS 网段（192.168.137.x，轮询等待网卡就绪），
  Ctrl+C 退出时自动恢复热点原状；`--hotspot-stop` / `--hotspot-status`
  分别用于手动收尾与只读诊断；`--hotspot-ssid/-pass` 可自定义
- 真机全链路验证：拉起热点 → 双码 → 服务经 192.168.137.1 可达（401/200）→ 恢复关闭

### Fixed

- PowerShell 脚本注入方式：`-Command -` 会静默吞脚本、逐行 `iex` 会拆碎
  多行结构，改为 stdin + `Out-String | iex` 整体执行；补
  `System.Runtime.WindowsRuntime` 程序集加载与 UTF-8 输出编码

## [0.2.0] - 2026-10-05

### Added

- **多网卡 IP 候选**：启动时枚举全部非回环 IPv4，主地址按
  192.168 > 172.16-31 > 10.x 优先级选择，其余网卡（WSL/虚拟机/热点）的
  候选地址全部列出，手机连错网卡不再抓瞎
- **热点场景兜底**：手机热点等无外网路由环境（路由探测会失败）自动降级为
  网卡枚举，状态栏标注 IP 来源（路由探测 / 网卡枚举兜底 / 手动指定）
- 若干网络层单元测试（网段优先级、脏数据过滤、回环排除）

## [0.1.0] - 2026-10-04

### Added

- 局域网文件互传服务：终端显示二维码，手机浏览器扫码即用（安卓零安装）
- 文件列表 / 流式下载（中文文件名 UTF-8 编码）/ multipart 上传（进度、
  文件名清洗、防保留名、自动重名 (1)）
- 双向文本：手机→电脑自动写入剪贴板；电脑剪贴板→手机 5 秒轮询
- 安全：每次启动随机配对码，全部 API 强制校验（无码 401）；下载路径穿越
  防护；局域网 IP 自动探测（UDP connect 路由法）
- 测试：13 个（文件名安全/路径穿越/token/上传重名/路由集成）全绿；
  E2E curl 全流程验证（含二进制 md5 比对、中文文件名、UTF-8 文本）
