# r-todo 桌面端发版脚本
# 用法: ./scripts/release-desktop.ps1 [-Version <string>] [-SkipBuild] [-SkipTag]

param(
    [string]$Version,
    [switch]$SkipBuild,
    [switch]$SkipTag
)

$ErrorActionPreference = "Stop"

# ── 1. 确认工作区干净 ──
$dirty = git status --porcelain
if ($dirty) {
    Write-Host "❌ 工作区有未提交的改动，请先 commit:" -ForegroundColor Red
    Write-Host $dirty
    exit 1
}

# ── 2. 版本号处理 ──
$confPath = "apps/desktop/src-tauri/tauri.conf.json"
$pkgPath = "apps/desktop/package.json"
$cargoPath = "apps/desktop/src-tauri/Cargo.toml"

function Get-CurrentVersion {
    $conf = Get-Content $confPath | ConvertFrom-Json
    return $conf.version
}

$currentVersion = Get-CurrentVersion
Write-Host "📋 当前版本: $currentVersion" -ForegroundColor Cyan

if (-not $Version) {
    $inputVersion = Read-Host "输入新版本号 (回车保持 $currentVersion)"
    if ($inputVersion) { $Version = $inputVersion }
    else { $Version = $currentVersion }
}

if ($Version -notmatch '^\d+\.\d+\.\d+$') {
    Write-Host "❌ 版本号格式错误，应为 x.y.z" -ForegroundColor Red
    exit 1
}

# ── 3. 更新版本号 ──
if ($Version -ne $currentVersion) {
    Write-Host "📝 更新版本号: $currentVersion -> $Version" -ForegroundColor Yellow

    # tauri.conf.json
    $conf = Get-Content $confPath -Raw | ConvertFrom-Json
    $conf.version = $Version
    $conf | ConvertTo-Json -Depth 10 | Set-Content $confPath -NoNewline

    # package.json
    $pkg = Get-Content $pkgPath -Raw | ConvertFrom-Json
    $pkg.version = $Version
    $pkg | ConvertTo-Json -Depth 10 | Set-Content $pkgPath -NoNewline

    # Cargo.toml (简单替换)
    (Get-Content $cargoPath -Raw) -replace 'version = ".*?"', "version = `"$Version`"" | Set-Content $cargoPath -NoNewline

    git add $confPath $pkgPath $cargoPath
    git commit -m "chore: bump version to $Version"
    git push
    Write-Host "✅ 版本号已更新并推送" -ForegroundColor Green
}

# ── 4. 打包 ──
if (-not $SkipBuild) {
    Write-Host "🔨 开始打包 (NSIS)..." -ForegroundColor Cyan
    Set-Location apps/desktop
    pnpm tauri build
    Set-Location ../..

    $setupExe = Get-ChildItem "apps/desktop/src-tauri/target/release/bundle/nsis/*-setup.exe" -ErrorAction SilentlyContinue
    if ($setupExe) {
        $size = [math]::Round($setupExe.Length / 1MB, 1)
        Write-Host "`n📦 打包产物: $($setupExe.Name) ($size MB)" -ForegroundColor Green
        Write-Host "   $($setupExe.FullName)" -ForegroundColor Gray
    } else {
        Write-Host "⚠️  未找到安装包，请检查打包日志" -ForegroundColor Yellow
    }
}

# ── 5. Git Tag ──
if (-not $SkipTag) {
    $tagName = "v$Version"
    $exists = git tag -l $tagName
    if (-not $exists) {
        git tag $tagName
        git push origin $tagName
        Write-Host "🏷️  已创建并推送 tag: $tagName" -ForegroundColor Green
    } else {
        Write-Host "⏭️  Tag $tagName 已存在，跳过" -ForegroundColor Yellow
    }
}

Write-Host "`n✨ 发版完成: v$Version" -ForegroundColor Cyan
Write-Host "   打包产物位置: apps/desktop/src-tauri/target/release/bundle/" -ForegroundColor Gray
