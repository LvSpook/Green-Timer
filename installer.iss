; Green Timer - Inno Setup script
; Build the app first:  powershell -ExecutionPolicy Bypass -File build.ps1
; (or: cargo build --release, then copy target\release\GreenTimer.exe plus
; green_timer.ico into dist\GreenTimer\)
; Then compile this file with Inno Setup 6.
;
; Paths are relative to this script, so the project can be moved or cloned
; anywhere without editing them.

#define AppName "Green Timer"
#define AppVersion "1.2.0"
#define AppPublisher "Luke V"
#define AppExeName "GreenTimer.exe"

[Setup]
AppId={{8F3C2B71-5A4D-4E2A-9C19-5E6F7A8B9C01}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher={#AppPublisher}
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
UninstallDisplayIcon={app}\{#AppExeName}
OutputDir=Output
OutputBaseFilename=GreenTimer_Setup_v{#AppVersion}
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
; Must match the app: it installs per-user and never needs elevation.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
SetupIconFile=green_timer.ico
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
DisableProgramGroupPage=yes
CloseApplications=yes
RestartApplications=no

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "startupicon"; Description: "Start Green Timer when I sign in"; GroupDescription: "Startup:"; Flags: unchecked

[Files]
Source: "dist\GreenTimer\{#AppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "dist\GreenTimer\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "green_timer.ico"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExeName}"; IconFilename: "{app}\green_timer.ico"
Name: "{group}\Uninstall {#AppName}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExeName}"; IconFilename: "{app}\green_timer.ico"; Tasks: desktopicon
Name: "{userstartup}\{#AppName}"; Filename: "{app}\{#AppExeName}"; Tasks: startupicon

[Run]
Filename: "{app}\{#AppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(AppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; Settings live in %APPDATA% and are intentionally preserved on uninstall.
Type: filesandordirs; Name: "{app}"
