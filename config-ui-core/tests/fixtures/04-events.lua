local wezterm = require 'wezterm'
local config = {}
wezterm.on('window-config-reloaded', function(window, pane)
  window:toast_notification('orca-term', 'config reloaded', nil, 4000)
end)
return config
