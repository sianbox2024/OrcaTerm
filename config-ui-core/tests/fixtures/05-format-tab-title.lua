local wezterm = require 'wezterm'
local config = {}
wezterm.on('format-tab-title', function(tabs, _tabs_panes, _cfg, _hover, _max_width)
  local title = tabs.tab_title or tostring(tabs.tab_index + 1)
  return ' ' .. title .. ' '
end)
return config
