#ifndef AppVersion
  #error Run Build-Release.ps1 to supply the release version and paths.
#endif

[Setup]
AppId={{B97FA13A-0923-4C24-A6D5-7F5276293F19}
AppName=ADraw
AppVersion={#AppVersion}
AppPublisher=ADraw contributors
AppPublisherURL=https://github.com/mikehodgson/adraw
AppSupportURL=https://github.com/mikehodgson/adraw/issues
AppUpdatesURL=https://github.com/mikehodgson/adraw/releases
DefaultDirName={localappdata}\Programs\ADraw
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir={#OutputDir}
OutputBaseFilename=adraw-{#AppVersion}-windows-x64-setup
SetupIconFile={#ProjectRoot}\crates\ansi_editor\assets\adraw.ico
UninstallDisplayIcon={app}\adraw.exe
VersionInfoVersion={#FileVersion}
VersionInfoProductVersion={#AppVersion}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
CloseApplications=no
RestartApplications=no

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\ADraw"; Filename: "{app}\adraw.exe"
Name: "{autodesktop}\ADraw"; Filename: "{app}\adraw.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\adraw.exe"; Description: "Launch ADraw"; Flags: nowait postinstall skipifsilent
