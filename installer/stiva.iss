; Inno Setup script for Stiva. Built in CI by .github/workflows/installer.yml:
;   ISCC /DMyAppVersion=0.1.0 /DSourceDir=dist installer\stiva.iss
; Per-user install: no administrator rights, files under %LOCALAPPDATA%\Programs\Stiva.

#ifndef MyAppVersion
  #define MyAppVersion "0.0.0"
#endif
#ifndef SourceDir
  #define SourceDir "dist"
#endif

#define MyAppName "Stiva"
#define MyAppExe "stiva.exe"

[Setup]
; Fixed forever: it is how Windows (and the next installer) recognises this app for upgrades and uninstall.
AppId={{0F2D4377-DA69-4F82-819A-E7E1C0FB4023}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher=echidistanti
AppPublisherURL=https://github.com/echidistanti/stiva
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
UninstallDisplayIcon={app}\{#MyAppExe}
SetupIconFile=..\assets\icon.ico
UninstallDisplayName={#MyAppName}
OutputDir=output
OutputBaseFilename=stiva-{#MyAppVersion}-windows-x64-setup
Compression=lzma2/ultra
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "italian"; MessagesFile: "compiler:Languages\Italian.isl"

[CustomMessages]
english.RemoveDataPrompt=Also remove Stiva's settings, presets, queue, logs and the ffmpeg it downloaded?%n%nYour videos are never touched.
italian.RemoveDataPrompt=Rimuovere anche impostazioni, preset, coda, log e l'ffmpeg scaricato da Stiva?%n%nI tuoi video non vengono mai toccati.

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceDir}\{#MyAppExe}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
; The AppUserModelID ties the notification Stiva shows when a queue ends to this shortcut's name and icon.
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExe}"; AppUserModelID: "Stiva"
Name: "{group}\{cm:UninstallProgram,{#MyAppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExe}"; Tasks: desktopicon; AppUserModelID: "Stiva"

[Registry]
; The notification identity the program writes at run time (name and icon). Created here empty so that
; the uninstaller removes it, with whatever the program put in it.
Root: HKCU; Subkey: "Software\Classes\AppUserModelId\Stiva"; Flags: uninsdeletekey

[Run]
Filename: "{app}\{#MyAppExe}"; Description: "{cm:LaunchProgram,{#MyAppName}}"; Flags: nowait postinstall skipifsilent

[Code]
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  DataDir: String;
begin
  if CurUninstallStep <> usPostUninstall then Exit;
  DataDir := ExpandConstant('{localappdata}\Stiva');
  if not DirExists(DataDir) then Exit;
  { A silent uninstall keeps user data; an interactive one asks. }
  if UninstallSilent then Exit;
  if MsgBox(CustomMessage('RemoveDataPrompt'), mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES then
  begin
    { Only what Stiva itself writes, item by item. The output folder may have been set inside this one:
      never delete it as a whole. }
    DelTree(DataDir + '\bin', True, True, True);
    DelTree(DataDir + '\presets', True, True, True);
    DelTree(DataDir + '\logs', True, True, True);
    DeleteFile(DataDir + '\settings.json');
    DeleteFile(DataDir + '\queue.json');
    DeleteFile(DataDir + '\icon.png');
    { Removes the folder only when nothing else is left in it. }
    RemoveDir(DataDir);
  end;
end;
