local wezterm = require 'wezterm'
local config = {}
wezterm.on('new-tab-button-click', function(window, pane, button, default_action)
  if button == 'Left' then
    window:perform_action(default_action, pane)
  end
end)
config.inactive_pane_hsb = { saturation = 0.85, brightness = 0.75 }
return config
