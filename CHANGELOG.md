# Changelog

## v0.1.0 (2026-08-26)

首个 orca-term 版本。基于 WezTerm 分叉，核心差异化能力为**内置 GPUI 图形化配置界面**。

### 新增

- **图形配置界面（`orca-term-config-ui`）**
  - 七组导航：字体与光标 / 配色 / 窗口外观 / 标签栏 / 启动与默认行为 / 键绑定 / 高级
  - 内置配色方案网格：300+ 方案、搜索过滤、点选即换（与 `wezterm.color.get_builtin_schemes` 同源）
  - 键绑定编辑器：用户绑定增删改、20 个常用 action 子集、按键捕获录入、冲突高亮、默认键位只读对照
  - 高级面板：原始 Lua 查看（行号）、整个文件/仅标记区双模式、语法校验按钮、外部编辑器打开
  - 热重载闭环：notify 监听配置文件（2s 防抖）自动刷新；未保存改动冲突横幅提示
  - 单实例运行
- **CLI 子命令**：`orca-term config-ui [--config PATH]`
- **`ShowConfigUI` KeyAssignment**：可在 Lua 中绑快捷键唤起配置界面；同时进入命令面板
- **品牌化**：`orca-term.version` 返回 `orca-term <tag> (based on WezTerm)`；二进制 `orca-term.exe` / `orca-term-gui.exe`

### 配置管理核心

- `<orca-gui-config-start/end>` 标记区双向同步：GUI 只重写标记区，区外内容逐字保留
- 存量配置引导迁移：自动识别 `local config = {} … return config` 惯用形；
  `return {}` / `return c` 型文件自动改写顶层 return 保证 GUI 赋值生效
- 保存前滚动备份（保留 5 份）
- default-diff 发射：仅写与基线不同的字段

### 质量

- config-ui-core：51 个单元测试 + 往返/fixture/性能三组集成测试
- 26 个社区常见风格配置样本库（tests/fixtures），全部通过读链路解析 + 标记区零丢失往返
- 性能验收：保存发射 < 100ms；读快照 < 2s；GUI 冷启动 < 2s（debug 构建 1.79s 实测）

### 许可

基于 WezTerm（MIT）分叉，遵守上游许可条款。
