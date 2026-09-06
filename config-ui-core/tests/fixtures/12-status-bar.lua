local wezterm = require 'wezterm'
local config = {}
wezterm.on('update-right-status', function(window, _pane)
  window:set_right_status(window:active_workspace())
end)
return config
