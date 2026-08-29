#ifndef MyAppVersion
  #define MyAppVersion "0.1.0"
#endif
#ifndef BinDir
  #define BinDir "."
#endif
#ifndef OutputDir
  #define OutputDir "dist"
#endif

#define MyAppName "tunnel-ui"
#define MyAppPublisher "TrasherDK"
#define MyAppURL "https://github.com/trasherdk/tunnel-ui-rust"
#define MyAppExeName "tunnel-ui.exe"

[Setup]
AppId={{8F3C1A2B-9D4E-4B7A-A6C0-7E1B2C3D4E5F}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
AppUpdatesURL={#MyAppURL}/releases
DefaultDirName={localappdata}\Programs\tunnel-ui
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir={#OutputDir}
OutputBaseFilename=tunnel-ui-{#MyAppVersion}-setup
SetupIconFile=..\..\assets\icon.ico
UninstallDisplayIcon={app}\{#MyAppExeName}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64
ArchitecturesInstallIn64BitMode=x64
MinVersion=10.0

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a Desktop shortcut"; GroupDescription: "Additional icons:"; Flags: unchecked

[Files]
Source: "{#BinDir}\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{userprograms}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; WorkingDir: "{code:GetDataDir}"; Comment: "SSH local-forward TUI"
Name: "{userdesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; WorkingDir: "{code:GetDataDir}"; Comment: "SSH local-forward TUI"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "Launch {#MyAppName}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
Type: files; Name: "{app}\tunnel-home"

[Code]
var
  DataDirPage: TInputDirWizardPage;
  LastInstallDir: String;

procedure InitializeWizard;
begin
  DataDirPage := CreateInputDirPage(wpSelectDir,
    'Config and state folder',
    'Where should tunnel-ui store saved setups and logs?',
    'Configs go in configs\ under this folder, and runtime files in .state\. The default is a data subfolder of the install directory.',
    False, '');
  DataDirPage.Add('');
  DataDirPage.Values[0] := GetPreviousData('DataDir', '');
end;

procedure RegisterPreviousData(PreviousDataKey: Integer);
begin
  SetPreviousData(PreviousDataKey, 'DataDir', DataDirPage.Values[0]);
end;

procedure CurPageChanged(CurPageID: Integer);
var
  Suggested: String;
begin
  if CurPageID = DataDirPage.ID then
  begin
    Suggested := AddBackslash(WizardDirValue) + 'data';
    if (DataDirPage.Values[0] = '') or
       (DataDirPage.Values[0] = AddBackslash(LastInstallDir) + 'data') then
      DataDirPage.Values[0] := Suggested;
    LastInstallDir := WizardDirValue;
  end;
end;

function NextButtonClick(CurPageID: Integer): Boolean;
begin
  Result := True;
  if CurPageID = DataDirPage.ID then
  begin
    Result := Trim(DataDirPage.Values[0]) <> '';
    if not Result then
      MsgBox('Please choose a folder for configs and state.', mbError, MB_OK);
  end;
end;

function GetDataDir(Param: String): String;
begin
  Result := Trim(DataDirPage.Values[0]);
  if Result = '' then
    Result := ExpandConstant('{app}\data');
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  DataDir, HomeFile: String;
  Lines: TArrayOfString;
begin
  if CurStep = ssPostInstall then
  begin
    DataDir := GetDataDir('');
    ForceDirectories(DataDir);
    HomeFile := ExpandConstant('{app}\tunnel-home');
    SetArrayLength(Lines, 1);
    Lines[0] := DataDir;
    SaveStringsToFile(HomeFile, Lines, True);
  end;
end;
