-- OrcaTerm 随包默认配置（构建时随 dist 分发；可手动编辑，也可由配置界面
-- 保存，支持热重载）。母本为用户实机配置：PowerShell 启动菜单经
-- starship\ 子目录启用 starship 提示符；SSH 连接的私钥路径、连接后命令
-- 等字段按目标机器实际情况修改。
local wezterm = require 'wezterm'
local config = wezterm.config_builder()

config.mouse_bindings = {
  {
    event = { Down = { streak = 1, button = 'Right' } },
    mods = 'NONE',
    action = wezterm.action_callback(function(window, pane)
      local sel_text = window:get_selection_text_for_pane(pane)
      if sel_text ~= nil and #sel_text > 0 then
        window:perform_action(wezterm.action.CopyTo 'Clipboard', pane)
        window:perform_action(wezterm.action.ClearSelection, pane)
      else
        window:perform_action(wezterm.action.PasteFrom 'Clipboard', pane)
      end
    end),
  },
}
config.color_scheme = 'Atlas (base16)'
config.keys = { { key='Enter', mods='ALT', action=wezterm.action.DisableDefaultAssignment }, { key='RightArrow', mods='ALT', action=wezterm.action.SplitHorizontal({domain='CurrentPaneDomain',elevate=false,set_environment_variables={}}) } }
config.ssh_domains = {
  { name='Debian', remote_address='192.168.0.129', multiplexing='None', username='sb', ssh_option={ identityfile='D:\\Users\\Anran\\Documents\\Debian\\id_ed25519' }, default_prog={'sh','-c','cd ~/myprojects; exec $SHELL'}, sftp_command='D:\\Program Files (x86)\\WinSCP\\WinSCP.exe' },
  { name='MSI', remote_address='192.168.0.164', multiplexing='None', username='sb', ssh_option={ identityfile='D:\\Users\\Anran\\Documents\\MSI\\id_ed25519' }, default_prog={'sh','-c','cd ~/myprojects/AQIM; exec $SHELL'}, sftp_command='D:\\Program Files (x86)\\WinSCP\\WinSCP.exe' },
}
config.launch_menu = {
  { label='新CMD窗口', args={'cmd.exe'}, domain='DefaultDomain' },
  { label='新PowerShell 7窗口', args={'D:\\Tools\\PowerShell\\7\\pwsh.exe', '-NoLogo', '-NoExit', '-Command', '$env:STARSHIP_CONFIG=\'D:\\Tools\\OrcaTerm\\starship\\starship.toml\'; $env:STARSHIP_CACHE=\'D:\\Tools\\OrcaTerm\\starship\\starship-cache\'; Invoke-Expression (& \'D:\\Tools\\OrcaTerm\\starship\\starship.exe\' init powershell | Out-String)'}, domain='DefaultDomain' },
  { label='新管理员CMD窗口', args={'cmd.exe'}, elevate=true },
  { label='新管理员PowerShell 7窗口', args={'D:\\Tools\\PowerShell\\7\\pwsh.exe', '-NoLogo', '-NoExit', '-Command', '$env:STARSHIP_CONFIG=\'D:\\Tools\\OrcaTerm\\starship\\starship.toml\'; $env:STARSHIP_CACHE=\'D:\\Tools\\OrcaTerm\\starship\\starship-cache\'; Invoke-Expression (& \'D:\\Tools\\OrcaTerm\\starship\\starship.exe\' init powershell | Out-String)'}, elevate=true },
  { label='SSH连接（Debian）', domain={ DomainName='Debian' } },
  { label='SSH连接（MSI）', domain={ DomainName='MSI' } },
}
wezterm.on('format-tab-title', function(tab, tabs, panes, config, hover, tab_max_width)
  local title = tab.tab_title
  if #title == 0 then
    title = tab.active_pane.title
  end
  local pane = tab.active_pane
  if pane and pane.is_elevated then
    title = '(管理员)' .. title
  end
  if #title == 0 then
    title = '终端'
  end
  -- orca:tab-colors
  if tab.is_active then
    local hues = {'Red', 'Lime', 'Yellow', 'Blue', 'Fuchsia', 'Aqua'}
    local bg = hues[(tab.tab_id % #hues) + 1]
    return {
      { Background = { AnsiColor = bg } },
      { Foreground = { AnsiColor = 'Black' } },
      { Text = ' ' .. title .. ' ' },
    }
  end
  return {
    { Text = title },
  }
end)
return config
