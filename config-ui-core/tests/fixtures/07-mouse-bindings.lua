local wezterm = require 'wezterm'
local act = wezterm.action
local config = {}
config.mouse_bindings = {
  {
    event = { Down = { streak = 1, button = 'Right' } },
    mods = 'NONE',
    action = act.PasteFrom 'Clipboard',
  },
}
return config
