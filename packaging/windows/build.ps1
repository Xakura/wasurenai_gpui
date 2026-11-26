<#
.SYNOPSIS
    构建 Wasurenai Release 二进制并使用 Inno Setup 6 生成 Windows 安装包。
.EXAMPLE
    pwsh -File packaging/windows/build.ps1
.EXAMPLE
    pwsh -File packaging/windows/build.ps1 -Version 0.0.1 -SkipBuild
#>
[CmdletBinding()]
param(
    [string]$Version,
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'

$RepoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$CargoTomlPath = Join-Path $RepoRoot 'Cargo.toml'

if ([string]::IsNullOrWhiteSpace($Version)) {
    $tomlLine = Select-String -Path $CargoTomlPath -Pattern '^\s*version\s*=\s*"([^"]+)"' | Select-Object -First 1
    if (-not $tomlLine) {
        throw "无法从 $CargoTomlPath 解析版本号，请通过 -Version 显式指定。"
    }
    $Version = $tomlLine.Matches[0].Groups[1].Value
}

function Find-Iscc {
    $cmd = Get-Command 'ISCC.exe' -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }

    $candidates = @(
        (Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 6\ISCC.exe'),
        (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6\ISCC.exe'),
        (Join-Path $env:ProgramFiles 'Inno Setup 6\ISCC.exe')
    )
    foreach ($path in $candidates) {
        if ($path -and (Test-Path $path)) { return $path }
    }
    throw "未找到 ISCC.exe（Inno Setup 6）。请先安装 JRSoftware.InnoSetup。"
}

$Iscc = Find-Iscc

if (-not $SkipBuild) {
    Write-Host "[1/3] 编译 Release 可执行文件..."
    Push-Location $RepoRoot
    try {
        cargo build --release
        if ($LASTEXITCODE -ne 0) {
            throw "cargo build --release 失败（退出码 $LASTEXITCODE）。"
        }
    }
    finally {
        Pop-Location
    }
}

$ExePath = Join-Path $RepoRoot 'target\release\wasurenai_gpui.exe'
if (-not (Test-Path $ExePath)) {
    throw "未找到可执行文件：$ExePath"
}

$IconFile = @(
    (Join-Path $RepoRoot 'target\release'),
    (Join-Path $RepoRoot 'target')
) | ForEach-Object {
    if (Test-Path $_) {
        Get-ChildItem -Path $_ -Filter 'wasurenai.ico' -Recurse -ErrorAction SilentlyContinue
    }
} | Sort-Object LastWriteTime -Descending | Select-Object -First 1

if (-not $IconFile) {
    throw "未在 target 目录下找到 wasurenai.ico，请确认未跳过 build.rs 生成。"
}

$DistDir = Join-Path $RepoRoot 'dist'
New-Item -ItemType Directory -Path $DistDir -Force | Out-Null

$IssPath = Join-Path $PSScriptRoot 'wasurenai.iss'
Write-Host "[2/3] 使用 Inno Setup 打包（版本 $Version）..."
& $Iscc `
    "/DAppVersion=$Version" `
    "/DExePath=$ExePath" `
    "/DAppIcon=$($IconFile.FullName)" `
    "/DOutputDir=$DistDir" `
    $IssPath

if ($LASTEXITCODE -ne 0) {
    throw "ISCC.exe 打包失败（退出码 $LASTEXITCODE）。"
}

$SetupExe = Join-Path $DistDir "Wasurenai-$Version-setup.exe"
if (-not (Test-Path $SetupExe)) {
    throw "打包看似成功但未找到预期输出：$SetupExe"
}

$SizeMb = [Math]::Round((Get-Item $SetupExe).Length / 1MB, 2)
Write-Host "[3/3] 安装包生成完成：$SetupExe ($SizeMb MB)"
