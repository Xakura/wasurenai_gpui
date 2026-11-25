; Wasurenai Windows 安装脚本（Inno Setup 6，当前用户免 UAC 安装）

#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif

#ifndef ExePath
  #define ExePath "..\target\release\wasurenai_gpui.exe"
#endif

#ifndef OutputDir
  #define OutputDir "..\dist"
#endif

[Setup]
AppId={{8E6F5C41-3B9A-4D2E-9F10-7C8A2E4B6D91}
AppName=Wasurenai
AppVersion={#AppVersion}
AppVerName=Wasurenai {#AppVersion}
AppPublisher=Wasurenai
DefaultDirName={autopf}\Wasurenai
PrivilegesRequired=lowest
DisableProgramGroupPage=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir={#OutputDir}
OutputBaseFilename=Wasurenai-{#AppVersion}-setup
#ifdef AppIcon
SetupIconFile={#AppIcon}
#endif
UninstallDisplayIcon={app}\wasurenai_gpui.exe
UninstallDisplayName=Wasurenai
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
AppMutex=Local\WasurenaiGpuiSingleInstance
CloseApplications=yes
CloseApplicationsFilter=wasurenai_gpui.exe

[Languages]
Name: "chinesesimp"; MessagesFile: "compiler:Default.isl,ChineseSimplified.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "autostart"; Description: "开机自动启动 Wasurenai"; GroupDescription: "{cm:AutoStartProgramGroupDescription}"

[Files]
Source: "{#ExePath}"; DestDir: "{app}"; DestName: "wasurenai_gpui.exe"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Wasurenai"; Filename: "{app}\wasurenai_gpui.exe"
Name: "{autodesktop}\Wasurenai"; Filename: "{app}\wasurenai_gpui.exe"; Tasks: desktopicon

[Registry]
; 与 src/platform/autostart.rs 完全一致：HKCU Run 键值带双引号
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "Wasurenai"; ValueData: """{app}\wasurenai_gpui.exe"""; Flags: uninsdeletevalue; Tasks: autostart

[Run]
Filename: "{app}\wasurenai_gpui.exe"; Description: "安装完成后立即启动 Wasurenai"; Flags: nowait postinstall skipifsilent

[Code]
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  DataDir: string;
begin
  if CurUninstallStep = usUninstall then
  begin
    // 即使用户是安装后在应用设置界面里开启的自启，卸载时也一并清理 Run 注册表项
    RegDeleteValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'Wasurenai');
  end
  else if CurUninstallStep = usPostUninstall then
  begin
    if not UninstallSilent() then
    begin
      DataDir := ExpandConstant('{userappdata}\wasurenai_gpui');
      if DirExists(DataDir) then
      begin
        if MsgBox(
          '是否同时删除保存的待办数据与配置？' + #13#10 + DataDir,
          mbConfirmation,
          MB_YESNO or MB_DEFBUTTON2
        ) = IDYES then
          DelTree(DataDir, True, True, True);
      end;
    end;
  end;
end;
