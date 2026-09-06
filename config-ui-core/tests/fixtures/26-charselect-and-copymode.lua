local wezterm = require 'wezterm'
local act = wezterm.action
local config = {}
config.keys = {
  { key = 'U', mods = 'CTRL|SHIFT', action = act.CharSelect { copy_on_select = true, copy_to = 'ClipboardAndPrimarySelection' } },
}
return config
