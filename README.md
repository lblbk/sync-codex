# Codex 远程模型同步

一个轻量工具：读取本地 [CC Switch](https://cc-switch.cc) 的模型列表，通过 SFTP 把 JSON 同步到远程 Linux 主机，供远程 Codex 环境使用。

技术栈：**Vite 7 + Svelte 5 + Tauri 2 + Rust（Tauri 命令 / reqwest / russh）**

## 功能

- 代理本地 CC Switch API（`http://127.0.0.1:15721/v1/models`）获取模型列表
- 一键把模型 JSON 流式上传到远程主机（SFTP，无需临时文件）
- 远程文件内容预览（读取上限 2MB，纯 SFTP 读取，无 shell 命令拼接）
- 配置保存在浏览器 localStorage，可一键恢复默认值
- 操作日志与状态徽章（空闲 / 同步中 / 成功 / 失败）

## 配置使用

1. cc-switch 开启路由功能，注意端口号设置！

2. ssh 中增加端口配置，如果需要转发，一定确认好转发端口, 下图为例 15721 cc-switch 端口，15722 默认端口
```
Host humble
  HostName 192.168.xx.xxx
  User humble
  RemoteForward 15722 127.0.0.1:15721
```

3. 配置好远程 codex 文件, 核心 base_url model_catalog_json 两个字段

```toml
model_provider = "cc-switch-proxy"
model = "grok-4.6"
disable_response_storage = true
model_reasoning_effort = "max"
model_catalog_json = "cc-switch-model-catalog.json"

[model_providers.cc-switch-proxy]
name = "cc switch proxy"
wire_api = "responses"
base_url = "http://127.0.0.1:15722/v1"
api_key = "proxy-placeholder"
```

4. 配置完成后远程 codex 和本地一样，此时使用本程序开启同步

```bash
npm install                # 安装前端与 Tauri CLI 依赖
npm run dev                # 只启动 Vite 前端（不包含后端命令，浏览器内不可调接口）
npm run build              # 构建前端产物到 dist/
npm run tauri:dev          # Tauri 开发模式：自动启动 Vite + Rust 本地服务
npm run tauri:build        # 打包桌面应用安装包
```

- 桌面应用开发/打包：`npm run tauri:dev` / `npm run tauri:build`

## 打包说明

- 打包类型由 `src-tauri/tauri.conf.json` 的 `bundle.targets` 决定。当前为 `["nsis"]`，会生成 Windows 的 `*-setup.exe` 安装包；Windows 下可选值还有 `"msi"`，或使用 `"all"` 打包当前平台支持的全部类型。
- 架构不写在 `tauri.conf.json`，由 Rust 的目标三元组决定，例如 `x86_64-pc-windows-msvc` 会生成 `_x64-setup.exe`。
- 命令行可以临时覆盖：`npx tauri build --bundles nsis`、`npx tauri build --bundles msi nsis`；只生成可执行文件可以加 `--no-bundle`。
- Windows 便携模式：在可执行文件旁创建 `sync-codex-data` 文件夹后，WebView2 数据会保存在其中；没有该文件夹时仍使用原来的本机数据目录。可在退出程序后将 `%LOCALAPPDATA%\com.local.sync-codex\EBWebView` 复制为 `sync-codex-data\EBWebView`，以保留现有配置。已有的 `WEBVIEW2_USER_DATA_FOLDER` 环境变量优先。此目录可能包含本机配置和密码，请勿上传或公开发布。

## 目录结构

```
src/
  main.js     # 入口
  App.svelte  # 主界面
  app.css     # 全局样式
src-tauri/
  Cargo.toml  # Rust 依赖与 Tauri 配置
  tauri.conf.json
  src/
    api.rs    # Tauri 命令：模型代理、同步、对比、心跳
    ssh.rs    # russh/russh-sftp 封装：上传与读取（纯 Rust SSH 栈）
    models.rs # 模型摘要
    watcher.rs # 自动同步调度
vite.config.js  # Vite 配置（Tauri dev 模式使用的本地开发服务器）
```

## 说明

- SSH 密码仅保存在应用本机 localStorage，用于自动执行同步；如需更高安全性，建议改用密钥认证。
- 远程文件默认路径 `/home/<user>/.codex/cc-switch-model-catalog.json`，可在界面中修改。
- SSH 后端使用 `russh`（纯 Rust）+ `russh-sftp`，不依赖 `ssh2`/`libssh2-sys`/`libz-sys`。唯一的 C 依赖是 `ring`，构建时需要 `.cargo/config.toml`（已在仓库内）规避 MSVC `cl.exe` 的 `D8050` 错误。
