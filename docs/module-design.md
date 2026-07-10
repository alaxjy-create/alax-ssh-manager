# 前后端模块设计

## 前端模块

```text
src/
  components/       基础 UI 组件
  layout/           主布局、侧边栏、顶部栏、状态栏
  server/           服务器列表、服务器表单、连接测试入口
  terminal/         终端标签页与 xterm.js 容器
  file-manager/     远程文件管理器
  transfer/         传输任务面板
  settings/         设置页与日志入口
  pages/            页面组合
  stores/           Zustand 状态
  hooks/            主题、Tauri 命令等 Hooks
  lib/              工具函数、Tauri 调用封装
  types/            共享 TypeScript 类型
```

## Rust 后端模块

```text
src-tauri/src/
  commands/         前端可调用命令
  config/           应用路径、运行配置
  credentials/      系统凭据存储抽象
  db/               SQLite 初始化、迁移、查询
  logs/             日志写入与脱敏
  sftp/             SFTP 文件操作，第四阶段实现
  ssh/              SSH 连接与终端通道，第三阶段实现
  transfer/         传输队列，第五阶段实现
  state.rs          Tauri 全局状态
  lib.rs            应用入口与命令注册
  main.rs           Tauri 启动入口
```

## 前后端通信

前端通过 Tauri `invoke` 调用 Rust 命令。命令返回结构化数据，错误统一返回可读提示。第一阶段提供：

- `get_app_status`
- `initialize_database`
- `list_servers`
- `list_groups`
- `get_log_directory`

后续阶段增加：

- `create_server`
- `update_server`
- `delete_server`
- `test_connection`
- `open_terminal`
- `sftp_read_dir`
- `sftp_upload`
- `sftp_download`
- `transfer_cancel`
- `transfer_retry`

## 安全边界

- 前端不接触明文密码，最多只提交一次性输入。
- Rust 命令接收敏感值后立即写入系统凭据存储。
- SQLite 只保存 `credential_ref` 和 `private_key_ref`。
- 日志模块在写入前统一脱敏。
- 导出功能默认不包含敏感信息。
