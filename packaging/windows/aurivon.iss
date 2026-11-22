; Inno Setup script for Aurivon (Windows x86_64).
; Built by scripts/package-windows.ps1 — expects MyAppVersion and SourceDir defines.

#ifndef MyAppVersion
  #define MyAppVersion "0.0.0"
#endif
#ifndef SourceDir
  #define SourceDir "..\..\dist\aurivon-stage"
#endif

#define MyAppName "Aurivon"
#define MyAppPublisher "Alteian"
#define MyAppURL "https://github.com/Alteian/aurivon"
#define MyAppExeName "aurivon.exe"

[Setup]
AppId={{A8B4E2F1-7C3D-4A91-9E5B-2D6F8A0C1B47}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
AppUpdatesURL={#MyAppURL}/releases
DefaultDirName={localappdata}\aurivon\app\versions\{#MyAppVersion}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
LicenseFile={#SourceDir}\LICENSE
OutputDir={#SourceDir}\..\
OutputBaseFilename=aurivon-{#MyAppVersion}-x86_64-windows-setup
SetupIconFile=
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
UninstallDisplayIcon={app}\{#MyAppExeName}
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{localappdata}\aurivon\app\current\{#MyAppExeName}"; WorkingDir: "{localappdata}\aurivon\app\current"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{localappdata}\aurivon\app\current\{#MyAppExeName}"; WorkingDir: "{localappdata}\aurivon\app\current"; Tasks: desktopicon

[Run]
Filename: "{cmd}"; Parameters: "/C if exist ""{localappdata}\aurivon\app\current"" rmdir ""{localappdata}\aurivon\app\current"" & mklink /J ""{localappdata}\aurivon\app\current"" ""{app}"""; Flags: runhidden waituntilterminated
Filename: "{localappdata}\aurivon\app\current\{#MyAppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent

[Code]
function InitializeSetup(): Boolean;
begin
  Result := True;
end;
