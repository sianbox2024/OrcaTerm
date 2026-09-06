local wezterm = require 'wezterm'
local config = {}
local function make_launcher(label)
  return function(window, pane)
    window:perform_action(wezterm.action.SpawnCommandInNewTab { label = label }, pane)
  end
end
wezterm.on('format-tab-title', function(tabs)
  return make_launcher('x')(nil, nil) or tostring(tabs.tab_index)
end)
return config
