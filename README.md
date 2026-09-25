# Lumia Launcher – 前端开发指南

> **本项目采用 Tauri + Vue 3 + TypeScript 架构。**  
> 前端代码由 AI 辅助生成，后端（Rust）由开发者单独实现。  
> 本指南专门为 AI 编程助手（如 Cursor、Copilot）提供上下文，以生成符合规范的前端代码。

---

## 📖 项目简介

Lumia Launcher 是一款跨平台的 Minecraft 启动器（Windows / macOS / Linux），前端使用 Vue 3 + TypeScript 构建，后端使用 Rust + Tauri 提供系统能力。  
**您只需要关注前端 UI 和交互逻辑**，所有系统调用（文件操作、进程管理、下载等）均通过 Tauri 的 `invoke` 调用后端命令实现。

---

## 🛠️ 前端技术栈

| 类别 | 技术 |
|------|------|
| 框架 | Vue 3 (Composition API + `<script setup>`) |
| 语言 | TypeScript |
| 构建工具 | Vite |
| 桌面桥接 | Tauri (调用 `@tauri-apps/api`) |
| 样式 | 原生 CSS（推荐配合 Tailwind CSS 或 UnoCSS 加速开发） |
| 状态管理 | 组件内 `ref`/`reactive`，全局简单状态可用 `provide/inject` 或 Pinia |
| 包管理器 | npm |

---

## 📁 前端项目结构（仅前端部分）

```
src/
├── assets/                # 静态资源（图片、字体、全局样式）
│   └── main.css
├── components/            # Vue 组件
│   ├── AppHeader.vue      # 头部（Logo、状态显示）
│   ├── HomeView.vue       # 主页（版本选择、启动、日志）
│   ├── DownloadView.vue   # 下载页（在线版本列表、下载进度）
│   ├── SettingsView.vue   # 设置页（Java 配置、内存、用户名）
│   ├── VersionSelector.vue # 版本下拉框（可复用）
│   └── LogDisplay.vue      # 日志显示区域（支持自动滚动）
├── composables/           # 组合式函数（可选）
│   └── useTauri.ts       # 封装 Tauri 调用（可选）
├── types/                 # TypeScript 类型定义
│   └── index.ts
├── utils/                 # 工具函数
│   └── tauri.ts           # 封装 Tauri invoke 调用
├── App.vue                # 根组件（布局、导航切换）
├── main.ts                # 入口文件
└── vite-env.d.ts
```

---

## 🧩 核心组件职责

### 1. `App.vue`
- 提供整体布局（顶部导航 + 内容区域）
- 维护当前显示的视图（`HomeView` / `DownloadView` / `SettingsView`）
- 管理全局状态（如当前选中的版本、用户名等，如果需要跨组件共享）

### 2. `HomeView.vue`
- 显示**已安装的版本列表**（从后端获取本地版本）
- 提供**玩家名输入框**
- **启动按钮**：调用 `launch_game` 命令
- **打开游戏目录按钮**：调用 `open_folder` 命令
- 显示**操作日志**（使用 `LogDisplay` 组件）

### 3. `DownloadView.vue`
- 显示**在线版本列表**（从后端获取所有可用版本）
- **下载按钮**：调用 `download_game` 命令
- 显示**下载进度**（通过进度条或文本反馈）
- 显示当前选中版本所需的 Java 版本（从后端获取）

### 4. `SettingsView.vue`
- **Java 路径管理**：
  - 自动检测 Java（调用 `get_java_paths` 显示列表）
  - 手动输入路径（输入框 + 浏览按钮）
- **Java 版本偏好**：下拉选择（自动 / 8 / 11 / 17 / 21 / 25）
- **最大内存分配**：滑动条或数字输入框
- **保存设置**：调用 `save_config` 保存所有配置
- 加载现有配置（调用 `get_config`）

### 5. `VersionSelector.vue`（可复用）
- 下拉选择版本
- 支持 `v-model` 绑定当前选中版本

### 6. `LogDisplay.vue`
- 显示日志列表（使用 `v-for`）
- 自动滚动到底部（使用 `watch` 监听新日志）

---

## 🔌 后端接口（Tauri 命令）

前端通过 `@tauri-apps/api/core` 的 `invoke` 调用以下命令。  
**请勿实现后端逻辑**，只需按照下方接口规范调用即可。

| 命令名 | 参数类型 | 返回值 | 说明 |
|--------|---------|--------|------|
| `get_versions` | 无 | `Promise<string[]>` | 获取所有可用游戏版本（在线） |
| `get_local_versions` | 无 | `Promise<string[]>` | 获取已下载到本地的游戏版本 |
| `download_game` | `{ version: string }` | `Promise<void>` | 下载指定版本（会发送进度事件，见下文） |
| `launch_game` | `{ version: string, username: string, java_path?: string, max_memory?: number }` | `Promise<void>` | 启动游戏（可覆盖配置） |
| `open_folder` | 无 | `Promise<void>` | 打开游戏目录 |
| `get_java_paths` | 无 | `Promise<string[]>` | 扫描系统 Java 可执行文件路径 |
| `set_java_path` | `{ path: string }` | `Promise<void>` | 保存 Java 路径到配置 |
| `get_config` | 无 | `Promise<{ username: string, java_path: string, max_memory: number, use_rosetta: boolean }>` | 读取当前配置 |
| `save_config` | `{ username: string, java_path: string, max_memory: number, use_rosetta: boolean }` | `Promise<void>` | 保存配置 |
| `get_required_java` | `{ version: string }` | `Promise<number>` | 获取某个版本所需的 Java 主版本号 |

### 进度事件
`download_game` 命令在执行过程中会通过 Tauri 的 **事件系统** 发送进度更新。前端需监听 `download-progress` 事件：

```typescript
import { listen } from '@tauri-apps/api/event';

const unlisten = await listen('download-progress', (event) => {
  const { progress, stage } = event.payload; // { progress: 0..100, stage: string }
  // 更新进度条或日志
});
```

---

## 🎨 样式与主题规范

- **主题**：深色背景 (`#1a1a2e`)，辅以亮色强调 (`#e94560`)。  
- **字体**：系统默认无衬线字体 (`-apple-system, 'Segoe UI', Roboto, sans-serif`)。  
- **排版**：使用 `flex`/`grid` 保证响应式布局。  
- **组件风格**：简洁、扁平、圆角。  
- **统一类名**：如果使用 Tailwind，请遵循官方约定；若使用原生 CSS，建议采用 BEM 命名。

### 示例颜色变量（在 `main.css` 中定义）

```css
:root {
  --bg-primary: #1a1a2e;
  --bg-secondary: #16213e;
  --bg-tertiary: #0f0f1a;
  --text-primary: #ffffff;
  --text-secondary: #a0aec0;
  --accent: #e94560;
  --success: #2ecc71;
  --warning: #f39c12;
  --border-color: #2d2d44;
}
```

---

## 📦 状态管理建议

- **组件内状态**：使用 `ref` / `reactive`。
- **跨组件共享**：若状态（如用户名、选中版本）在多个视图间共享，可将状态提升到 `App.vue` 并通过 `provide` / `inject` 或使用 **Pinia**（推荐）。
- **日志列表**：建议在 `App.vue` 中维护全局日志数组，通过 `provide` 提供给所有组件，方便统一管理。

---

## 🔧 工具函数（`utils/tauri.ts`）

统一封装 Tauri `invoke` 调用，方便类型提示。

```typescript
import { invoke } from '@tauri-apps/api/core';

export async function getVersions(): Promise<string[]> {
  return await invoke('get_versions');
}

export async function getLocalVersions(): Promise<string[]> {
  return await invoke('get_local_versions');
}

export async function downloadGame(version: string): Promise<void> {
  return await invoke('download_game', { version });
}

// ... 其余命令类似
```

---

## 🚀 开发流程

1. **启动开发服务器**（自动打开 Tauri 窗口）：
   ```bash
   npm run tauri dev
   ```
2. 修改 Vue 组件，保存后**热重载**立即生效。
3. 前端代码位于 `src/` 目录，后端 Rust 代码位于 `src-tauri/`（无需关心）。
4. 确保所有异步操作均有错误处理，并通过日志显示错误信息。

---

## 🤖 AI 辅助编程指南

作为 AI 助手，在生成前端代码时，请遵循以下原则：

- **组件生成**：优先使用 `<script setup>` + TypeScript。
- **命名约定**：组件文件使用 PascalCase（如 `HomeView.vue`），工具函数使用 camelCase。
- **错误处理**：所有 `invoke` 调用必须包含 `try/catch`，并将错误信息推入日志。
- **日志记录**：在每个重要操作前后（刷新、下载、启动）写入日志，格式为 `🔄 正在刷新...`、`✅ 刷新完成`、`❌ 错误信息`。
- **API 调用**：一律通过 `utils/tauri.ts` 导出的函数进行，不要在组件中直接 `invoke`。
- **UI 风格**：保持整体简洁、深色，符合 Lumia 品牌色调。
- **注释**：关键逻辑添加中文注释，便于理解和维护。

---

## 📚 参考资料

- [Tauri API 文档](https://tauri.app/reference/)
- [Vue 3 文档](https://vuejs.org/guide/introduction)
- [TypeScript 手册](https://www.typescriptlang.org/docs/)

---

**现在您可以开始编写或生成前端代码了！** 🚀
所有后端命令已定义清楚，您只需专注于 UI 和交互逻辑，后端实现由开发者完成。
重要的事情说三遍：您只需专注于前端 UI 和交互逻辑！您只需专注于前端 UI 和交互逻辑！您只需专注于前端 UI 和交互逻辑！