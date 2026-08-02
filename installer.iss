[Setup]
AppName=Green Timer
AppVersion=1.0.0
DefaultDirName={autopf}\GreenTimer
DefaultGroupName=Green Timer
UninstallDisplayIcon={app}\GreenTimer.exe
OutputDir=Output
OutputBaseFilename=GreenTimer_Setup
Compression=lzma
SolidCompression=yes
PrivilegesRequired=admin

[Files]
Source: "dist\GreenTimer.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Green Timer"; Filename: "{app}\GreenTimer.exe"
Name: "{autodesktop}\Green Timer"; Filename: "{app}\GreenTimer.exe"

[Run]
Filename: "{app}\GreenTimer.exe"; Description: "Launch Green Timer"; Flags: nowait postinstall skipifsilent