# ALAX SSH Manager v0.5.0 完成报告

## 完成范围

- 真实 SSH 密码/私钥认证、主机密钥验证、交互式 PTY 终端和 keepalive
- 真实 SFTP 文件与目录管理、递归删除、重命名、压缩、预览和文本编辑
- 文件/目录上传下载、系统选择器、后台进度、取消、重试和断点续传
- 原子替换、冲突检测、上传后复查、权限修改和校验和
- 密码登录账号的受控 sudo 后备
- 本地 SSH 端口转发与持久规则
- 服务器监控、日志、设置、系统安全凭据和 SQLite 持久化
- CSP、资源上限、日志脱敏、路径校验和数据库事务加固

## 验证结果

- TypeScript 类型检查通过
- Vite 生产构建通过
- Cargo 格式检查通过
- Clippy `-D warnings` 通过
- Rust 单元测试通过
- npm 生产依赖离线审计通过
- RustSec `cargo audit` 通过：0 个漏洞；保留 17 条上游维护状态警告，其中 GTK 系列属于非 Windows 目标依赖
- Tauri Windows release 构建通过
- Windows 绿色版启动与真实 SSH/SFTP 行为验证通过

## 发布产物

- `releases/alax-ssh-manager-x64-0.5.0.exe`

## 说明

浏览器地址 `http://127.0.0.1:5173/` 是前端预览，不具备 Tauri 系统能力。实际连接、系统凭据、文件选择器、资源管理器和绿色版验证均以桌面应用为准。

详细功能矩阵与剩余高级能力见 [功能与安全审计](feature-audit.md)。
