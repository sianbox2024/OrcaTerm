local wezterm = require 'wezterm'
local act = wezterm.action
local config = {}
config.keys = {
  { key = 'c', mods = 'CTRL|SHIFT', action = act.CopyTo 'ClipboardAndPrimarySelection' },
  { key = 'v', mods = 'CTRL|SHIFT', action = act.PasteFrom 'Clipboard' },
}
return config
