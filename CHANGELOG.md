# Changelog

格式基于 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，
版本遵循 [SemVer](https://semver.org/lang/zh-CN/)。

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
