-- 变量名不是 config 的样本：GUI 迁移需把顶层 return 改写为 local config = c
local c = {}
c.font_size = 15.0
c.scrollback_lines = 10000
local helper = function() return 42 end
c.default_prog = { 'powershell.exe', '-NoLogo' }
return c
