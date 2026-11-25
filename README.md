# Wasurenai

常驻桌面的轻量待办与强提醒工具，基于 Rust 与 [GPUI](https://www.gpui.rs/)（`gpui-kit`）构建。

日常工作繁杂时，普通待办软件很容易被切到后台遗忘。Wasurenai 将待办清单直接常驻在桌面底层，并在需要时通过全局快捷键或到期强提醒置顶呼出，同时自动将已完成事项整理为本地 Markdown 工作日志。

## 核心特性

- **双层级桌面常驻面板**
  - **桌面底层（Bottom）**：始终位于普通窗口之下，按下 `Win+D` 显示桌面时依然可见，且不占用任务栏图标。
  - **全局快捷键上浮（Top）**：随时一键将面板置顶到所有窗口之上查看或勾选，默认 `Ctrl+Alt+Shift+W`（可在设置页录制修改）。
  - **防误触与流畅动效**：勾选完成原地停留 220ms（支持反悔取消）后平滑下移至已完成区；支持收起/展开抽屉动效与上下层切换动效。
- **到期强提醒**
  - 到点后自动将面板切换为提醒视图并强制置顶，需手动点击「知道了」/「全部知道了」或勾选完成后才会回落桌面层。
- **浅色卡片风编辑窗口**
  - **待办页**：顶部固定快速添加栏（支持日历 + 时分选择与快捷预设、预定义多标签勾选），下方分「进行中」与「今日已完成」展示；昨日及更早完成的事项自动隐藏归档。
  - **日志页**：按日期分组预览全部已完成工作，支持一键打开日志文件、在文件管理器中定位目录或导出 Markdown 副本。
  - **设置页**：预定义工作标签管理、全局快捷键录制、开机自启开关与本地存储路径展示。
- **自动 Markdown 工作日志**
  - 所有待办与配置以原子写入（`.tmp` + `rename`）保存在本地 JSON；每次落盘自动同步生成按日期与完成时间排序的 `completed_log.md`。
- **系统集成**
  - 系统托盘常驻（动态 tooltip 与未完成/逾期状态角标）、单实例守卫、开机自启、Explorer 重启自动恢复桌面层。

## 平台支持与架构

当前完整支持 **Windows 11**，代码已按跨平台抽象层解耦，便于后续移植至 macOS：

```text
src/
├── main.rs                 应用入口、事件循环与跨天刷新
├── lib.rs                  模块导出与单向依赖边界
├── core/                   纯领域逻辑（零 UI / OS 依赖，可独立单测）
│   ├── model.rs            TodoItem / Store 数据结构与旧版 JSON 兼容迁移
│   ├── store.rs            原子持久化与增删改
│   ├── ordering.rs         排序规则、标签聚合与当日可见性过滤
│   ├── reminder.rs         到期判定纯函数
│   ├── timeparse.rs        宽容截止时间解析与格式化
│   ├── hotkey.rs           全局快捷键解析与规范化
│   └── worklog.rs          Markdown 工作日志生成与原子同步
├── platform/               跨平台系统集成抽象层（对外零 OS 类型泄漏）
│   ├── mod.rs              统一门面：NativeWindow / WindowLayer / Event / Command
│   ├── icon.rs             纯跨平台 SDF 矢量抗锯齿图标渲染
│   ├── windows/            Win32 实现（Progman owner、子类化、DWM、托盘、热键、注册表）
│   ├── macos/              macOS 后端占位桩（预留 AppKit / NSStatusItem / LaunchAgent 接口）
│   └── unsupported/        其他平台兜底实现
└── ui/                     GPUI 界面层（零 Win32 / Cocoa 直接依赖）
    ├── state.rs            AppState（防抖落盘、强提醒确认、日志同步）
    ├── theme.rs            桌面面板（深色）与编辑器（浅色）两套配色 token
    ├── windows.rs          窗口生命周期与层级状态机
    ├── widget.rs           桌面常驻面板与提醒视图
    ├── panel_checkbox.rs   桌面面板手绘勾选框
    └── editor/             编辑窗口（待办 / 日志 / 设置三页签及共用组件）
packaging/
└── windows/                Inno Setup 6 中文安装包脚本与一键构建脚本
```

## 快速开始

### 环境要求

- Rust 工具链（支持 `edition = "2024"`）
- Windows 11（`x86_64-pc-windows-msvc`）

### 运行与测试

```bash
# 开发模式运行
cargo run

# 运行单元测试
cargo test

# 静态检查
cargo clippy --all-targets -- -D warnings
```

### 构建 Windows 安装包

需先安装 [Inno Setup 6](https://jrsoftware.org/isinfo.php)，然后执行：

```powershell
pwsh -File packaging/windows/build.ps1
```

脚本会自动编译 Release 二进制、通过 `build.rs` 生成多分辨率 `.ico` 图标，并在 `dist/` 下产出免 UAC 的当前用户安装向导 `Wasurenai-<version>-setup.exe`。

## 本地数据位置

默认位于系统用户数据目录下的 `wasurenai_gpui` 文件夹（Windows 为 `%APPDATA%\wasurenai_gpui\`）：

- `todos.json`：待办列表、预定义标签与快捷键配置
- `completed_log.md`：自动同步的已完成工作 Markdown 日志
