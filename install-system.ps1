[CmdletBinding()]
param(
    [string]$InstallRoot = (Join-Path $env:LOCALAPPDATA "R-AI-OS"),
    [string]$WorkspaceRoot = (Join-Path $HOME "Dev_Ops"),
    [switch]$SkipBuild,
    [switch]$NoScheduledTask,
    [switch]$NoPath
)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$BinDir = Join-Path $InstallRoot "bin"
$ConfigDir = Join-Path $env:APPDATA "raios"
$SystemDir = Join-Path $WorkspaceRoot "00_System"
$SkillsDir = Join-Path $SystemDir ".agents\skills"
$AiosdPath = Join-Path $BinDir "aiosd.exe"
$RaiosPath = Join-Path $BinDir "raios.exe"
$Utf8NoBom = New-Object System.Text.UTF8Encoding($false)

function Require-Command([string]$Name, [string]$Hint) {
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        throw "$Name bulunamadi. $Hint"
    }
}

function To-TomlPath([string]$Path) {
    return $Path.Replace('\', '/')
}

Write-Host "R-AI-OS Windows kurulumu basliyor..." -ForegroundColor Cyan
Write-Host "  Repo:      $RepoRoot"
Write-Host "  Binary:    $BinDir"
Write-Host "  Workspace: $WorkspaceRoot"

if (-not $SkipBuild) {
    Require-Command "cargo" "Rust toolchain'i rustup.rs adresinden kurun."
    Push-Location $RepoRoot
    try {
        Write-Host "Rust binary'leri derleniyor..." -ForegroundColor Cyan
        & cargo build --release --locked --bin raios --bin aiosd
        if ($LASTEXITCODE -ne 0) {
            throw "cargo build basarisiz oldu (exit code: $LASTEXITCODE)"
        }
    } finally {
        Pop-Location
    }
}

New-Item -ItemType Directory -Force -Path $BinDir, $ConfigDir, $SystemDir, $SkillsDir | Out-Null

foreach ($Binary in @("raios.exe", "aiosd.exe")) {
    $Source = Join-Path $RepoRoot "target\release\$Binary"
    if (-not (Test-Path $Source)) {
        throw "$Source bulunamadi. Once cargo build --release calistirin veya -SkipBuild kullanmayin."
    }
    Copy-Item -Force $Source (Join-Path $BinDir $Binary)
    Write-Host "  $Binary kuruldu" -ForegroundColor Green
}

# Keep the policy beside the platform config so the daemon and CLI resolve it
# identically regardless of the current working directory.
$PolicySource = Join-Path $RepoRoot "raios-policy.toml"
if (Test-Path $PolicySource) {
    Copy-Item -Force $PolicySource (Join-Path $ConfigDir "raios-policy.toml")
}

# Git symlinks used by the Linux development workspace are not portable to a
# normal Windows checkout. Preserve the same role with a real bootstrap file.
$ConstitutionCandidates = @(
    (Join-Path $RepoRoot "AGENT_CONSTITUTION.md"),
    (Join-Path $HOME "AGENT_CONSTITUTION.md")
)
$ConstitutionSource = $ConstitutionCandidates | Where-Object { Test-Path $_ } | Select-Object -First 1
$MasterPath = Join-Path $SystemDir "MASTER.md"
if ($ConstitutionSource) {
    Copy-Item -Force $ConstitutionSource $MasterPath
} elseif (-not (Test-Path $MasterPath)) {
    $Bootstrap = @"
# R-AI-OS Windows Bootstrap

Copy the shared AGENT_CONSTITUTION.md into this file before using agent
wrappers. The runtime configuration is already prepared for this path.
"@
    [System.IO.File]::WriteAllText($MasterPath, $Bootstrap, $Utf8NoBom)
    Write-Warning "AGENT_CONSTITUTION.md bulunamadi; $MasterPath bootstrap olarak olusturuldu."
}

$DevOpsToml = To-TomlPath $WorkspaceRoot
$MasterToml = To-TomlPath $MasterPath
$SkillsToml = To-TomlPath $SkillsDir
$Config = @"
dev_ops_path = "$DevOpsToml"
master_md_path = "$MasterToml"
skills_path = "$SkillsToml"
vault_projects_path = ""
system_name = "k-ai-ra"
agent_wrapper_enabled = false
"@
[System.IO.File]::WriteAllText((Join-Path $ConfigDir "config.toml"), $Config, $Utf8NoBom)

if (-not $NoPath) {
    $UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $PathEntries = @($UserPath -split ';' | Where-Object { $_ })
    if ($PathEntries -notcontains $BinDir) {
        [Environment]::SetEnvironmentVariable("Path", (($PathEntries + $BinDir) -join ';'), "User")
        $env:Path = "$BinDir;$env:Path"
        Write-Host "  Kullanici PATH guncellendi" -ForegroundColor Green
    }
}

if (-not $NoScheduledTask) {
    try {
        $Action = New-ScheduledTaskAction -Execute $AiosdPath -WorkingDirectory $InstallRoot
        $Trigger = New-ScheduledTaskTrigger -AtLogOn
        $Settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable
        Register-ScheduledTask -TaskName "RAIOS_Daemon" -Action $Action -Trigger $Trigger -Settings $Settings -Description "R-AI-OS Background Intelligence Daemon" -Force | Out-Null
        Start-ScheduledTask -TaskName "RAIOS_Daemon"
        Write-Host "  RAIOS_Daemon Scheduled Task kuruldu ve baslatildi" -ForegroundColor Green
    } catch {
        Write-Warning "Scheduled Task kurulamadı: $($_.Exception.Message)"
        Write-Host "  Elle baslatmak icin: & '$AiosdPath'" -ForegroundColor Yellow
    }
}

Write-Host "Kurulum tamamlandi. Yeni bir PowerShell acip 'raios --help' calistirin." -ForegroundColor Green
