# OrcaTerm 内置提示符 —— 纯 shell 实现，无外部依赖。
# 由 OrcaTerm 注入到远端交互式 shell（bash 以 --rcfile 加载、zsh 以
# ZDOTDIR 指向的 .zshrc 加载），在用户自己的 rc 之后执行，因此最终
# 提示符以本脚本为准。远端机器不需要安装任何额外的程序。

# 输出 git 分支片段，形如 " (main*)"；不在 git 仓库中时输出空串。
__orca_git_info() {
  command -v git >/dev/null 2>&1 || return 0
  __orca_branch=$(git symbolic-ref --short -q HEAD 2>/dev/null) ||
    __orca_branch=$(git rev-parse --short HEAD 2>/dev/null) || return 0
  __orca_dirty=''
  if [ -n "$(git status --porcelain 2>/dev/null)" ]; then
    __orca_dirty='*'
  fi
  printf ' (%s%s)' "$__orca_branch" "$__orca_dirty"
  unset __orca_branch __orca_dirty
}

# 把当前目录中的 $HOME 缩写为 ~。
__orca_shortpwd() {
  case "$PWD" in
    "$HOME") printf '~' ;;
    "$HOME"/*) printf '~%s' "${PWD#"$HOME"}" ;;
    *) printf '%s' "$PWD" ;;
  esac
}

if [ -n "${ZSH_VERSION:-}" ]; then
  # ---------------- zsh ----------------
  __orca_precmd() {
    local __st=$?
    local __dir; __dir="$(__orca_shortpwd)"
    # 提示符展开会把 % 当作转义前缀，先把路径/分支里的 % 打断
    __dir="${__dir//%/%%}"
    local __git; __git="$(__orca_git_info)"
    __git="${__git//%/%%}"
    local __arrow='%F{cyan}❯%f'
    [ "$__st" -eq 0 ] || __arrow='%F{red}❯%f'
    PROMPT="%{${__orca_seq133a}$(printf '\033]133;D;%s\007' "$__st")}%F{green}%n@%m%f %F{blue}${__dir}%f%F{magenta}${__git}%f ${__arrow} "
  }
  __orca_preexec() {
    printf '\033]133;C\007'
  }
  __orca_seq133a=$'\033]133;A\007'
  if (( ! ${precmd_functions[(Ie)__orca_precmd]:-0} )); then
    precmd_functions+=(__orca_precmd)
  fi
  if (( ! ${preexec_functions[(Ie)__orca_preexec]:-0} )); then
    preexec_functions+=(__orca_preexec)
  fi
else
  # ---------------- bash ----------------
  # PS1 里的反斜杠转义（\e \u \h \[ \] \a）由 bash 在显示提示符时展开。
  __orca_prompt_command() {
    local __st=$?
    local __dir; __dir="$(__orca_shortpwd)"
    __dir="${__dir//\\/\\\\}"
    local __git; __git="$(__orca_git_info)"
    local __arrowcol='0;36'
    [ "$__st" -ne 0 ] && __arrowcol='0;31'
    PS1="\[\e]133;D;$__st\a\e]133;A\a\]\[\e[0;32m\]\u@\h\[\e[0m\] \[\e[0;34m\]${__dir}\[\e[0m\]\[\e[0;35m\]${__git}\[\e[0m\] \[\e[${__arrowcol}m\]❯\[\e[0m\] "
    PS0='\e]133;C\a'
  }
  # 追加到 PROMPT_COMMAND 末尾：用户已有的钩子先执行，随后由本函数
  # 覆盖 PS1，使远端提示符让位于 OrcaTerm 提示符。
  # 换行做分隔符，避免和用户已有的分号/井号内容拼接出语法问题。
  case "${PROMPT_COMMAND:-}" in
    *__orca_prompt_command*) ;;
    *) PROMPT_COMMAND="${PROMPT_COMMAND:+${PROMPT_COMMAND}
}__orca_prompt_command" ;;
  esac
fi
