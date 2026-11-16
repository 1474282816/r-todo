#define MyAppName "rtools"
#define MyAppVersion "0.2.0"
#define MyAppPublisher "renrunqi"
#define MyAppExeName "RTools.Desktop.exe"
#define SourceDir "..\..\..\..\artifacts\desktop-win-x64"

[Setup]
AppId={{8F97366E-60BA-4D7E-9C51-6D4B5A0D5701}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
DefaultDirName={localappdata}\Programs\rtools
DefaultGroupName=rtools
PrivilegesRequired=lowest
OutputDir=..\..\..\..\artifacts\release
OutputBaseFilename=rtools-{#MyAppVersion}-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
UninstallDisplayIcon={app}\{#MyAppExeName}
SetupIconFile=..\src\RTools.Desktop\Assets\rtools.ico
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible

[Tasks]
Name: "desktopicon"; Description: "创建桌面快捷方式"; GroupDescription: "快捷方式："; Flags: unchecked
Name: "startup"; Description: "开机自动启动 rtools"; GroupDescription: "启动选项："; Flags: unchecked

[Files]
Source: "{#SourceDir}\RTools.Desktop.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\rtools-engine.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\rtools"; Filename: "{app}\{#MyAppExeName}"
Name: "{autodesktop}\rtools"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "启动 rtools"; Flags: nowait postinstall skipifsilent

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "rtools"; ValueData: """{app}\{#MyAppExeName}"" --autostart"; Flags: uninsdeletevalue; Tasks: startup
