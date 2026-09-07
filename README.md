# Codex 远程模型同步

一个轻量工具：读取本地 [CC Switch](https://cc-switch.cc) 的模型列表，通过 SFTP 把 JSON 同步到远程 Linux 主机，供远程 Codex 环境使用。

技术栈：**Vite 7 + Svelte 5 + Node 原生 http / ssh2**

## 功能

- 代理本地 CC Switch API（`http://127.0.0.1:15721/v1/models`）获取模型列表
- 一键把模型 JSON 流式上传到远程主机（SFTP，无需临时文件）
- 远程文件内容预览（读取上限 256KB，纯 SFTP 读取，无 shell 命令拼接）
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
npm install        # 安装依赖
npm run dev        # 开发模式（Vite dev server，端口 8000）
npm run build      # 构建产物到 dist/
npm start          # 生产模式（静态文件 + API，端口 8001）
```

- 开发模式：浏览器打开 `http://127.0.0.1:8000`
- 生产模式：浏览器打开 `http://127.0.0.1:8001`

> 生产模式依赖 `dist/` 产物，改动源码后请先 `npm run build` 再 `npm start`。

## 目录结构

```
server/
  index.js    # 生产 HTTP 服务器：静态文件 + API 路由
  api.js      # API 处理：模型代理、同步、远程查看
  ssh.js      # SSH2/SFTP 封装：上传与读取
src/
  main.js     # 入口
  App.svelte  # 主界面
  app.css     # 全局样式
vite.config.js  # Vite 配置（dev 模式挂载 /api 中间件）
```

## 说明

- SSH 密码仅保存在本地浏览器 localStorage，用于自动执行同步；如需更高安全性，建议改用密钥认证。
- 远程文件默认路径 `/home/<user>/.codex/cc-switch-model-catalog.json`，可在界面中修改。