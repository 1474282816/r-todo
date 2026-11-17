#define AppName "RTools"
#define AppVersion "0.3.0"
#define SourceDir "E:\artifacts\release-final-tauri"

[Setup]
AppId={{A86B6A4D-6B9E-4EE5-AB6C-72F169D9013A}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=RTools
DefaultDirName={localappdata}\Programs\RTools
DefaultGroupName=RTools
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir=E:\artifacts\release-final-tauri
OutputBaseFilename=RTools_0.3.0_x64-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
UninstallDisplayIcon={app}\rtools-desktop-tauri.exe

[Files]
Source: "{#SourceDir}\rtools-desktop-tauri.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\rtools-engine.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{userprograms}\RTools"; Filename: "{app}\rtools-desktop-tauri.exe"
Name: "{userdesktop}\RTools"; Filename: "{app}\rtools-desktop-tauri.exe"; Tasks: desktopicon

[Tasks]
Name: "desktopicon"; Description: "创建桌面快捷方式"; Flags: unchecked

[Run]
Filename: "{app}\rtools-desktop-tauri.exe"; Description: "启动 RTools"; Flags: nowait postinstall skipifsilent
