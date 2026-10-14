# OrcaTerm 内置提示符 —— fish 版本，纯 fish 实现，无外部依赖。
# 由 OrcaTerm 经 fish 的 `-C 'source <本文件>'`（--init-command）注入：
# fish 先加载用户的 ~/.config/fish/config.fish，再执行 -C，因此最终
# 提示符以本脚本定义的 fish_prompt 为准。远端机器不需要安装任何程序。
#
# 与 bash/zsh 内置版（orca_prompt_builtin.sh）保持一致的视觉：
# 绿 user@host、蓝当前目录（$HOME 缩写为 ~）、洋红 git 分支、青/红箭头，
# 并输出 OSC 133 shell integration 序列（A=提示符起点、D=上条命令退出码、
# C=命令开始）。

# 输出 git 分支片段，形如 " (main*)"；不在 git 仓库中时输出空串。
function __orca_git_info
    command -q git; or return 0
    set -l b (git symbolic-ref --short -q HEAD 2>/dev/null)
    if test -z "$b"
        set b (git rev-parse --short HEAD 2>/dev/null)
    end
    test -z "$b"; and return 0
    set -l dirty ''
    if test -n (git status --porcelain 2>/dev/null | string collect)
        set dirty '*'
    end
    printf ' (%s%s)' "$b" "$dirty"
end

# 把 $pwd 中的 $home 缩写为 ~。用 string sub 做字面前缀判断，
# 避免 glob 元字符歧义，与 bash `case "$HOME"/*` 行为对齐。
function __orca_shortpwd -a pwd home
    if test "$pwd" = "$home"
        printf '~'
        return
    end
    set -l hl (string length -- "$home")
    if test (string sub --length $hl -- "$pwd") = "$home"
        printf '~%s' (string sub --start (math $hl + 1) -- "$pwd")
    else
        printf '%s' "$pwd"
    end
end

# user@host：$USER 兜底 id -un，hostname -s 取短名（对齐 bash \u@\h）。
function __orca_userhost
    set -l u "$USER"
    test -z "$u"; and set u (id -un 2>/dev/null)
    printf '%s@%s' "$u" (hostname -s 2>/dev/null)
end

# 命令开始前输出 OSC 133;C（对齐 bash PS0 / zsh preexec）。
function __orca_preexec --on-event fish_preexec
    printf '\e]133;C\a'
end

function fish_prompt
    # $status 必须在任何命令之前捕获，否则会被后续命令覆盖。
    set -l st $status
    set -l dir (__orca_shortpwd "$PWD" "$HOME")
    set -l git (__orca_git_info)
    set -l arrowcol cyan
    test $st -ne 0; and set arrowcol red

    # OSC 133:D 带上一条命令退出码，A 标记提示符起点。
    printf '\e]133;D;%s\a\e]133;A\a' $st
    printf '%s%s ' (set_color green) (__orca_userhost)
    set_color normal
    printf '%s%s ' (set_color blue) "$dir"
    set_color normal
    printf '%s%s ' (set_color magenta) "$git"
    set_color normal
    printf '%s❯%s ' (set_color $arrowcol)
    set_color normal
end
