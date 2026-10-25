; Windows installer of Astrolabe. Built by .github/workflows/release-binaries.yml:
;   ASTROLABE_VERSION=0.1.0 iscc packaging/windows/astrolabe.iss   (makes dist/astrolabe-windows-x86_64-setup.exe)
#define Version GetEnv("ASTROLABE_VERSION")

[Setup]
AppId={{6F2C8B14-3D5E-4A97-B0C1-7E9A42D5F318}
AppName=Astrolabe
AppVersion={#Version}
AppPublisher=Andrea Larboullet Marin
AppPublisherURL=https://github.com/alarboulletmarin/astrolabe
DefaultDirName={autopf}\Astrolabe
DefaultGroupName=Astrolabe
; Installed for the current user only (%LOCALAPPDATA%\Programs\Astrolabe): no administrator prompt.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
MinVersion=10.0
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
SetupIconFile=astrolabe.ico
UninstallDisplayIcon={app}\astrolabe.exe
LicenseFile=..\..\LICENSE
OutputDir=..\..\dist
OutputBaseFilename=astrolabe-windows-x86_64-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
DisableProgramGroupPage=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "french"; MessagesFile: "compiler:Languages\French.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; Flags: unchecked

[Files]
Source: "..\..\target\release\astrolabe.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\assets\fonts\LICENSE-IBM-Plex.txt"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Astrolabe"; Filename: "{app}\astrolabe.exe"
Name: "{autodesktop}\Astrolabe"; Filename: "{app}\astrolabe.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\astrolabe.exe"; Description: "{cm:LaunchProgram,Astrolabe}"; Flags: nowait postinstall skipifsilent
