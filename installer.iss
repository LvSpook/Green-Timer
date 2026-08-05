[Setup]
; --- Application Details ---
AppName=Green Timer
AppVersion=1.0.0
AppPublisher=Luke V
; Default installation folder
DefaultDirName={autopf}\Green Timer
DefaultGroupName=Green Timer

; --- Output Settings ---
; Where the final Setup.exe will be saved
OutputDir=C:\Users\lukev\Desktop\Green Timer source\Output
OutputBaseFilename=GreenTimer_Setup_v1.0.0
Compression=lzma
SolidCompression=yes

; This allows the app to install without requiring Admin permissions
PrivilegesRequired=lowest

[Tasks]
; Creates a checkbox during install for a desktop shortcut
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
; Grabs your working .exe from the dist/main folder and puts it in the installation folder
Source: "C:\Users\lukev\Desktop\Green Timer source\dist\main\main.exe"; DestDir: "{app}"; Flags: ignoreversion

; Grabs ALL the extra DLLs and folders next to the executable and packages them together
Source: "C:\Users\lukev\Desktop\Green Timer source\dist\main\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
; Creates the start menu icon
Name: "{group}\Green Timer"; Filename: "{app}\main.exe"
; Creates the desktop icon
Name: "{autodesktop}\Green Timer"; Filename: "{app}\main.exe"; Tasks: desktopicon

[Run]
; Gives the user an option to launch the app immediately after installing
Filename: "{app}\main.exe"; Description: "{cm:LaunchProgram,Green Timer}"; Flags: nowait postinstall skipifsilent