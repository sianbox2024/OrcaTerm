local wezterm = require 'wezterm'
local act = wezterm.action
local config = {}
config.key_tables = {
  copy_mode = {
    { key = 'Escape', mods = 'NONE', action = act.CopyMode 'Close' },
  },
  search_mode = {
    { key = 'Enter', mods = 'NONE', action = act.ActivateCopyMode },
  },
}
return config
