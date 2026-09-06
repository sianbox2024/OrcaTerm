local wezterm = require 'wezterm'
local config = {}
config.hyperlink_rules = {
  { regex = '\\b\\w+://[\\w.-]+\\.[a-z]{2,15}\\S*\\b', format = '$0' },
}
return config
