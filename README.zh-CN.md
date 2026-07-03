# KeyPulse 键盘频率仪

KeyPulse 是一个注重隐私的键盘活动统计桌面应用，支持 macOS 和 Windows。

它用于观察你的打字节奏、快捷键使用情况和每日键盘活跃度，但不会保存你实际输入的文字、原始按键序列、密码，普通字母和数字也不会以具体字符落盘。

## 功能特性

- 实时显示最近一分钟敲击频率
- 统计今日总敲击次数
- 记录分钟峰值
- 按小时展示今日键盘活动节奏
- 统计 Enter、Backspace、Tab、Esc、方向键、功能键、修饰键等按键类别
- 统计 Cmd+C、Ctrl+V 等快捷键组合次数
- macOS 关闭主窗口后自动常驻顶部菜单栏
- 只在本机保存聚合统计数据

## 隐私说明

KeyPulse 的目标是“只看节奏，不看内容”。

- 不保存输入文本
- 不保存原始按键序列
- 不保存密码
- 普通字母和数字只按类别计数
- 统计数据保存在本机

## macOS 权限

macOS 需要开启“输入监控”权限，KeyPulse 才能监听全局键盘事件。

打开：

```text
系统设置 > 隐私与安全性 > 输入监控
```

允许 KeyPulse 后，如果应用内仍显示“待生效”，请重启 KeyPulse。重新安装应用后，可能需要把 KeyPulse 的开关关闭再打开一次，让 macOS 重新绑定权限。

## Windows 说明

MVP 版本的架构支持 Windows 构建。部分安全软件可能会拦截全局键盘事件监听，需要手动允许 KeyPulse 运行。

## 本地开发

依赖：

- Node.js
- Rust
- 当前平台所需的 Tauri 构建依赖

安装依赖：

```bash
npm install
```

启动开发模式：

```bash
npm run tauri:dev
```

构建前端：

```bash
npm run build
```

打包桌面应用：

```bash
npm run tauri -- build
```

## 项目结构

```text
keypulse-mac/
├── src/                 # React 渲染层
├── src-tauri/           # Tauri 与 Rust 原生层
├── scripts/             # 工具脚本
├── package.json
└── README.md
```

## 开源协议

MIT
