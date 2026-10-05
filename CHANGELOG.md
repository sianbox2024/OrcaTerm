# Changelog

## 未发布

### 新增

- **SSH 提示符注入（远端零安装获得 starship 提示符）**
  - 动机：提示符由远端 shell 打印，终端只负责显示；远端没装 starship 时，
    任何终端都只能显示默认 PS1。OrcaTerm 现在会在建立 SSH 连接时自动把
    提示符逻辑注入到远端交互式 shell，远端机器无需安装任何东西。
  - 开关：`ssh_inject_prompt`（全局）/ `inject_prompt`（每个 ssh_domains
    条目，覆盖全局），三档（与上游枚举配置一致，首字母大写）：
    - `"Off"`（默认）：不注入，行为与上游 WezTerm 一致；
    - `"Builtin"`：注入内置的纯 shell 提示符（用户@主机、短路径、git 分支
      及脏标记、退出码变色），bash/zsh 通用，零外部依赖；
    - `"Starship"`：把静态编译的 starship 二进制上传到远端
      `~/.cache/orcaterm/bin/` 并以 rcfile 方式启用，远端得到 100% 真正
      的 starship（远端 `~/.config/starship.toml` 照常生效）。
  - 本地 starship 二进制解析顺序：显式配置的 `ssh_starship_binary_path` /
    `starship_binary_path` → 程序同目录 `dist/starship/`（build.sh 打包时
    自动下载附带，离线环境开箱即用）→ 本机 PATH → 本地缓存 → GitHub
    Releases 下载（校验官方 sha256）；本机二进制必须是与远端架构匹配的
    Linux ELF，否则自动跳过（避免把 macOS/Windows 二进制传上去跑不起来）。
  - 注入机制：探测远端 `$SHELL/$HOME/架构` → SFTP 上传（不可用时退化为
    exec+base64）→ 以 `bash --rcfile` / `zsh ZDOTDIR` 启动交互式 shell，
    注入的 rc 会先 source 用户原有的 rc；内置提示符还输出 OSC 133 标记，
    为后续终端侧命令块渲染预留。
  - 安全回退链：仅内置 SSH 客户端且未显式指定远端命令时注入；远端
    登录 shell 不是 bash/zsh、SFTP/base64 上传失败、starship 下载失败
    （自动降级为 builtin）等任何一步失败，都回退为远端默认提示符，
    不影响连接本身。

  ```lua
  -- 全局开启 starship 注入
  config.ssh_inject_prompt = "Starship"
  -- 也可按域覆盖：
  -- config.ssh_domains = { { name = "dev", remote_address = "dev.example.com",
  --                         inject_prompt = "Builtin" } }
  ```

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
