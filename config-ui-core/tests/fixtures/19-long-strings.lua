local wezterm = require 'wezterm'
local config = {}
config.keys = {
  {
    key = 'E',
    mods = 'CTRL|SHIFT',
    action = wezterm.action.PromptInputLine {
      description = 'Enter name',
      action = wezterm.action_callback(function(window, pane)
        local name = ''
        -- [[ not a long string here ]]
        if name ~= '' then
          pane:split()
        end
      end),
    },
  },
}
return config
