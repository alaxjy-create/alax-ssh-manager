# ALAX SSH Manager

[![CI](https://github.com/alaxjy-create/alax-ssh-manager/actions/workflows/ci.yml/badge.svg)](https://github.com/alaxjy-create/alax-ssh-manager/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/alaxjy-create/alax-ssh-manager)](https://github.com/alaxjy-create/alax-ssh-manager/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

ALAX SSH Manager 是一款 Windows 优先的 SSH/SFTP 桌面管理工具，使用 Tauri、React 和 Rust 构建。

当前版本：**v0.5.0**。

## 下载

- [GitHub Releases](https://github.com/alaxjy-create/alax-ssh-manager/releases/latest)
- 绿色版无需安装，服务器配置和凭据保存在当前 Windows 用户的本地数据目录与系统凭据存储中。
- v0.5.0 暂未进行 Authenticode 代码签名。运行前请核对 Release 页面提供的 SHA-256；Windows SmartScreen 可能显示未知发布者提示。

## v0.5.0 更新

- 新增 SSH 主机密钥 SHA-256 指纹确认、持久信任和变更阻断，首次连接必须确认，密钥变化默认拒绝连接。
- 新增持久化本地端口转发，可保存、启动、停止和编辑隧道规则；监听地址限制为本机回环地址。
- 新增远程 UTF-8 文本编辑器，支持 2 MB 上限、远端变更冲突检测、临时文件原子替换和权限保留。
- 新增文件权限修改、递归 `chmod`、SHA-256/SHA-1/MD5 校验和及增强属性面板。
- 新增文件夹上传与下载，使用系统目录选择器和后台传输任务完成压缩、传输与解压。
- 文件上传/下载支持断点续传、隐藏临时文件、完成后原子替换及失败恢复，避免半文件覆盖正式文件。
- 服务器复制改为真实持久化复制，密码、passphrase 和私钥仍只复制到系统安全凭据，不进入 SQLite。
- 修复服务器配置、凭据引用和主机指纹更新的事务一致性，启用每个 SQLite 连接的外键约束和忙等待。
- 修复监控轮询导致的高 CPU 风险；监控改为手动启动、禁止重叠请求，并在窗口隐藏时暂停。
- 修复切换工作区或终端标签后 xterm 画面变空白的问题；每个会话保留独立终端实例并在恢复可见时重绘。
- 增加终端、隧道、传输、命令输出、日志读取和文本预览的资源上限，避免异常输入造成内存或任务膨胀。
- 启用生产环境内容安全策略，日志敏感信息脱敏并阻止日志换行注入。

## 核心能力

### SSH

- 服务器、分组、标签、搜索和真实配置复制
- 密码、Ed25519/ECDSA 私钥文件、私钥内容和 passphrase 认证
- 系统安全凭据存储，SQLite 只保存不可用作登录的引用
- 主机密钥指纹确认与变更保护
- 多标签交互式 PTY 终端、历史回放、窗口尺寸同步和会话清理
- SSH keepalive、认证/连接/命令超时和输出大小限制
- 服务器 CPU、内存、磁盘、网络、温度、负载和运行时间监控
- 本地 SSH 端口转发

### SFTP

- 远程目录浏览、历史导航、路径跳转、排序、隐藏文件和多选
- 新建文件/文件夹、递归删除、重命名、远程压缩和系统“打开方式”
- 文件与目录上传/下载、后台进度、速度、取消、重试和断点续传
- 图片、音频、视频、PDF、文本和代码预览
- 内置文本编辑、冲突检测、权限修改和校验和
- 对密码登录账号提供受控 sudo 后备，支持管理受保护目录
- 文件操作后远端存在性复查，避免后端误报成功

## 安全边界

- 密码、passphrase 和私钥内容不写入 SQLite、应用日志或前端状态持久层。
- 所有真实 SSH/SFTP 连接均由 Rust 后端发起，并在后端再次执行主机密钥校验。
- 远程路径拒绝控制字符、相对路径穿越和危险根目录删除。
- sudo 密码仅从系统安全凭据临时读取，通过 SSH 标准输入发送。
- 日志按敏感标记整条脱敏，并限制单条长度和控制字符。
- 本地隧道默认且强制只监听 `127.0.0.1`、`::1` 或 `localhost`。
- `russh` 上游 RSA 实现仍受 `RUSTSEC-2023-0071` 影响，因此 v0.5.0 暂时禁用 RSA 私钥，待上游提供修复后再恢复。

## 隐私

- 应用不包含遥测、行为分析、广告 SDK 或自动上传功能。
- 除用户主动配置的 SSH/SFTP 服务器外，应用不会向第三方发送服务器资料、文件列表或凭据。
- 服务器配置保存在当前用户的应用数据目录；密码、passphrase 和私钥内容保存在操作系统安全凭据存储。
- 提交 Issue 前请删除日志中的主机名、IP、用户名、路径和其他环境信息。

## 功能对照

v0.5.0 已覆盖常用 SSH/SFTP 客户端的核心工作流。跳板机、SSH Agent、远程/动态转发、目录双向同步和命令片段库属于后续高级能力，不在本次“核心持平”范围。详细矩阵见 [功能与安全审计](docs/feature-audit.md)。

## 技术架构

- 桌面框架：Tauri v2
- 前端：React、TypeScript、Vite、Tailwind CSS、Zustand
- 后端：Rust、Tokio
- SSH/SFTP：`russh`、`russh-sftp`
- 终端：xterm.js
- 数据库：SQLite
- 凭据：Windows Credential Manager；其他平台使用系统安全凭据后端

## 开发与构建

```powershell
npm.cmd install
npm.cmd run check
npm.cmd run dev
```

桌面开发运行：

```powershell
npm.cmd run tauri:dev
```

构建桌面包和绿色版：

```powershell
npm.cmd run tauri:build
```

绿色版位于 `src-tauri/target/release/alax-ssh-manager.exe`，发布副本位于 `releases/`。

> 浏览器中的 `http://127.0.0.1:5173/` 仅用于前端预览。真实 SSH/SFTP、系统凭据、文件选择器和资源管理器调用需要运行 Tauri 桌面应用。

## 文档

- [功能与安全审计](docs/feature-audit.md)
- [技术方案](docs/technical-plan.md)
- [数据库设计](docs/database-design.md)
- [模块设计](docs/module-design.md)
- [完成报告](docs/completion-report.md)

## 参与贡献

请阅读 [贡献指南](CONTRIBUTING.md) 和 [行为准则](CODE_OF_CONDUCT.md)。安全问题请按照 [安全策略](SECURITY.md) 私下报告，不要创建公开 Issue。

## 许可证

本项目基于 [MIT License](LICENSE) 开源。
