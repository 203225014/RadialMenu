; Установщик «Радиальное меню» (Inno Setup 6). Сборка: iscc /DAppVersion=0.6.0 installer\RadialMenu.iss
; Ставится на пользователя (без прав администратора), обновляется поверх, при удалении
; снимает автозапуск. Настройки и menu.yaml в %APPDATA%\RadialMenu при удалении остаются.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
AppId={{2DB9D7C9-4C2C-42A1-9E11-912B7611D912}
AppName=Радиальное меню
AppVersion={#AppVersion}
AppPublisher=Радиальное меню
DefaultDirName={localappdata}\Programs\RadialMenu
DefaultGroupName=Радиальное меню
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir=..\dist
OutputBaseFilename=RadialMenu-Setup-{#AppVersion}
SetupIconFile=..\assets\RadialMenu.ico
UninstallDisplayIcon={app}\RadialMenu.exe
UninstallDisplayName=Радиальное меню
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
CloseApplications=no

[Languages]
Name: "russian"; MessagesFile: "compiler:Languages\Russian.isl"

[Tasks]
Name: "autostart"; Description: "Запускать вместе с Windows"; Flags: unchecked

[Files]
Source: "..\target\release\RadialMenu.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Радиальное меню"; Filename: "{app}\RadialMenu.exe"

[Registry]
; значение, которое создаёт и сама программа (галка «Запускать с Windows» в трее)
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "RadialMenu"; ValueData: """{app}\RadialMenu.exe"""; Tasks: autostart; Flags: uninsdeletevalue

[Run]
Filename: "{app}\RadialMenu.exe"; Description: "Запустить Радиальное меню"; Flags: nowait postinstall skipifsilent

[UninstallRun]
; на случай, если автозапуск включили из программы, а не при установке
Filename: "{app}\RadialMenu.exe"; Parameters: "--remove-autostart"; RunOnceId: "RemoveAutostart"; Flags: runhidden

[Code]
// Запущенную программу закрываем до установки и удаления: иначе exe занят.
procedure StopApp;
var
  Code: Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/im RadialMenu.exe /f', '', SW_HIDE, ewWaitUntilTerminated, Code);
end;

function InitializeSetup: Boolean;
begin
  StopApp;
  Result := True;
end;

function InitializeUninstall: Boolean;
begin
  StopApp;
  Result := True;
end;
