; Инсталлятор радиального меню. Собирается скриптом build.ps1 (версия передаётся параметром /DAppVersion=x.y.z)
#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#define AppName "Радиальное меню"
#define AppExe "RadialMenu.exe"
#define RuntimeUrl "https://aka.ms/dotnet/8.0/windowsdesktop-runtime-win-x64.exe"

[Setup]
AppId={{5B2E6C1A-7D3F-4A8E-9C14-0A1B2C3D4E5F}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=RadialMenu
AppPublisherURL=https://github.com/203225014/RadialMenu
AppSupportURL=https://github.com/203225014/RadialMenu/issues
AppUpdatesURL=https://github.com/203225014/RadialMenu/releases
DefaultDirName={autopf}\RadialMenu
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=Output
OutputBaseFilename=RadialMenu-Setup-{#AppVersion}
SetupIconFile=..\Resources\RadialMenu.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
CloseApplications=no
VersionInfoVersion={#AppVersion}

[Languages]
Name: "russian"; MessagesFile: "compiler:Languages\Russian.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "autostart"; Description: "Запускать вместе с Windows"; GroupDescription: "Дополнительно:"
Name: "desktopicon"; Description: "Ярлык на рабочем столе"; GroupDescription: "Дополнительно:"; Flags: unchecked

[Files]
Source: "..\publish\fd\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{group}\Удалить {#AppName}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Registry]
; тот же ключ, что использует переключатель «Запускать вместе с Windows» в настройках программы
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "RadialMenu"; \
  ValueData: """{app}\{#AppExe}"""; Tasks: autostart

[Run]
Filename: "{app}\{#AppExe}"; Description: "Запустить {#AppName}"; Flags: nowait postinstall skipifsilent

[Code]
function DesktopRuntimeInstalled: Boolean;
var
  Names: TArrayOfString;
  I: Integer;
begin
  Result := False;
  if RegGetValueNames(HKLM64, 'SOFTWARE\dotnet\Setup\InstalledVersions\x64\sharedfx\Microsoft.WindowsDesktop.App', Names) then
    for I := 0 to GetArrayLength(Names) - 1 do
      if Copy(Names[I], 1, 2) = '8.' then
      begin
        Result := True;
        Exit;
      end;
end;

procedure StopApp;
var
  Code: Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM {#AppExe}', '', SW_HIDE, ewWaitUntilTerminated, Code);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Path: String;
  Code: Integer;
begin
  Result := '';
  StopApp;   // работающую программу нельзя заменить
  if DesktopRuntimeInstalled then Exit;
  // нужен .NET 8 Desktop Runtime: скачиваем с сайта Microsoft и ставим (потребуется подтверждение Windows)
  try
    WizardForm.StatusLabel.Caption := 'Загрузка .NET 8 Desktop Runtime...';
    DownloadTemporaryFile('{#RuntimeUrl}', 'windowsdesktop-runtime-x64.exe', '', nil);
    Path := ExpandConstant('{tmp}\windowsdesktop-runtime-x64.exe');
    WizardForm.StatusLabel.Caption := 'Установка .NET 8 Desktop Runtime...';
    if not ShellExec('runas', Path, '/install /quiet /norestart', '', SW_SHOW, ewWaitUntilTerminated, Code) or
       ((Code <> 0) and (Code <> 3010)) then
      Result := 'Не удалось установить .NET 8 Desktop Runtime (код ' + IntToStr(Code) + ').' + #13#10 +
                'Установите его вручную: https://dotnet.microsoft.com/download/dotnet/8.0 (Desktop Runtime x64), затем запустите установку снова.';
  except
    Result := 'Не удалось скачать .NET 8 Desktop Runtime: ' + GetExceptionMessage + #13#10 +
              'Проверьте подключение к интернету или установите его вручную: https://dotnet.microsoft.com/download/dotnet/8.0';
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
  begin
    StopApp;
    RegDeleteValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'RadialMenu');   // автозапуск, включённый из настроек
  end;
end;

