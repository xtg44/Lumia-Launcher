<div align="center">

<img src="icon.png" width="110" alt="Lumia Launcher" />

# Lumia Launcher

**跨平台 Minecraft 启动器** · Tauri 2 + Rust + Vue 3

[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-4c6ef5?style=flat-square)](#下载安装)
[![Version](https://img.shields.io/badge/version-1.0.0--beta.1-e94560?style=flat-square)](#下载安装)
[![Tauri](https://img.shields.io/badge/Tauri-2-24C8DB?style=flat-square&logo=tauri&logoColor=white)](https://tauri.app/)
[![Vue](https://img.shields.io/badge/Vue-3-42b883?style=flat-square&logo=vuedotjs&logoColor=white)](https://vuejs.org/)
[![Rust](https://img.shields.io/badge/Rust-stable-000000?style=flat-square&logo=rust&logoColor=white)](https://www.rust-lang.org/)

[官网](https://lumialauncher.cn) · [下载](https://lumialauncher.cn) · [插件语言文档](docs/lumi-reference.md) · [插件系统设计](docs/插件系统设计.md)

</div>

---

## 简介

Lumia Launcher 是一个用 **Rust + Tauri 2** 构建的轻量跨平台 Minecraft 启动器，界面使用 **Vue 3 + TypeScript**。

安装包体积小、启动快，内置完整的版本管理与资源下载能力，并提供一套自研插件语言 **Lumi**，让零编程基础的用户也能给启动器写扩展。

<!--
  截图占位：真实截图放在 .github/screenshots/ 或 docs/images/ 后取消注释即可
  <p align="center"><img src="docs/images/home.png" width="720" alt="主页" /></p>
-->

## 功能特性

### 🎮 游戏与版本
- **多版本管理** —— 原版及 **Forge / NeoForge / Fabric / Quilt** 加载器版本的安装、更新与删除
- **一键启动**，自动匹配合适的 Java 版本
- **Microsoft 账号登录**（完整的 MS → XBL → XSTS → Minecraft 令牌链）
- **联机支持**（基于 Terracotta）
- **崩溃报告**查看，启动异常可直接定位日志
- macOS 支持 **Touch Bar** 操作

### 📦 资源下载
- 版本、Mod、整合包、资源包、光影包一站式下载安装
- 接入 **BMCLAPI** 国内镜像，下载免代理
- 同时支持 **Modrinth** 与 **CurseForge** 生态

### 🎵 体验
- 内置**音乐播放器**与常驻播放条
- 内置 **AI 助手**面板
- **深色主题**，界面简洁扁平
- **7 种界面语言**：简体中文 / 繁體中文（中国台湾 / 中国香港）/ English / 日本語 / 한국어 / Français

### 🧩 插件系统（Lumi）
- 自研 **Lumi 插件语言**：类 Python 缩进语法、极简英文关键词，**零编程基础可读**
- `.lplugin` 插件包 = zip 改后缀，可含脚本与资源文件
- 事件驱动模型（`listen 控件.事件`），UI 由脚本解释出的控件树驱动
- 支持压缩包 / 文件夹 / 单文件三种安装形态
- 详见 [Lumi 语言完整参考](docs/lumi-reference.md) 与 [插件系统设计](docs/插件系统设计.md)

### 🔧 其他
- Java 路径自动扫描与手动指定、内存分配配置
- 应用内自动更新检查与更新日志

## 下载安装

前往官网 **[lumialauncher.cn](https://lumialauncher.cn)**，页面会自动识别你的平台并给出对应安装包：

| 平台 | 安装包 |
|---|---|
| Windows 10+ | `.exe` |
| macOS 10.15+ | `.dmg`（Apple Silicon / Intel） |
| Linux | 见官网说明 |

> 当前为 **v1.0.0-beta.1** 测试版，功能与接口可能变动，欢迎反馈问题。

## 技术栈

| 层 | 技术 |
|---|---|
| 桌面框架 | Tauri 2 |
| 后端 | Rust（约 12,000 行，6 个模块） |
| 前端 | Vue 3.5（Composition API + `<script setup>`）+ TypeScript 5.6 |
| 构建 | Vite 6 |
| 国际化 | vue-i18n 11 |
| 图标 / 渲染 | @iconify/vue、markdown-it、qrcode-generator |

Rust 后端模块划分：

| 模块 | 职责 |
|---|---|
| `main.rs` | 核心命令、版本解析、下载与启动逻辑 |
| `lumi.rs` | Lumi 插件语言解释器 |
| `plugins.rs` | 插件加载、注册与生命周期 |
| `music.rs` | 音乐播放 |
| `auth.rs` | Microsoft 登录与令牌刷新 |
| `touchbar.rs` | macOS Touch Bar 集成 |

## 项目结构

```
Lumia Launcher/
├── src/                  # Vue 3 前端
│   ├── components/       # 24 个视图与组件
│   ├── i18n/             # 7 种语言
│   ├── utils/            # tauri.ts 统一封装 invoke、svg.ts 等
│   └── App.vue
├── src-tauri/            # Rust 后端
│   ├── src/              # 6 个模块
│   ├── capabilities/     # Tauri 权限声明
│   ├── icons/            # 各平台图标
│   └── tauri.conf.json
├── docs/                 # 插件语言参考、插件系统设计、开发指南
├── public/               # 静态资源（图标、皮肤等）
└── package.json
```

## 本地开发

### 环境要求

- [Node.js](https://nodejs.org/) 18+
- [Rust](https://rustup.rs/) 稳定版
- 各平台 Tauri 系统依赖（见 [Tauri 前置要求](https://tauri.app/start/prerequisites/)）

### 启动

```bash
git clone https://github.com/xtg44/Lumia-Launcher.git
cd Lumia-Launcher
npm install
npm run tauri dev        # 启动开发窗口（前端热重载）
```

### 构建安装包

```bash
npm run tauri build                                    # 当前平台

# macOS 指定架构
rustup target add x86_64-apple-darwin                  # Intel
npm run tauri build -- --target x86_64-apple-darwin
npm run tauri build -- --target universal-apple-darwin # 通用包
```

产物位置：

| 平台 | 路径 |
|---|---|
| macOS | `src-tauri/target/<triple>/release/bundle/dmg/*.dmg` |
| Windows | `src-tauri/target/release/bundle/nsis/*.exe` |
| Linux | `src-tauri/target/release/bundle/{deb,appimage}/*` |

> Windows 上交叉编译可配合 [`cargo-xwin`](https://github.com/rust-cross/cargo-xwin)：`tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc`。

## 参与贡献

欢迎提交 Issue 与 Pull Request。

- Bug 反馈请附上**系统版本、启动器版本、复现步骤**，涉及启动失败时请附崩溃报告或日志
- 提交代码前请确保 `npm run build`（含 `vue-tsc` 类型检查）通过
- 新增界面文案请同步补齐 `src/i18n/` 下的语言文件

前端开发规范与后端命令接口说明见 [docs/前端开发指南.md](docs/前端开发指南.md)。

## 许可证

本项目采用 **MIT License** 发布，完整条款见 [LICENSE](LICENSE)。
Copyright (c) 2026 Lumia
你可以自由使用、修改、分发本项目，包括商业用途，只需保留版权声明与许可声明。。

---

<div align="center">

[Minecraft](https://www.minecraft.net/) 是 Mojang Studios 的商标。本项目与 Mojang Studios 及 Microsoft 无任何关联。

</div>
