# r-todo 桌面端发版脚本
# 用法: ./scripts/release-desktop.ps1 [-Version <string>] [-SkipBuild] [-SkipTag] [-SkipRelease]
#
# 首次使用需配置 Gitee Token:
#   $env:GITEE_TOKEN = "你的私人令牌"
# 或永久保存到 PowerShell profile:
#   [Environment]::SetEnvironmentVariable("GITEE_TOKEN", "你的私人令牌", "User")

param(
    [string]$Version,
    [switch]$SkipBuild,
    [switch]$SkipTag,
    [switch]$SkipRelease
)

$ErrorActionPreference = "Stop"

# ── 0. 解析仓库信息 ──
$remoteUrl = git remote get-url origin
if ($remoteUrl -match 'gitee\.com[:/]([^/]+)/([^/.]+)') {
    $owner = $Matches[1]
    $repo = $Matches[2]
} else {
    Write-Host "❌ 无法从 remote URL 解析 Gitee 仓库: $remoteUrl" -ForegroundColor Red
    exit 1
}

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
$setupExe = $null
if (-not $SkipBuild) {
    Write-Host "🔨 开始打包 (NSIS)..." -ForegroundColor Cyan
    Set-Location apps/desktop
    pnpm tauri build
    Set-Location ../..

    $setupExe = Get-ChildItem "apps/desktop/src-tauri/target/release/bundle/nsis/*-setup.exe" -ErrorAction SilentlyContinue
    if ($setupExe) {
        $size = [math]::Round($setupExe.Length / 1MB, 1)
        Write-Host "`n📦 打包产物: $($setupExe.Name) ($size MB)" -ForegroundColor Green
    } else {
        Write-Host "⚠️  未找到安装包，请检查打包日志" -ForegroundColor Yellow
    }
}

# ── 5. Git Tag ──
$tagName = "v$Version"
if (-not $SkipTag) {
    $exists = git tag -l $tagName
    if (-not $exists) {
        git tag $tagName
        git push origin $tagName
        Write-Host "🏷️  已创建并推送 tag: $tagName" -ForegroundColor Green
    } else {
        Write-Host "⏭️  Tag $tagName 已存在，跳过" -ForegroundColor Yellow
    }
}

# ── 6. 发布到 Gitee Release ──
if (-not $SkipRelease) {
    $token = $env:GITEE_TOKEN
    if (-not $token) {
        Write-Host "`n⚠️  未设置 GITEE_TOKEN 环境变量，跳过发布 Release" -ForegroundColor Yellow
        Write-Host "   设置方法: `$env:GITEE_TOKEN = '你的私人令牌'" -ForegroundColor Gray
        Write-Host "   获取地址: https://gitee.com/profile/personal_access_tokens" -ForegroundColor Gray
    } else {
        Write-Host "`n🚀 发布到 Gitee Release..." -ForegroundColor Cyan

        $apiBase = "https://gitee.com/api/v5/repos/$owner/$repo"

        # 检查 Release 是否已存在
        $releaseExists = $false
        try {
            $existing = Invoke-RestMethod -Uri "$apiBase/releases/tags/$tagName" -Method Get -Headers @{ Authorization = "token $token" } -ErrorAction SilentlyContinue
            if ($existing.tag_name -eq $tagName) {
                $releaseExists = $true
                Write-Host "⏭️  Release $tagName 已存在，更新附件..." -ForegroundColor Yellow
            }
        } catch {}

        if (-not $releaseExists) {
            # 创建 Release
            $body = @{
                tag_name = $tagName
                name = "r-todo v$Version"
                body = @"
## r-todo v$Version

### 下载
- Windows x64: ``r-todo_${Version}_x64-setup.exe``

### 安装
双击 exe 安装即可。首次运行会自动创建桌面快捷方式。
"@
                prerelease = $false
            } | ConvertTo-Json -Depth 10

            $release = Invoke-RestMethod -Uri "$apiBase/releases" -Method Post `
                -Headers @{ Authorization = "token $token"; "Content-Type" = "application/json" } `
                -Body $body
            Write-Host "✅ Release 已创建: $($release.html_url)" -ForegroundColor Green
        }

        # 上传附件
        if ($setupExe) {
            $releaseId = if ($releaseExists) { $existing.id } else { $release.id }
            $uploadUrl = "$apiBase/releases/$releaseId/attach_files"

            Write-Host "📤 上传 $($setupExe.Name)..." -ForegroundColor Cyan
            try {
                $boundary = [System.Guid]::NewGuid().ToString()
                $fileBytes = [System.IO.File]::ReadAllBytes($setupExe.FullName)
                $fileName = $setupExe.Name

                $bodyLines = @(
                    "--$boundary",
                    "Content-Disposition: form-data; name=`"file`"; filename=`"$fileName`"",
                    "Content-Type: application/octet-stream",
                    "",
                    "",
                    "--$boundary--"
                )
                $bodyBytes = [System.Text.Encoding]::UTF8.GetBytes($bodyLines -join "`r`n")

                # 简化：用 curl 上传（PowerShell 的 multipart 比较麻烦）
                $tempFile = [System.IO.Path]::GetTempFileName()
                [System.IO.File]::WriteAllBytes($tempFile, $fileBytes)

                $result = curl.exe -s -X POST "$uploadUrl" `
                    -H "Authorization: token $token" `
                    -F "file=@$tempFile" | ConvertFrom-Json

                Remove-Item $tempFile -ErrorAction SilentlyContinue

                if ($result.browser_download_url) {
                    Write-Host "✅ 附件上传成功" -ForegroundColor Green
                    Write-Host "   下载地址: $($result.browser_download_url)" -ForegroundColor Gray
                } else {
                    Write-Host "⚠️  附件上传可能失败，请检查 Release 页面" -ForegroundColor Yellow
                }
            } catch {
                Write-Host "⚠️  附件上传失败: $_" -ForegroundColor Yellow
                Write-Host "   可手动上传: https://gitee.com/$owner/$repo/releases" -ForegroundColor Gray
            }
        }

        Write-Host "📎 Release 页面: https://gitee.com/$owner/$repo/releases/tag/$tagName" -ForegroundColor Gray
    }
}

Write-Host "`n✨ 发版完成: v$Version" -ForegroundColor Cyan
