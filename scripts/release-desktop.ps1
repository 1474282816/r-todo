# r-todo Desktop Release Script (GitHub)
# Usage:
#   ./scripts/release-desktop.ps1 [-Version <x.y.z>] [-Notes <text>] [-SkipBuild] [-SkipTag] [-SkipRelease] [-DryRun]
#
# Prerequisites:
#   - gh CLI installed and logged in: `gh auth login`
#   - pnpm + Rust + NSIS toolchain (Tauri build)
#   - updater signing key (default: ~/.tauri/r-todo.key + ~/.tauri/r-todo.key.password)
#     override with:
#       $env:TAURI_SIGNING_PRIVATE_KEY_PATH = "C:\path\to\key"
#       $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = "password"
#
# Note:
#   - 仓库为 private 时，Release/raw 资产不能被 App 匿名下载，自动更新要等仓库转 public。
#   - latest.json 始终提交到 master（updater 读取 raw），Release 里也放一份作归档。

param(
    [string]$Version,
    [string]$Notes,
    [switch]$SkipBuild,
    [switch]$SkipTag,
    [switch]$SkipRelease,
    [switch]$DryRun
)

$ErrorActionPreference = "Stop"

# ── UTF-8 读写（PS 5.1 的 Get-Content/Set-Content 默认 ANSI，会损坏中文注释） ──
function Read-Utf8([string]$Path) {
    return [System.IO.File]::ReadAllText($Path, [System.Text.UTF8Encoding]::new($false))
}
function Write-Utf8([string]$Path, [string]$Content) {
    [System.IO.File]::WriteAllText($Path, $Content, [System.Text.UTF8Encoding]::new($false))
}

# ── 解析 GitHub 仓库（支持 HTTPS / SSH remote） ──
$remoteUrl = (git remote get-url origin).Trim()
$cleanRemote = $remoteUrl -replace '\.git$', '' -replace '/$', ''
if ($cleanRemote -match 'github\.com[:/]([^/]+)/([^/]+)$') {
    $owner = $Matches[1]
    $repo = $Matches[2]
} else {
    Write-Host "[ERR] Cannot parse GitHub repo from remote: $remoteUrl" -ForegroundColor Red
    exit 1
}
$repoSlug = "$owner/$repo"
$expectedEndpoint = "https://raw.githubusercontent.com/$repoSlug/master/apps/desktop/latest.json"

# ── 工作区必须干净（避免把无关改动混进版本提交） ──
$dirty = git status --porcelain
if ($dirty) {
    Write-Host "[ERR] Uncommitted changes:" -ForegroundColor Red
    Write-Host $dirty
    exit 1
}

# ── gh CLI 检查 ──
if (-not (Get-Command gh -ErrorAction SilentlyContinue)) {
    Write-Host "[ERR] gh CLI not found. Install it and run: gh auth login" -ForegroundColor Red
    exit 1
}
gh auth status | Out-Null
if ($LASTEXITCODE -ne 0) {
    Write-Host "[ERR] gh is not logged in. Run: gh auth login" -ForegroundColor Red
    exit 1
}

# ── 版本文件 ──
$confPath = "apps/desktop/src-tauri/tauri.conf.json"
$pkgPath = "apps/desktop/package.json"
$cargoPath = "apps/desktop/src-tauri/Cargo.toml"
$manifestPath = "apps/desktop/latest.json"

$currentVersion = (Read-Utf8 $confPath | ConvertFrom-Json).version
Write-Host "[INFO] Current version: $currentVersion" -ForegroundColor Cyan

if (-not $Version) {
    $inputVersion = Read-Host "Enter new version (Enter to keep $currentVersion)"
    if ($inputVersion) { $Version = $inputVersion }
    else { $Version = $currentVersion }
}

if ($Version -notmatch '^\d+\.\d+\.\d+$') {
    Write-Host "[ERR] Invalid version format, expected x.y.z" -ForegroundColor Red
    exit 1
}

# ── updater endpoint 必须指向当前 GitHub 仓库 ──
$conf = Read-Utf8 $confPath | ConvertFrom-Json
$currentEndpoint = $conf.plugins.updater.endpoints[0]
if ($currentEndpoint -ne $expectedEndpoint) {
    Write-Host "[ERR] updater endpoint mismatch:" -ForegroundColor Red
    Write-Host "  expected: $expectedEndpoint"
    Write-Host "  actual:   $currentEndpoint"
    Write-Host "  Update apps/desktop/src-tauri/tauri.conf.json first." -ForegroundColor Yellow
    exit 1
}

# ── 版本号变更 ──
if ($Version -ne $currentVersion) {
    Write-Host "[INFO] Bumping: $currentVersion -> $Version" -ForegroundColor Yellow

    # 精确替换顶层 version 字段，不用 ConvertTo-Json 重写整个 JSON——
    # PS 5.1 下 ConvertTo-Json | Write-Utf8 会把文件写成 0 字节（2026-09-14 事故）
    foreach ($path in @($confPath, $pkgPath)) {
        $content = Read-Utf8 $path
        $content = $content -replace '"version"\s*:\s*"[^"]*"', '"version": "' + $Version + '"'
        Write-Utf8 $path $content
    }

    $cargoContent = Read-Utf8 $cargoPath
    # 只替换 [package] 段的 version（第一处）
    $cargoContent = $cargoContent -replace '(\[package\][^\[]*?version = )"[^"]*"', "`$1`"$Version`""
    Write-Utf8 $cargoPath $cargoContent

    # 刷新 Cargo.lock 里的包版本（否则 bump 提交里的 lock 仍是旧版本）
    Push-Location "apps/desktop/src-tauri"
    cargo check --quiet
    $cargoExit = $LASTEXITCODE
    Pop-Location
    if ($cargoExit -ne 0) {
        Write-Host "[ERR] cargo check failed after version bump" -ForegroundColor Red
        exit 1
    }

    git add $confPath $pkgPath $cargoPath "apps/desktop/src-tauri/Cargo.lock"
    git commit -m "chore: bump version to $Version"
    git push origin HEAD
    if ($LASTEXITCODE -ne 0) {
        Write-Host "[ERR] push failed" -ForegroundColor Red
        exit 1
    }
    Write-Host "[OK] Version bumped and pushed" -ForegroundColor Green
}

# ── 签名密钥 ──
$keyPath = $env:TAURI_SIGNING_PRIVATE_KEY_PATH
if (-not $keyPath) {
    $keyPath = Join-Path $env:USERPROFILE ".tauri\r-todo.key"
}
$pwPath = Join-Path $env:USERPROFILE ".tauri\r-todo.key.password"

if (-not $env:TAURI_SIGNING_PRIVATE_KEY) {
    if (-not (Test-Path $keyPath)) {
        Write-Host "[ERR] Signing key not found: $keyPath" -ForegroundColor Red
        Write-Host "  Set TAURI_SIGNING_PRIVATE_KEY_PATH or place the key at the default path." -ForegroundColor Yellow
        exit 1
    }
    # build 阶段读取的是私钥内容（不是路径）
    $env:TAURI_SIGNING_PRIVATE_KEY = Read-Utf8 $keyPath
}
if (-not $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD) {
    if (Test-Path $pwPath) {
        $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = (Read-Utf8 $pwPath).Trim()
    } else {
        Write-Host "[ERR] Signing key password not set." -ForegroundColor Red
        Write-Host "  Set TAURI_SIGNING_PRIVATE_KEY_PASSWORD or create $pwPath" -ForegroundColor Yellow
        exit 1
    }
}

# ── Dry run ──
if ($DryRun) {
    $exeForVersion = "apps/desktop/src-tauri/target/release/bundle/nsis/r-todo_${Version}_x64-setup.exe"
    Write-Host "[DRY RUN] repo       : $repoSlug" -ForegroundColor Cyan
    Write-Host "[DRY RUN] version    : $currentVersion -> $Version"
    Write-Host "[DRY RUN] endpoint   : $expectedEndpoint"
    Write-Host "[DRY RUN] key        : $keyPath"
    Write-Host "[DRY RUN] bundle     : $exeForVersion (exists: $(Test-Path $exeForVersion))"
    Write-Host "[DRY RUN] build/tag/release: $(-not $SkipBuild)/$(-not $SkipTag)/$(-not $SkipRelease)"
    exit 0
}

# ── 构建 ──
if (-not $SkipBuild) {
    Write-Host "[INFO] Building (NSIS)..." -ForegroundColor Cyan
    Push-Location apps/desktop
    pnpm tauri build
    $buildExit = $LASTEXITCODE
    Pop-Location
    if ($buildExit -ne 0) {
        Write-Host "[ERR] tauri build failed (exit $buildExit)" -ForegroundColor Red
        exit 1
    }
}

$setupExe = Get-Item "apps/desktop/src-tauri/target/release/bundle/nsis/r-todo_${Version}_x64-setup.exe" -ErrorAction SilentlyContinue
if (-not $setupExe) {
    Write-Host "[ERR] Bundle not found for v$Version (build failed or filename mismatch)" -ForegroundColor Red
    exit 1
}
$sizeMB = [math]::Round($setupExe.Length / 1MB, 1)
Write-Host "[OK] Bundle: $($setupExe.Name) ($sizeMB MB)" -ForegroundColor Green

# ── 签名（build 通常会生成；缺失时补签） ──
$sigPath = $setupExe.FullName -replace '\.exe$', '.exe.sig'
if (-not (Test-Path $sigPath)) {
    Write-Host "[INFO] Signature missing, signing bundle..." -ForegroundColor Cyan
    Push-Location apps/desktop
    pnpm tauri signer sign -f $keyPath -p $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD $setupExe.FullName
    $signExit = $LASTEXITCODE
    Pop-Location
    if ($signExit -ne 0 -or -not (Test-Path $sigPath)) {
        Write-Host "[ERR] Failed to sign bundle" -ForegroundColor Red
        exit 1
    }
}
$signature = (Read-Utf8 $sigPath).Trim()
if (-not $signature) {
    Write-Host "[ERR] Signature file is empty: $sigPath" -ForegroundColor Red
    exit 1
}
Write-Host "[OK] Signature: $sigPath" -ForegroundColor Green

# ── 生成 latest.json ──
$notesText = if ($Notes) { $Notes } else { "r-todo v$Version" }
$setupUrl = "https://github.com/$repoSlug/releases/download/v$Version/$($setupExe.Name)"
$pubDate = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")

$manifest = [ordered]@{
    version = $Version
    notes = $notesText
    pub_date = $pubDate
    platforms = [ordered]@{
        "windows-x86_64" = [ordered]@{
            signature = $signature
            url = $setupUrl
        }
    }
}
$manifestJson = $manifest | ConvertTo-Json -Depth 10
# 先存变量再显式传参：避免多行字符串逐行绑定到 $Content 把文件写空（2026-09-14 事故）
Write-Utf8 $manifestPath ([string]$manifestJson)
Write-Host "[OK] Update manifest: $manifestPath" -ForegroundColor Green

# ── 提交 manifest ──
git add $manifestPath
$manifestDiff = git diff --cached --name-only
if ($manifestDiff) {
    git commit -m "chore: update manifest to v$Version"
    git push origin HEAD
    if ($LASTEXITCODE -ne 0) {
        Write-Host "[WARN] manifest push failed" -ForegroundColor Yellow
    } else {
        Write-Host "[OK] Manifest committed and pushed" -ForegroundColor Green
    }
} else {
    Write-Host "[SKIP] Manifest unchanged" -ForegroundColor Yellow
}

# ── Tag ──
$tagName = "v$Version"
if (-not $SkipTag) {
    git fetch origin --tags 2>$null | Out-Null
    $exists = git tag -l $tagName
    if (-not $exists) {
        git tag $tagName
        git push origin $tagName
        if ($LASTEXITCODE -ne 0) {
            Write-Host "[ERR] Tag push failed" -ForegroundColor Red
            exit 1
        }
        Write-Host "[OK] Tag $tagName created and pushed" -ForegroundColor Green
    } else {
        Write-Host "[SKIP] Tag $tagName already exists" -ForegroundColor Yellow
    }
}

# ── GitHub Release ──
if (-not $SkipRelease) {
    gh release view $tagName --repo $repoSlug 2>$null | Out-Null
    $releaseExists = ($LASTEXITCODE -eq 0)

    if ($releaseExists) {
        Write-Host "[INFO] Release $tagName exists, uploading assets (clobber)..." -ForegroundColor Cyan
        gh release upload $tagName $setupExe.FullName $manifestPath --clobber --repo $repoSlug
    } else {
        Write-Host "[INFO] Creating Release $tagName..." -ForegroundColor Cyan
        gh release create $tagName $setupExe.FullName $manifestPath --repo $repoSlug --title "r-todo v$Version" --notes $notesText
    }
    if ($LASTEXITCODE -ne 0) {
        Write-Host "[ERR] GitHub Release failed" -ForegroundColor Red
        exit 1
    }
    Write-Host "[OK] Release: https://github.com/$repoSlug/releases/tag/$tagName" -ForegroundColor Green
}

# ── 私有仓库提醒 ──
$isPrivate = gh repo view $repoSlug --json isPrivate --jq .isPrivate 2>$null
if ([string]$isPrivate -and ([string]$isPrivate).Trim() -eq "true") {
    Write-Host ""
    Write-Host "[WARN] Repository is private. App auto-update will not work until it is public." -ForegroundColor Yellow
    Write-Host "       Release assets and raw latest.json require authentication." -ForegroundColor Gray
}

Write-Host ""
Write-Host "[DONE] Released: v$Version" -ForegroundColor Cyan
Write-Host "  Manifest: $expectedEndpoint"
