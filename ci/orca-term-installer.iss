; orca-term Windows 安装包（Inno Setup 6）
; 沿用上游 wezterm ci/windows-installer.iss，改目标名并纳入 config-ui。
; 构建：iscc.exe -DMyAppVersion=<version> -FOrcaTerm-Setup ci\orca-term-installer.iss
; 前置：cargo build --release -p wezterm -p wezterm-gui 后，
;       将 config-ui\target\release\orca-term-config-ui.exe 复制到 target\release\
; vim:ts=2:sw=2:et:

#define MyAppName "OrcaTerm"
#define MyAppPublisher "orca-term"
#define MyAppURL "https://github.com/wezterm/wezterm"
#define MyAppExeName "orca-term-gui.exe"

[Setup]
; 全新 AppId：与上游 WezTerm 并存安装互不干扰
AppId={{7E3A2C91-6B44-4F5A-9D18-2A0C8B51E400}
ArchitecturesAllowed=x64 arm64
ArchitecturesInstallIn64BitMode=x64 arm64
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
AppUpdatesURL={#MyAppURL}
DefaultDirName={autopf}\{#MyAppName}
DisableProgramGroupPage=yes
OutputDir=..
OutputBaseFilename=OrcaTerm-Setup
SetupIconFile=..\assets\windows\terminal.ico
UninstallDisplayIcon={app}\{#MyAppExeName}
Compression=lzma
SolidCompression=yes
WizardStyle=modern
; Build 1809 is required for pty support
MinVersion=10.0.17763
ChangesEnvironment=true

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\target\release\orca-term.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\orca-term-gui.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\orca-term-config-ui.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\wezterm-mux-server.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\mesa\opengl32.dll"; DestDir: "{app}\mesa"; Flags: ignoreversion; Check: FileExists(ExpandConstant('{src}\..\target\release\mesa\opengl32.dll'))
Source: "..\target\release\libEGL.dll"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "..\target\release\libGLESv2.dll"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "..\target\release\conpty.dll"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "..\target\release\OpenConsole.exe"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "..\target\release\strip-ansi-escapes.exe"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
; NOTE: Don't use "Flags: ignoreversion" on any shared system files

[Icons]
Name: "{autoprograms}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; AppUserModelID: "dev.orcaterm.app"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon; AppUserModelID: "dev.orcaterm.app"

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent

[Registry]
Root: HKA; Subkey: "Software\Classes\Drive\shell\Open OrcaTerm here"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Drive\shell\Open OrcaTerm here"; ValueName: "icon"; ValueType: string; ValueData: "{app}\{#MyAppExeName}"; Flags: uninsdeletekey;
Root: HKA; Subkey: "Software\Classes\Drive\shell\Open OrcaTerm here\command"; ValueType: string; ValueData: """{app}\orca-term.exe"" start --no-auto-connect --cwd ""%V\"""; Flags: uninsdeletekey;
Root: HKA; Subkey: "Software\Classes\Directory\Background\shell\Open OrcaTerm here"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Directory\Background\shell\Open OrcaTerm here"; ValueName: "icon"; ValueType: string; ValueData: "{app}\{#MyAppExeName}"; Flags: uninsdeletekey;
Root: HKA; Subkey: "Software\Classes\Directory\Background\shell\Open OrcaTerm here\command"; ValueType: string; ValueData: """{app}\orca-term.exe"" start --no-auto-connect --cwd ""%V"; Flags: uninsdeletekey;
Root: HKA; Subkey: "Software\Classes\Directory\shell\Open OrcaTerm here"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Directory\shell\Open OrcaTerm here"; ValueName: "icon"; ValueType: string; ValueData: "{app}\{#MyAppExeName}"; Flags: uninsdeletekey;
Root: HKA; Subkey: "Software\Classes\Directory\shell\Open OrcaTerm here\command"; ValueType: string; ValueData: """{app}\orca-term.exe"" start --no-auto-connect --cwd ""%V\\"""; Flags: uninsdeletekey;

[Code]
{ https://stackoverflow.com/a/46609047/149111 }
const EnvironmentKey = 'SYSTEM\CurrentControlSet\Control\Session Manager\Environment';

const IMAGE_FILE_MACHINE_AMD64 = $8664;

function GetMachineTypeAttributes(
    Machine: Word; var MachineTypeAttributes: Integer): HRESULT;
  external 'GetMachineTypeAttributes@Kernel32.dll stdcall delayload';

function IsSupportedArch(): Boolean;
var
  Version: TWindowsVersion;
  MachineTypeAttributes: Integer;
  Arch: TSetupProcessorArchitecture;
begin
  GetWindowsVersionEx(Version);
  if Version.Build >= 22000 then
  begin
    OleCheck(
      GetMachineTypeAttributes(IMAGE_FILE_MACHINE_AMD64, MachineTypeAttributes)
    );
    Result := MachineTypeAttributes <> 0;
  end
  else
  begin
    if Version.Build >= 21277 then
    begin
      Result := True;
    end
    else
    begin
      Arch := ProcessorArchitecture;
      Result := Arch = paX64;
    end
  end;
end;

<event('InitializeSetup')>
function InitializeSetupCheckArchitecture(): Boolean;
begin
  Result := IsSupportedArch();
end;

procedure EnvAddPath(instlPath: string);
var
  Paths: string;
begin
  if not RegQueryStringValue(HKEY_LOCAL_MACHINE, EnvironmentKey, 'Path', Paths) then
    Paths := '';

  if Paths = '' then
    Paths := instlPath + ';'
  else
  begin
    if Pos(';' + Uppercase(instlPath) + ';',  ';' + Uppercase(Paths) + ';') > 0 then exit;
    if Pos(';' + Uppercase(instlPath) + '\;', ';' + Uppercase(Paths) + ';') > 0 then exit;

    if Paths[length(Paths)] <> ';' then
      Paths := Paths + ';';

    Paths := Paths + instlPath + ';';
  end;

  if RegWriteStringValue(HKEY_LOCAL_MACHINE, EnvironmentKey, 'Path', Paths)
  then Log(Format('The [%s] added to PATH: [%s]', [instlPath, Paths]))
  else Log(Format('Error while adding the [%s] to PATH: [%s]', [instlPath, Paths]));
end;

procedure EnvRemovePath(instlPath: string);
var
  Paths: string;
  P, Offset, DelimLen: Integer;
begin
  if not RegQueryStringValue(HKEY_LOCAL_MACHINE, EnvironmentKey, 'Path', Paths) then
    exit;

  DelimLen := 1;
  P := Pos(';' + Uppercase(instlPath) + ';', ';' + Uppercase(Paths) + ';');
  if P = 0 then
  begin
    DelimLen := 2;
    P := Pos(';' + Uppercase(instlPath) + '\;', ';' + Uppercase(Paths) + ';');
    if P = 0 then exit;
  end;

  if P = 1 then
    Offset := 0
  else
    Offset := 1;
  Delete(Paths, P - Offset, Length(instlPath) + DelimLen);

  if RegWriteStringValue(HKEY_LOCAL_MACHINE, EnvironmentKey, 'Path', Paths)
  then Log(Format('The [%s] removed from PATH: [%s]', [instlPath, Paths]))
  else Log(Format('Error while removing the [%s] from PATH: [%s]', [instlPath, Paths]));
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
    EnvAddPath(ExpandConstant('{app}'));
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
    EnvRemovePath(ExpandConstant('{app}'));
end;
