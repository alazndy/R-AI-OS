# R-AI-OS System Master Installer
$ErrorActionPreference = "Stop"
Write-Host "R-AI-OS Kurulumu Baslatiliyor..." -ForegroundColor Cyan

# 1. Klasor Yapisi Olusturma
$BaseDir = Join-Path $HOME "Desktop\Dev_Ops_New"
$SystemDir = Join-Path $BaseDir "00_System"
$SkillsDir = Join-Path $SystemDir ".agents\skills"

if (!(Test-Path $BaseDir)) {
    Write-Host "Klasor yapisi olusturuluyor: $BaseDir" -ForegroundColor Yellow
    New-Item -Path $SkillsDir -ItemType Directory -Force
}

# 2. Bagimlilik Kontrolu (Node.js & Rust)
function Check-Command($cmd, $install_msg) {
    if (!(Get-Command $cmd -ErrorAction SilentlyContinue)) {
        Write-Host "Hata: $cmd bulunamadi. $install_msg" -ForegroundColor Red
        return $false
    }
    return $true
}

$hasNode = Check-Command "node" "Lutfen Node.js (v20+) kurun: https://nodejs.org/"
$hasRust = Check-Command "cargo" "Lutfen Rust kurun: https://rustup.rs/"

if (-not ($hasNode -and $hasRust)) {
    Write-Host "Temel gereksinimler eksik." -ForegroundColor Red
    exit
}

# 3. Global AI Araclari (NPM)
Write-Host "AI Agent'lar kuruluyor..." -ForegroundColor Cyan
# npm install -g @anthropic-ai/claude-code @google/generative-ai # Zaten kurulu oldugunu varsayiyoruz

# 4. R-AI-OS Build
Write-Host "R-AI-OS derleniyor (raios & aiosd)..." -ForegroundColor Cyan
cargo build --release

# 5. PATH ve Launcher Kaydi
$BinaryDestFolder = Join-Path $HOME ".aios"
if (!(Test-Path $BinaryDestFolder)) { New-Item -Path $BinaryDestFolder -ItemType Directory }

$Binaries = @("raios.exe", "aiosd.exe")
foreach ($bin in $Binaries) {
    $src = Join-Path (Get-Location) "target\release\$bin"
    if (Test-Path $src) {
        Copy-Item $src (Join-Path $BinaryDestFolder $bin) -Force
        Write-Host "$bin kopyalandi." -ForegroundColor Green
    }
}

# Kullanici PATH'ine ekle
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -notlike "*$BinaryDestFolder*") {
    Write-Host "PATH guncelleniyor..." -ForegroundColor Yellow
    $NewPath = $UserPath + ";" + $BinaryDestFolder
    [Environment]::SetEnvironmentVariable("Path", $NewPath, "User")
    $env:Path += ";$BinaryDestFolder"
}

# 6. MASTER.md Baslatma
$MasterFile = Join-Path $SystemDir "MASTER.md"
if (!(Test-Path $MasterFile)) {
    Write-Host "MASTER.md olusturuluyor..." -ForegroundColor Yellow
    $Content = @"
# R-AI-OS MASTER CONSTITUTION
## Core Rules
1. Be concise, be direct.
2. Code in English, Communicate in Turkish.
3. Functional first, error handling always.
"@
    Set-Content -Path $MasterFile -Value $Content
}

# 7. Otomatik Baslatma (Scheduled Task)
Write-Host "7. Otomatik baslatma ayarlaniyor..." -ForegroundColor Cyan
$AiosdPath = Join-Path $BinaryDestFolder "aiosd.exe"
if (Test-Path $AiosdPath) {
    try {
        $Action = New-ScheduledTaskAction -Execute $AiosdPath -WorkingDirectory $BinaryDestFolder
        $Trigger = New-ScheduledTaskTrigger -AtLogOn
        $Settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable
        Register-ScheduledTask -TaskName "RAIOS_Daemon" -Action $Action -Trigger $Trigger -Settings $Settings -Description "R-AI-OS Background Intelligence Daemon" -Force -ErrorAction Stop
        Write-Host "✅ Otomatik baslatma (Scheduled Task) basariyla olusturuldu." -ForegroundColor Green
    } catch {
        Write-Host "⚠️ Uyari: Otomatik baslatma ayarlanamadi. (Yonetici haklari gerekebilir)" -ForegroundColor Yellow
    }
}

Write-Host "Kurulum tamamlandi! Yeni bir terminal acip 'raios' yazarak baslayabilirsin." -ForegroundColor Green
