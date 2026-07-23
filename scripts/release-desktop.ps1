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
    $conf = Get-Content $confPath | ConvertFrom-Json
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

    $conf = Get-Content $confPath -Raw | ConvertFrom-Json
    $conf.version = $Version
    $conf | ConvertTo-Json -Depth 10 | Set-Content $confPath -NoNewline

    $pkg = Get-Content $pkgPath -Raw | ConvertFrom-Json
    $pkg.version = $Version
    $pkg | ConvertTo-Json -Depth 10 | Set-Content $pkgPath -NoNewline

    (Get-Content $cargoPath -Raw) -replace 'version = ".*?"', "version = `"$Version`"" | Set-Content $cargoPath -NoNewline

    git add $confPath $pkgPath $cargoPath
    git commit -m "chore: bump version to $Version"
    git push
    Write-Host "[OK] Version bumped and pushed" -ForegroundColor Green
}

# Build
$setupExe = $null
if (-not $SkipBuild) {
    Write-Host "[INFO] Building (NSIS)..." -ForegroundColor Cyan
    Set-Location apps/desktop
    pnpm tauri build
    Set-Location ../..

    $setupExe = Get-ChildItem "apps/desktop/src-tauri/target/release/bundle/nsis/*-setup.exe" -ErrorAction SilentlyContinue
    if ($setupExe) {
        $sizeMB = [math]::Round($setupExe.Length / 1MB, 1)
        Write-Host ""
        Write-Host "[OK] Bundle: $($setupExe.Name) ($sizeMB MB)" -ForegroundColor Green
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
                $result = curl.exe -s -X POST "$uploadUrl" `
                    -H "Authorization: token $token" `
                    -F "file=@$($setupExe.FullName)" | ConvertFrom-Json

                if ($result.browser_download_url) {
                    Write-Host "[OK] Asset uploaded" -ForegroundColor Green
                    Write-Host "  URL: $($result.browser_download_url)" -ForegroundColor Gray
                } else {
                    Write-Host "[WARN] Upload may have failed" -ForegroundColor Yellow
                }
            } catch {
                Write-Host "[WARN] Upload failed: $_" -ForegroundColor Yellow
                Write-Host "  Manual: https://gitee.com/$owner/$repo/releases" -ForegroundColor Gray
            }
        }

        Write-Host "[LINK] https://gitee.com/$owner/$repo/releases/tag/$tagName" -ForegroundColor Gray
    }
}

Write-Host ""
Write-Host "[DONE] Released: v$Version" -ForegroundColor Cyan
