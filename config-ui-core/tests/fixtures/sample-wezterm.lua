-- 模拟真实社区配置的复杂度样本：回调、辅助函数、条件逻辑、特殊空白
local wezterm = require 'wezterm'
local act = wezterm.action

local function leading_trailing_space()   
  return "  padded  "
end

local config = {}

-- 字体规则含嵌套结构
config.font_rules = {
  {
    italic = true,
    font = wezterm.font('JetBrains Mono', { weight = 'Medium', italic = true }),
  },
}

config.font_size = 11.5   
config.enable_tab_bar = true

-- 平台条件逻辑
if wezterm.target_triple:find('windows') then
  config.default_prog = { 'pwsh.exe', '-NoLogo' }
else
  config.default_prog = { '/bin/zsh' }
end

-- 事件回调（GUI 管理区外的典型用户代码）
wezterm.on('format-tab-title', function(tabs)
  local title = tabs[1].active_pane.title
  return ' ' .. title .. ' '
end)

config.keys = {
  { key = 'c', mods = 'CTRL|SHIFT', action = act.CopyTo 'ClipboardAndPrimarySelection' },
  { key = 'v', mods = 'CTRL|SHIFT', action = act.PasteFrom 'Clipboard' },
}

config.color_scheme = 'Tokyo Night'

--[[ 长字符串注释
     return 出现在这里不应干扰顶层 return 识别
]]
config.window_padding = {
  left = '1cell',
  right = '1cell',
}

return config
