# 插件系统

Koid 插件运行于 **iframe 沙箱**（`sandbox="allow-scripts"`），通过 postMessage 与主应用桥接。

## 目录结构

```
app_data_dir/plugins/{id}/
├── manifest.json
└── index.html          # 入口（manifest.entry 指定）
```

## manifest.json

```json
{
  "name": "hello",
  "version": "0.1.0",
  "entry": "index.html",
  "permissions": ["notify", "llm", "storage", "file", "command", "network"]
}
```

权限未声明时全部放行（兼容）；声明后桥接层逐方法校验。

## 安装

- **本地 zip**：插件设置页 → 本地 zip（原生文件对话框）
- **远程 URL**：插件设置页 → 远程安装（走主进程代理下载）

zip 需包含 `manifest.json` 与入口文件。

## 命令面板

插件通过 `koid.command.register` 注册命令，可用 **Cmd/Ctrl+K** 全局唤起命令面板执行。

详见 [插件 API 参考](/reference/plugin-api)。
