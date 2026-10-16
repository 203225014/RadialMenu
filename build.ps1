# Сборка: публикация программы + инсталлятор. Результат: installer\Output\RadialMenu-Setup-<версия>.exe
$ErrorActionPreference = "Stop"
$env:Path = [Environment]::GetEnvironmentVariable('Path','Machine') + ';' + [Environment]::GetEnvironmentVariable('Path','User')
Set-Location $PSScriptRoot

$version = ([xml](Get-Content RadialMenu.csproj)).Project.PropertyGroup.Version | Where-Object { $_ } | Select-Object -First 1
Write-Host "Версия: $version"

Get-Process RadialMenu -ErrorAction SilentlyContinue | Stop-Process -Force
dotnet publish -c Release -r win-x64 --no-self-contained -p:PublishSingleFile=true -o publish\fd
if ($LASTEXITCODE -ne 0) { throw "dotnet publish failed" }

$iscc = @("$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe", "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe") | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $iscc) { throw "Inno Setup не найден (winget install JRSoftware.InnoSetup)" }
& $iscc "/DAppVersion=$version" installer\RadialMenu.iss
if ($LASTEXITCODE -ne 0) { throw "ISCC failed" }
Get-ChildItem installer\Output\*.exe | Select-Object Name, @{n='MB';e={[math]::Round($_.Length/1MB,2)}}
