# r-todo Desktop Release Script
# Usage: ./scripts/release-desktop.ps1 [-Version <string>] [-SkipBuild] [-SkipTag] [-SkipRelease]
#
# Configure Gitee Token (one-time):
#   $env:GITEE_TOKEN = "your_token"
#   [Environment]::SetEnvironmentVariable("GITEE_TOKEN", "your_token", "User")

param(
    [string]$Version,
    [switch]$SkipBuild,
    [switch]$SkipTag,
    [switch]$SkipRelease
)

$ErrorActionPreference = "Stop"

# ── UTF-8 读写（PS 5.1 的 Get-Content/Set-Content 默认 ANSI，会损坏中文注释） ──
function Read-Utf8([string]$Path) {
    return [System.IO.File]::ReadAllText($Path, [System.Text.UTF8Encoding]::new($false))
}
function Write-Utf8([string]$Path, [string]$Content) {
    [System.IO.File]::WriteAllText($Path, $Content, [System.Text.UTF8Encoding]::new($false))
}

# Parse repo info from git remote
$remoteUrl = git remote get-url origin
if ($remoteUrl -match 'gitee\.com[:/]([^/]+)/([^/.]+)') {
    $owner = $Matches[1]
    $repo = $Matches[2]
} else {
    Write-Host "[ERR] Cannot parse Gitee repo from remote: $remoteUrl" -ForegroundColor Red
    exit 1
}

# Check working directory is clean
$dirty = git status --porcelain
if ($dirty) {
    Write-Host "[ERR] Uncommitted changes:" -ForegroundColor Red
    Write-Host $dirty
    exit 1
}

# Version files
$confPath = "apps/desktop/src-tauri/tauri.conf.json"
$pkgPath = "apps/desktop/package.json"
$cargoPath = "apps/desktop/src-tauri/Cargo.toml"

function Get-CurrentVersion {
    $conf = Read-Utf8 $confPath | ConvertFrom-Json
    return $conf.version
}

$currentVersion = Get-CurrentVersion
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

# Bump version if changed
if ($Version -ne $currentVersion) {
    Write-Host "[INFO] Bumping: $currentVersion -> $Version" -ForegroundColor Yellow

    # 精确替换顶层 version 字段，不用 ConvertTo-Json 重写整个 JSON——
    # PS 5.1 下 ConvertTo-Json | Write-Utf8 会把文件写成 0 字节（2026-09-14 事故）
    foreach ($path in @($confPath, $pkgPath)) {
        $content = Read-Utf8 $path
        $content = $content -replace '"version"\s*:\s*"[^"]*"', '"version": "$Version"'
        Write-Utf8 $path $content
    }

    $cargoContent = Read-Utf8 $cargoPath
    # Only replace version in [package] section (first occurrence, single line)
    $cargoContent = $cargoContent -replace '(\[package\][^\[]*?version = )"[^"]*"', "`$1`"$Version`""
    Write-Utf8 $cargoPath $cargoContent

    git add $confPath $pkgPath $cargoPath "apps/desktop/src-tauri/Cargo.lock"
    git commit -m "chore: bump version to $Version"
    # PS 5.1 下 2>&1 会把 native 的 stderr（gitee banner）包装成 NativeCommandError
    # 并终止脚本——用 2>$null 只丢 stderr，退出码仍经 $LASTEXITCODE 检查
    git push 2>$null
    if ($LASTEXITCODE -ne 0) { Write-Host "[ERR] push failed" -ForegroundColor Red; exit 1 }
    Write-Host "[OK] Version bumped and pushed" -ForegroundColor Green
}

# Build
$setupExe = $null
if (-not $SkipBuild) {
    Write-Host "[INFO] Building (NSIS)..." -ForegroundColor Cyan
    Set-Location apps/desktop
    pnpm tauri build
    $buildExit = $LASTEXITCODE
    Set-Location ../..
    if ($buildExit -ne 0) {
        Write-Host "[ERR] tauri build failed (exit $buildExit)" -ForegroundColor Red
        exit 1
    }

    # Find the setup exe for this version（找不到必须报错，绝不 fallback 到旧包）
    $setupExe = Get-ChildItem "apps/desktop/src-tauri/target/release/bundle/nsis/r-todo_${Version}_x64-setup.exe" -ErrorAction SilentlyContinue
    if (-not $setupExe) {
        Write-Host "[ERR] Bundle not found for v$Version（构建失败或文件名不符）" -ForegroundColor Red
        exit 1
    }
    if ($setupExe) {
        $sizeMB = [math]::Round($setupExe.Length / 1MB, 1)
        Write-Host ""
        Write-Host "[OK] Bundle: $($setupExe.Name) ($sizeMB MB)" -ForegroundColor Green

        # Read signature file
        $sigFile = $setupExe.FullName -replace '\.exe$', '.exe.sig'
        $signature = ""
        if (Test-Path $sigFile) {
            $signature = (Get-Content $sigFile -Raw).Trim()
            Write-Host "[OK] Signature found" -ForegroundColor Green
        } else {
            Write-Host "[WARN] .sig file not found: $sigFile" -ForegroundColor Yellow
        }

        # Generate update manifest
        Write-Host "[INFO] Generating update manifest..." -ForegroundColor Cyan
        $setupUrl = "https://gitee.com/$owner/$repo/releases/download/v$Version/$($setupExe.Name)"
        $pubDate = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")

        $manifest = @{
            version = $Version
            notes = "r-todo v$Version"
            pub_date = $pubDate
            platforms = @{
                "windows-x86_64" = @{
                    signature = $signature
                    url = $setupUrl
                }
            }
        }

        $manifestPath = "apps/desktop/latest.json"
        $manifest | ConvertTo-Json -Depth 10 | Write-Utf8 $manifestPath
        Write-Host "[OK] Update manifest: $manifestPath" -ForegroundColor Green
    } else {
        Write-Host "[WARN] Setup exe not found" -ForegroundColor Yellow
    }
}

# Git Tag
$tagName = "v$Version"
if (-not $SkipTag) {
    $exists = git tag -l $tagName
    if (-not $exists) {
        git tag $tagName
        git push origin $tagName
        Write-Host "[OK] Tag $tagName created and pushed" -ForegroundColor Green
    } else {
        Write-Host "[SKIP] Tag $tagName already exists" -ForegroundColor Yellow
    }
}

# Gitee Release
if (-not $SkipRelease) {
    $token = $env:GITEE_TOKEN
    if (-not $token) {
        Write-Host ""
        Write-Host "[WARN] GITEE_TOKEN not set, skipping Release" -ForegroundColor Yellow
        Write-Host "  Set: `$env:GITEE_TOKEN = 'your_token'" -ForegroundColor Gray
        Write-Host "  URL: https://gitee.com/profile/personal_access_tokens" -ForegroundColor Gray
    } else {
        Write-Host ""
        Write-Host "[INFO] Publishing to Gitee Release..." -ForegroundColor Cyan

        $apiBase = "https://gitee.com/api/v5/repos/$owner/$repo"

        # Check if release exists
        $releaseExists = $false
        $existing = $null
        try {
            $existing = Invoke-RestMethod -Uri "$apiBase/releases/tags/$tagName" -Method Get -Headers @{ Authorization = "token $token" }
            if ($existing.tag_name -eq $tagName) {
                $releaseExists = $true
                Write-Host "[SKIP] Release $tagName exists, updating assets..." -ForegroundColor Yellow
            }
        } catch {}

        if (-not $releaseExists) {
            $releaseName = "r-todo v$Version"
            $releaseBody = "## r-todo v$Version`n`n### Download`n- Windows x64: r-todo_${Version}_x64-setup.exe`n`n### Install`nRun the exe to install."

            $bodyJson = @{
                tag_name = $tagName
                name = $releaseName
                body = $releaseBody
                prerelease = $false
                target_commitish = "master"
            } | ConvertTo-Json -Depth 10

            $release = Invoke-RestMethod -Uri "$apiBase/releases" -Method Post `
                -Headers @{ Authorization = "token $token"; "Content-Type" = "application/json" } `
                -Body $bodyJson
            Write-Host "[OK] Release created: $($release.html_url)" -ForegroundColor Green
        }

        # Upload asset
        if ($setupExe) {
            $releaseId = if ($releaseExists) { $existing.id } else { $release.id }
            $uploadUrl = "$apiBase/releases/$releaseId/attach_files"

            Write-Host "[INFO] Uploading $($setupExe.Name)..." -ForegroundColor Cyan
            try {
                $rawOutput = curl.exe -s -X POST "$uploadUrl" `
                    -H "Authorization: token $token" `
                    -F "file=@$($setupExe.FullName)"

                # Extract JSON from output (may have extra chars)
                $jsonMatch = [regex]::Match($rawOutput, '\{.*\}')
                if ($jsonMatch.Success) {
                    $result = $jsonMatch.Value | ConvertFrom-Json
                    if ($result.browser_download_url) {
                        Write-Host "[OK] Asset uploaded" -ForegroundColor Green
                        Write-Host "  URL: $($result.browser_download_url)" -ForegroundColor Gray
                    } else {
                        Write-Host "[WARN] Upload response missing URL" -ForegroundColor Yellow
                    }
                } else {
                    Write-Host "[WARN] Could not parse upload response" -ForegroundColor Yellow
                }
            } catch {
                Write-Host "[WARN] Upload failed: $_" -ForegroundColor Yellow
                Write-Host "  Manual: https://gitee.com/$owner/$repo/releases" -ForegroundColor Gray
            }
        }

        # Upload latest.json to release
        $manifestPath = "apps/desktop/latest.json"
        if (Test-Path $manifestPath) {
            $releaseId = if ($releaseExists) { $existing.id } else { $release.id }
            $uploadUrl = "$apiBase/releases/$releaseId/attach_files"

            Write-Host "[INFO] Uploading latest.json..." -ForegroundColor Cyan
            try {
                curl.exe -s -X POST "$uploadUrl" `
                    -H "Authorization: token $token" `
                    -F "file=@$manifestPath"
                Write-Host "[OK] latest.json uploaded" -ForegroundColor Green
            } catch {
                Write-Host "[WARN] latest.json upload failed: $_" -ForegroundColor Yellow
            }
        }

        Write-Host "[LINK] https://gitee.com/$owner/$repo/releases/tag/$tagName" -ForegroundColor Gray
    }
}

# Commit and push latest.json
$manifestPath = "apps/desktop/latest.json"
if (Test-Path $manifestPath) {
    git add $manifestPath
    git commit -m "chore: update manifest to v$Version" 2>$null
    git push 2>$null
    if ($LASTEXITCODE -ne 0) {
        Write-Host "[WARN] manifest push failed" -ForegroundColor Yellow
    } else {
        Write-Host "[OK] Manifest committed and pushed" -ForegroundColor Green
    }
}

Write-Host ""
Write-Host "[DONE] Released: v$Version" -ForegroundColor Cyan
