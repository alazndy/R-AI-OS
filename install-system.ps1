# R-AI-OS System Master Installer
# Bu script sistemi 0'dan kurar.

$ErrorActionPreference = "Stop"
Write-Host "🚀 R-AI-OS Kurulumu Başlatılıyor..." -ForegroundColor Cyan

# 1. Klasör Yapısı Oluşturma
$BaseDir = Join-Path $HOME "Desktop\Dev_Ops_New"
$SystemDir = Join-Path $BaseDir "00_System"
$SkillsDir = Join-Path $SystemDir ".agents\skills"

if (!(Test-Path $BaseDir)) {
    Write-Host "📂 Klasör yapısı oluşturuluyor: $BaseDir" -ForegroundColor Yellow
    New-Item -Path $SkillsDir -ItemType Directory -Force
}

# 2. Bağımlılık Kontrolü (Node.js & Rust)
function Check-Command($cmd, $install_msg) {
    if (!(Get-Command $cmd -ErrorAction SilentlyContinue)) {
        Write-Host "⚠️ $cmd bulunamadı. $install_msg" -ForegroundColor Red
        return $false
    }
    return $true
}

$hasNode = Check-Command "node" "Lütfen Node.js (v20+) kurun: https://nodejs.org/"
$hasRust = Check-Command "cargo" "Lütfen Rust kurun: https://rustup.rs/"

if (-not ($hasNode -and $hasRust)) {
    Write-Host "❌ Temel gereksinimler eksik. Lütfen yukarıdaki araçları kurup scripti tekrar çalıştırın." -ForegroundColor Red
    exit
}

# 3. Global AI Araçları (NPM)
Write-Host "🤖 AI Agent'lar kuruluyor..." -ForegroundColor Cyan
npm install -g @anthropic-ai/claude-code @google/generative-ai

# 4. R-AI-OS Build
Write-Host "📦 R-AI-OS derleniyor..." -ForegroundColor Cyan
cargo build --release

# 5. PATH ve Launcher Kaydı
$BinarySource = Join-Path (Get-Location) "target\release\raios.exe"
$BinaryDestFolder = Join-Path $HOME ".aios"
if (!(Test-Path $BinaryDestFolder)) { New-Item -Path $BinaryDestFolder -ItemType Directory }

Copy-Item $BinarySource (Join-Path $BinaryDestFolder "raios.exe") -Force

# Kullanıcı PATH'ine ekle
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -notlike "*$BinaryDestFolder*") {
    Write-Host "🔗 PATH güncelleniyor..." -ForegroundColor Yellow
    [Environment]::SetEnvironmentVariable("Path", "$UserPath;$BinaryDestFolder", "User")
    $env:Path += ";$BinaryDestFolder"
}

# 6. MASTER.md Başlatma
$MasterFile = Join-Path $SystemDir "MASTER.md"
if (!(Test-Path $MasterFile)) {
    Write-Host "📜 MASTER.md (Constitution) oluşturuluyor..." -ForegroundColor Yellow
    $Content = @"
# R-AI-OS MASTER CONSTITUTION
## Core Rules
1. Be concise, be direct.
2. Code in English, Communicate in Turkish.
3. Functional first, error handling always.
"@
    Set-Content -Path $MasterFile -Value $Content
}

Write-Host "✅ Kurulum tamamlandı! Yeni bir terminal açıp 'raios' yazarak başlayabilirsin." -ForegroundColor Green
