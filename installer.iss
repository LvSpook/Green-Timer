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

; *** NEW: This sets the icon of the SETUP.EXE installer itself ***
SetupIconFile=C:\Users\lukev\Desktop\Green Timer source\green_timer.ico

[Tasks]
; Creates a checkbox during install for a desktop shortcut
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
; Grabs your working executable from the dist/main folder
Source: "C:\Users\lukev\Desktop\Green Timer source\dist\main\main.exe"; DestDir: "{app}"; Flags: ignoreversion

; *** NEW: This copies the .ico file into the installation folder so the shortcuts can find it ***
Source: "C:\Users\lukev\Desktop\Green Timer source\green_timer.ico"; DestDir: "{app}"; Flags: ignoreversion

; Grabs ALL the extra DLLs and folders next to the executable and packages them together
Source: "C:\Users\lukev\Desktop\Green Timer source\dist\main\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
; *** MODIFIED: These lines now use the IconFilename parameter to point to the copied icon ***

; Creates the start menu icon with the custom icon
Name: "{group}\Green Timer"; Filename: "{app}\main.exe"; IconFilename: "{app}\green_timer.ico"

; Creates the desktop icon with the custom icon
Name: "{autodesktop}\Green Timer"; Filename: "{app}\main.exe"; Tasks: desktopicon; IconFilename: "{app}\green_timer.ico"

[Run]
; Gives the user an option to launch the app immediately after installing
Filename: "{app}\main.exe"; Description: "{cm:LaunchProgram,Green Timer}"; Flags: nowait postinstall skipifsilent