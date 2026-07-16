[CmdletBinding()]
param(
    [string]$ProjectDir = $PSScriptRoot
)

$ErrorActionPreference = "Stop"

$Python = "python"
$Script = Join-Path $ProjectDir "raios-tray.py"

if (-not (Get-Command $Python -ErrorAction SilentlyContinue)) {
    throw "python command not found"
}

$StartupDir = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\Startup"
$ShortcutPath = Join-Path $StartupDir "R-AI-OS Tray.lnk"

$WScript = New-Object -ComObject WScript.Shell
$Shortcut = $WScript.CreateShortcut($ShortcutPath)
$Shortcut.TargetPath = (Get-Command $Python).Source
$Shortcut.Arguments = "`"$Script`""
$Shortcut.WorkingDirectory = $ProjectDir
$Shortcut.IconLocation = (Get-Command $Python).Source
$Shortcut.Save()

Write-Host "Startup shortcut created at $ShortcutPath"
