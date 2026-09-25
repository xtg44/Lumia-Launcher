# Lumi 插件语言完整参考

Lumi 是 Lumia Launcher 的插件语言。类 Python 缩进语法，事件驱动，零编程基础可读。

---

## 1. 文件头（元信息）

每个 `main.lumi` 文件必须以元信息开头，每行一个 `键: 值`：

```
name: 插件名称
version: 1.0
description: 一句话描述（可选）
icon: icon.png        （可选，图标文件名）
min: 1.0              （可选，最低启动器版本要求）
```

- `name` 和 `version` **必须**写，否则插件无法识别。
- `description` 显示在插件中心。
- `icon` 是插件目录内的图片文件名（png/svg）。未声明时，侧边栏默认显示 `plugin.svg`。
- `min` 限制最低启动器版本，不满足的版本不加载。

---

## 2. 控件类型

### 2.1 按钮 `CreateButton`

```
名字 = CreateButton('按钮文字')
```

- 支持事件：`click`
- 支持布局：`.left` `.center` `.right`
- 可动态修改文字：`名字 = '新文字'`

### 2.2 文本 `CreateText`

```
名字 = CreateText('文本内容')
```

- 支持事件：`click`
- 支持布局：`.left` `.center` `.right`
- 可动态修改内容：`名字 = '新内容'`

### 2.3 输入框 `CreateInput`

```
名字 = CreateInput('占位符文字')
```

- 支持事件：`input`（每次输入时触发）、`change`（失去焦点/确认时触发）
- 支持布局：`.left` `.center` `.right`
- 可动态修改占位符：`名字 = '新占位符'`

### 2.4 开关 `CreateToggle`

```
名字 = CreateToggle('开关标签')
```

- 支持事件：`check`（开启）、`uncheck`（关闭）
- 支持布局：`.left` `.center` `.right`
- 条件判断：`名字.checked` —— 是否为选中状态
- 可动态修改标签：`名字 = '新标签'`

### 2.5 图片 `CreateImage`

```
名字 = CreateImage('图片文件名.png')
```

- 支持事件：`click`
- 图片文件放在插件目录内
- 支持布局：`.left` `.center` `.right`

### 2.6 分割线 `CreateLine`

```
名字 = CreateLine()         # 横向分割线，贯穿整行
名字 = CreateLine(300)      # 长度 300px 的横线
名字 = CreateLine(300, True) # 长度 300px 的竖线
```

- 无事件
- 无布局

### 2.7 下拉列表 `CreateList`

```
名字 = CreateList('选项1', '选项2', '选项3')
```

- 支持事件：`change`（选择变化时触发）
- 条件判断：
  - `名字 is select` —— 是否有选中项
  - `名字.selected` —— 获取当前选中项内容
  - `名字.selected == '选项1'` —— 判断选中项是否等于某值
- 支持布局：`.left` `.center` `.right`

---

## 3. 布局

所有控件默认**左对齐**。通过以下方式设置：

| 语法 | 效果 |
|------|------|
| `名字.left` | 左对齐 |
| `名字.center` | 居中 |
| `名字.right` | 右对齐 |

```
MyBtn = CreateButton('点击')
MyBtn.center
```

---

## 4. 修改控件文字

直接赋值语法，支持大部分控件的文字修改：

```
名字 = '新文字'
```

支持的控件：按钮、文本、输入框（改占位符）、开关（改标签）。

```lumi
Btn = CreateButton('开始')
Btn = '准备就绪'     # 按钮文字变为"准备就绪"
```

---

## 5. 条件判断 `if`

### 5.1 条件表达式

| 条件 | 说明 | 示例 |
|------|------|------|
| `A == B` | A 等于 B | `名字.selected == '选项1'` |
| `A != B` | A 不等于 B | `名字 != '某个值'` |
| `名字 is select` | 列表有选中项 | `MyList is select` |
| `名字 is not select` | 列表无选中项 | `MyList is not select` |
| `名字.checked` | 开关处于选中状态 | `MyToggle.checked` |

### 5.2 写法

**单行括号式（推荐）：**

```lumi
if 名字.selected == '备份'(toast('选中了备份'); sleep 1)
```

**缩进块式：**

```lumi
if 名字.selected == '备份':
    toast('选中了备份')
    sleep 1
else:
    toast('选中了其他')
```

- 支持 `else`，不支持 `elif`（嵌套 `if` 代替）。
- 单行括号内多个动作用分号 `;` 分隔。
- **两种写法不能在同一 `if` 里混用。**

---

## 6. 循环

### 6.1 无限循环 `while`

```lumi
while:
    toast('每 50ms 执行一次')
    if 开关.checked:
        toast('开关开着')
```

- **只在顶层（文件最外层）生效**，每轮执行一遍 + 自动 sleep 50ms。
- 块内的 `while` 只执行一遍。
- 使用 `break` 退出循环。

### 6.2 遍历循环 `for`

**遍历列表控件：**

```lumi
for item in MyList:
    toast(item)
```

**遍历字面量列表：**

```lumi
for item in ('a', 'b', 'c'):
    toast(item)
```

**数字区间：**

```lumi
for i in 0..10:        # 0, 1, 2, ..., 9（左闭右开）
    toast(i)
```

```lumi
for i in 1..5:         # 1, 2, 3, 4
    copy 'file' + i to backup-folder
```

**带步长：**

```lumi
for i in 0..10 step 2: # 0, 2, 4, 6, 8
    toast(i)
```

- `break` 立即退出循环。
- 无 `continue`（用 `if` 包裹替代）。

---

## 7. 事件监听 `listen`

### 7.1 控件事件

| 事件 | 适用控件 | 触发时机 |
|------|---------|----------|
| `click` | 按钮、文本、图片 | 被点击 |
| `input` | 输入框 | 每次输入内容时 |
| `change` | 输入框、下拉列表 | 值发生变化（失去焦点/选择变化） |
| `check` | 开关 | 开启 |
| `uncheck` | 开关 | 关闭 |

**单行括号式：**

```lumi
listen MyBtn.click(toast('被点击了'))
listen MyInput.input(Status = '正在输入')
```

- 逗号分隔多个动作：`listen Btn.click(动作1, 动作2)`

### 7.2 系统事件

| 事件名 | 触发时机（P0 已注册通道） |
|--------|--------------------------|
| `game_quit` | 游戏退出 |
| `game_start` | 游戏启动 |
| `game_crash` | 游戏崩溃 |
| `version_install` | 版本安装完成 |
| `version_download_start` | 版本下载开始 |
| `version_download_finish` | 版本下载完成 |
| `download_finish` | 下载任务完成 |
| `download_fail` | 下载任务失败 |
| `app_launch` | 启动器启动 |
| `app_close` | 启动器关闭 |
| `terracotta_join` | 加入联机房间 |
| `music_toggle` | 音乐播放/暂停切换 |

```lumi
listen game_quit(copy game-saves to backup-folder, popup('游戏已退出，已自动备份'))
listen game_start(toast('游戏启动了！'))
```

---

## 8. 动作库

每个动作都是 Rust 侧实现的安全白名单操作。出错时显示中文错误 toast，不崩溃启动器。

### 8.1 文件操作

| 动作 | 说明 | 示例 |
|------|------|------|
| `copy 源 to 目标` | 复制文件/目录 | `copy game-saves to backup-folder` |
| `move 源 to 目标` | 移动文件/目录 | `move 'old.zip' to backup-folder` |
| `delete 路径` | 删除文件/目录 | `delete 'temp-folder'` |
| `mkdir 路径` | 创建目录（支持多级） | `mkdir 'logs/2024'` |
| `exists 路径` | 判断路径是否存在，结果弹出 toast | `exists game-saves` |
| `listFiles 路径` | 列出目录内容，结果弹出 toast | `listFiles plugin-dir` |

### 8.2 系统操作

| 动作 | 说明 | 示例 |
|------|------|------|
| `open 路径` | 用系统默认程序打开文件/目录 | `open backup-folder` |
| `launchGame` | 启动当前版本游戏（P1 实现） | `launchGame` |
| `sleep 秒数` | 等待 n 秒（支持小数） | `sleep 1.5` |

### 8.3 通知

| 动作 | 说明 | 示例 |
|------|------|------|
| `toast('文字')` | 右下角轻提示（3 秒自动消失） | `toast('备份完成')` |
| `popup('文字')` | 弹窗对话框（用户点确定关闭） | `popup('操作成功！')` |

---

## 9. 内置名词（无需写完整路径）

| 名词 | 含义 | 实际路径 |
|------|------|----------|
| `game-dir` | 游戏根目录 | `<应用数据>/`  |
| `game-saves` | 当前版本存档目录 | `<应用数据>/saves` |
| `backup-folder` | 插件自己的备份目录 | `<插件目录>/backup`（自动创建） |
| `plugin-dir` | 插件自己的目录 | `<应用数据>/plugins/<插件id>/` |
| `today` | 当天日期 |（P1 替换为可读日期） |
| `versions-dir` | 版本目录 | `<应用数据>/versions` |
| `launcher-dir` | 启动器数据根目录 | `<应用数据>/` |
| `music-dir` | 音乐库目录 | `<应用数据>/music_library` |

自定义路径使用字符串：`copy 'saves/myworld' to backup-folder`。

### 路径安全

- 内置名词已经过 canonicalize 校验，防止 `../` 逃逸。
- 自定义路径只能写入插件自身目录或内置名词范围内。
- 写 `game-dir` 之外的任意路径默认拒绝。

---

## 10. 页面注入 `page`

插件可以向**已有页面**注入控件，或修改已有控件的文字。

### 10.1 语法

```lumi
page 页面名:
    控件名 = CreateButton('文字')   # 注入新控件
    已有控件名 = '新文字'           # 覆盖已有控件文字
    控件名.center                   # 设置布局
```

### 10.2 支持的页面

| 页面名 | 对应界面 |
|--------|---------|
| `home` | 主页（启动游戏 / 登录界面） |

### 10.3 可覆盖的已有控件名

#### `page home:` — 主页

| 控件名 | 原始内容 | 类型 |
|--------|---------|------|
| `greeting` | "早上好/下午好/晚上好, {用户名}!" | 文本 |
| `start_btn` | "开始游戏" | 按钮 |
| `start_sub` | 版本号 | 文本 |

```lumi
page home:
    greeting = '欢迎回来，冒险家！'
    start_btn = '开始冒险'
    start_sub = '准备好了吗？'
    QuickBtn = CreateButton('快速备份')
    QuickBtn.center
```

---

## 11. 注释与语法规则

- **注释**：`#` 或 `//` 直到行尾。
- **缩进**：4 空格（Tab 也接受，统一按空格解释）。
- **字符串**：单引号或双引号均可。
- **数字**：整数；`.` 已被关键字占用，浮点数仅 `sleep` 动作支持。
- **没有**：函数定义、变量类型声明、算术运算（除字符串拼接 `+`）、复杂控制流。
- **分号**：仅在单行括号式中分隔多个动作：`动作1; 动作2`。
- **大小写**：控件名、事件名**区分大小写**。

---

## 12. 文件结构

### 压缩包格式（推荐）

```
my-plugin.lplugin        (zip 改后缀)
├── main.lumi            （必需）
├── icon.png             （可选，图标）
└── ...                  （可选，图片等资源）
```

### 文件夹格式

```
plugins/
└── my-plugin/
    ├── main.lumi
    ├── icon.png
    └── ...
```

### 单文件格式

```
plugins/
└── my-plugin.lumi       （单文件，无需子目录）
```

- 单文件因无资源/无图标，不能过插件市场审核，但**本地加载不拦截**。
- 单文件插件侧边栏图标默认显示 `plugin.svg`。

### 放入位置

macOS: `~/Library/Application Support/Lumia/plugins/`

放入后，打开启动器 → 插件中心 → 点击「刷新」即可识别。

---

## 13. 完整示例

### 示例 1：自动备份存档

```lumi
name: 自动备份
version: 1.0

BackupBtn = CreateButton('立即备份')
BackupBtn.center
Status = CreateText('等待中')

listen BackupBtn.click(copy game-saves to backup-folder, Status = '备份完成', toast('备份完成'))

listen game_quit(copy game-saves to backup-folder, popup('游戏已退出，自动备份完成'))
```

### 示例 2：自定义首页

```lumi
name: 自定义首页
version: 1.0

page home:
    greeting = '欢迎回来，冒险家！'
    start_btn = '开始冒险'
    start_sub = '准备好了吗？'
    QuickBtn = CreateButton('快速备份')
    QuickBtn.center
```

### 示例 3：游戏事件监控

```lumi
name: 事件监控
version: 1.0

Status = CreateText('等待游戏启动')

listen game_start(Status = '游戏运行中', toast('游戏已启动'))

listen game_quit(Status = '等待游戏启动', toast('游戏已关闭'))
```

### 示例 4：下拉列表 + 条件判断

```lumi
name: 备份管理
version: 1.0

ActionList = CreateList('备份存档', '还原存档', '删除备份')
ActionList.center
Status = CreateText('请选择操作')

listen ActionList.change(
    Status = ActionList.selected,
    toast(ActionList.selected)
)
```

### 示例 5：开关 + 循环监控

```lumi
name: 自动监控
version: 1.0

AutoToggle = CreateToggle('自动备份')
AutoToggle.center
Status = CreateText('自动备份：关闭')
Count = CreateText('0 次备份')

while:
    if AutoToggle.checked:
        Status = '自动备份：运行中'
        copy game-saves to backup-folder
        sleep 60
    else:
        Status = '自动备份：关闭'
    sleep 0.1
```

### 示例 6：输入框 + 目录操作

```lumi
name: 目录浏览
version: 1.0

DirInput = CreateInput('输入目录名')
DirInput.center
Btn = CreateButton('查看')
Btn.center
Result = CreateText('')

listen Btn.click(exists backup-folder, listFiles plugin-dir)
```

---

## 14. 错误处理

- 解释器每「动作/块」包裹错误边界。
- 出错时显示**中文错误 toast**，例如：`第 12 行：备份目录不存在`。
- 单个插件出错**不影响其他插件和启动器正常运行**。
- 语法错误在加载阶段捕获，运行错误在事件触发时捕获。

常见错误：
- `第 N 行：字符串没有闭合引号` —— 引号不配对
- `第 N 行：无法识别的字符 'X'` —— 非法字符
- `复制失败："xxx" 不存在` —— 路径不存在
- `第 N 行：未知控件/动作/事件` —— 拼写错误

---

## 15. VSCode 扩展

`vscode-lumi/` 提供编辑体验：

- **语法高亮**：关键词、控件类型、字符串、注释
- **自动补全**：`CreateButton` 等控件、动作词、内置名词、事件名
- **诊断**：未知控件/动作/事件、缩进错误
- **命令 `Lumi: 打包`**：当前文件夹一键打包为 `.lplugin`

---

## 16. 速查表

### 控件速查

| 创建语法 | 类型 | 可修改文字? |
|----------|------|:---:|
| `X = CreateButton('t')` | 按钮 | ✓ |
| `X = CreateText('t')` | 文本 | ✓ |
| `X = CreateInput('p')` | 输入框 | ✓ |
| `X = CreateToggle('t')` | 开关 | ✓ |
| `X = CreateImage('f')` | 图片 | ✗ |
| `X = CreateLine()` | 横线 | ✗ |
| `X = CreateLine(n)` | 横线 n px | ✗ |
| `X = CreateLine(n, True)` | 竖线 n px | ✗ |
| `X = CreateList('a','b')` | 下拉列表 | ✗ |

### 事件速查

| 事件 | 写法 |
|------|------|
| 点击 | `listen X.click()` |
| 输入 | `listen X.input()` |
| 变化 | `listen X.change()` |
| 选中 | `listen X.check()` |
| 取消选中 | `listen X.uncheck()` |
| 游戏退出 | `listen game_quit()` |
| 游戏启动 | `listen game_start()` |

### 动作速查

| 动作 | 写法 |
|------|------|
| 复制 | `copy A to B` |
| 移动 | `move A to B` |
| 删除 | `delete A` |
| 打开 | `open A` |
| 弹窗 | `popup('文字')` |
| 提示 | `toast('文字')` |
| 等待 | `sleep 1` |
| 创建目录 | `mkdir A` |
| 检查存在 | `exists A` |
| 列出文件 | `listFiles A` |
| 启动游戏 | `launchGame` |