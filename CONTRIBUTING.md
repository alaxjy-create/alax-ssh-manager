# Contributing to ALAX SSH Manager

感谢你参与 ALAX SSH Manager。请先创建 Issue 说明较大的功能或架构改动，修复明确 Bug 可直接提交 Pull Request。

## 开发环境

- Node.js 22
- Rust stable，包含 `rustfmt` 和 `clippy`
- Windows 10/11 与 WebView2 Runtime

```powershell
npm.cmd ci
npm.cmd run check
npm.cmd run build
cd src-tauri
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
```

桌面开发运行：

```powershell
npm.cmd run tauri:dev
```

## Pull Request 要求

- 保持改动聚焦，不混入无关重构或生成文件。
- 为新增后端逻辑补充测试；修复 Bug 时尽量添加回归测试。
- 不提交真实服务器地址、用户名、密码、私钥、日志或本地数据库。
- 不降低主机密钥校验、路径校验、CSP、凭据存储和日志脱敏边界。
- 确保 CI 中的 TypeScript、Rust、Clippy 和测试全部通过。

向本项目提交代码即表示你同意该贡献按 MIT License 发布。
