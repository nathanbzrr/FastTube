#ifndef AppVersion
  #define AppVersion "0.3.0"
#endif
#ifndef StagingDir
  #define StagingDir "..\target\installer-staging"
#endif
#ifndef OutputDir
  #define OutputDir "..\target\installer"
#endif
#ifndef RepoRoot
  #define RepoRoot ".."
#endif

[Setup]
AppId={{8F6A9D24-4B7C-4E12-9A3B-7C1D5E2F8A90}
AppName=FastTube
AppVersion={#AppVersion}
AppVerName=FastTube {#AppVersion}
AppPublisher=nathanbzrr
AppPublisherURL=https://github.com/nathanbzrr/FastTube
AppSupportURL=https://github.com/nathanbzrr/FastTube
AppUpdatesURL=https://github.com/nathanbzrr/FastTube/releases
DefaultDirName={autopf}\FastTube
DisableDirPage=no
DefaultGroupName=FastTube
DisableProgramGroupPage=no
AllowNoIcons=yes
DisableWelcomePage=no
DisableReadyPage=no
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir={#OutputDir}
OutputBaseFilename=FastTube-Setup-{#AppVersion}-x64
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ChangesEnvironment=yes
CloseApplications=yes
UninstallDisplayIcon={app}\fasttube.exe
UninstallDisplayName=FastTube {#AppVersion}

[Languages]
Name: "brazilianportuguese"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[CustomMessages]
brazilianportuguese.SystemIntegration=Integração com o sistema:
brazilianportuguese.AddToPath=Adicionar a pasta de instalação ao PATH
brazilianportuguese.RegisterProtocol=Registrar protocolo fasttube:// (abrir links no FastTube)
brazilianportuguese.StartWithWindows=Iniciar o FastTube junto com o Windows (na bandeja)
english.SystemIntegration=System integration:
english.AddToPath=Add installation folder to PATH
english.RegisterProtocol=Register fasttube:// protocol handler (open links in FastTube)
english.StartWithWindows=Start FastTube with Windows (minimized to tray)

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"
Name: "addtopath"; Description: "{cm:AddToPath}"; GroupDescription: "{cm:SystemIntegration}"; Flags: unchecked
Name: "protocol"; Description: "{cm:RegisterProtocol}"; GroupDescription: "{cm:SystemIntegration}"
Name: "autostart"; Description: "{cm:StartWithWindows}"; GroupDescription: "{cm:SystemIntegration}"; Flags: unchecked

[Registry]
Root: HKA; Subkey: "Software\Classes\fasttube"; ValueType: string; ValueName: ""; ValueData: "URL:FastTube Protocol"; Flags: uninsdeletekey; Tasks: protocol
Root: HKA; Subkey: "Software\Classes\fasttube"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""; Flags: uninsdeletekey; Tasks: protocol
Root: HKA; Subkey: "Software\Classes\fasttube\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: """{app}\fasttube.exe"",0"; Flags: uninsdeletekey; Tasks: protocol
Root: HKA; Subkey: "Software\Classes\fasttube\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\fasttube.exe"" ""%1"""; Flags: uninsdeletekey; Tasks: protocol
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "FastTube"; ValueData: """{app}\fasttube.exe"" --start-hidden"; Flags: uninsdeletevalue; Tasks: autostart

[Files]
Source: "{#StagingDir}\fasttube.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StagingDir}\yt-dlp.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StagingDir}\*.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#RepoRoot}\README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\FastTube"; Filename: "{app}\fasttube.exe"
Name: "{group}\{cm:UninstallProgram,FastTube}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\FastTube"; Filename: "{app}\fasttube.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\fasttube.exe"; Description: "{cm:LaunchProgram,FastTube}"; Flags: nowait postinstall skipifsilent

[Code]
function GetPathRootKey(): Integer;
begin
  if IsAdminInstallMode then
    Result := HKEY_LOCAL_MACHINE
  else
    Result := HKEY_CURRENT_USER;
end;

function GetPathSubKey(): string;
begin
  if IsAdminInstallMode then
    Result := 'SYSTEM\CurrentControlSet\Control\Session Manager\Environment'
  else
    Result := 'Environment';
end;

procedure AddDirToPath(const Dir: string);
var
  CurrentPath: string;
  UpperPath: string;
  UpperDir: string;
  RootKey: Integer;
  SubKey: string;
begin
  RootKey := GetPathRootKey();
  SubKey := GetPathSubKey();
  if not RegQueryStringValue(RootKey, SubKey, 'Path', CurrentPath) then
    CurrentPath := '';

  UpperPath := ';' + Uppercase(CurrentPath) + ';';
  UpperDir := ';' + Uppercase(Dir) + ';';
  if Pos(UpperDir, UpperPath) > 0 then
    Exit;

  if (CurrentPath <> '') and (Copy(CurrentPath, Length(CurrentPath), 1) <> ';') then
    CurrentPath := CurrentPath + ';';
  CurrentPath := CurrentPath + Dir;

  RegWriteExpandStringValue(RootKey, SubKey, 'Path', CurrentPath);
end;

procedure RemoveDirFromPath(const Dir: string);
var
  CurrentPath: string;
  WorkingPath: string;
  P: Integer;
  RootKey: Integer;
  SubKey: string;
begin
  RootKey := GetPathRootKey();
  SubKey := GetPathSubKey();
  if not RegQueryStringValue(RootKey, SubKey, 'Path', CurrentPath) then
    Exit;

  WorkingPath := ';' + CurrentPath + ';';
  P := Pos(';' + Uppercase(Dir) + ';', Uppercase(WorkingPath));
  if P = 0 then
    Exit;

  Delete(WorkingPath, P, Length(Dir) + 1);
  if (Length(WorkingPath) > 0) and (WorkingPath[1] = ';') then
    Delete(WorkingPath, 1, 1);
  if (Length(WorkingPath) > 0) and (WorkingPath[Length(WorkingPath)] = ';') then
    Delete(WorkingPath, Length(WorkingPath), 1);

  RegWriteExpandStringValue(RootKey, SubKey, 'Path', WorkingPath);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if (CurStep = ssPostInstall) and WizardIsTaskSelected('addtopath') then
    AddDirToPath(ExpandConstant('{app}'));
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
    RemoveDirFromPath(ExpandConstant('{app}'));
end;
