param(
    [string]$DownloadRoot = ".devtools/downloads",
    [string]$SourceRoot = ".devtools/starter-sources",
    [string]$OutputRoot = "dist/engine-packs/windows-x86_64/starter"
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$devtoolsRoot = [System.IO.Path]::GetFullPath((Join-Path $repoRoot ".devtools"))

function Resolve-RepoPath([string]$Path) {
    if ([System.IO.Path]::IsPathRooted($Path)) {
        return [System.IO.Path]::GetFullPath($Path)
    }
    [System.IO.Path]::GetFullPath((Join-Path $repoRoot $Path))
}

function Assert-WithinDevtools([string]$Path) {
    $prefix = $devtoolsRoot.TrimEnd([System.IO.Path]::DirectorySeparatorChar) + [System.IO.Path]::DirectorySeparatorChar
    if (-not $Path.StartsWith($prefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "Dependency cache and source paths must stay inside $devtoolsRoot"
    }
}

$downloadPath = Resolve-RepoPath $DownloadRoot
$sourcePath = Resolve-RepoPath $SourceRoot
Assert-WithinDevtools $downloadPath
Assert-WithinDevtools $sourcePath
New-Item -ItemType Directory -Path $downloadPath -Force | Out-Null
New-Item -ItemType Directory -Path $sourcePath -Force | Out-Null

$dependencies = @(
    [ordered]@{
        Name = "poppler-26.02.0-0"
        Archive = "Release-26.02.0-0.zip"
        Url = "https://github.com/oschwartz10612/poppler-windows/releases/download/v26.02.0-0/Release-26.02.0-0.zip"
        Sha256 = "993e4a94376ed712fafc7058d724ea0b943d118bbd2305cd9ed55174eb85cda5"
        ExtractedDirectory = "poppler-26.02.0"
    },
    [ordered]@{
        Name = "ffmpeg-9.0.1-essentials"
        Archive = "ffmpeg-9.0.1-essentials_build.zip"
        Url = "https://www.gyan.dev/ffmpeg/builds/packages/ffmpeg-9.0.1-essentials_build.zip"
        Sha256 = "fec81ae03971d9dd4be3ebe02e263bd2ec1d789483f931bdba5f5715e65da2e9"
        ExtractedDirectory = "ffmpeg-9.0.1-essentials_build"
    }
)

function Get-Sha256([string]$Path) {
    (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLowerInvariant()
}

function Get-VerifiedArchive($Dependency) {
    $archivePath = Join-Path $downloadPath $Dependency.Archive
    if ((Test-Path -LiteralPath $archivePath -PathType Leaf) -and
        (Get-Sha256 $archivePath) -eq $Dependency.Sha256) {
        return $archivePath
    }

    $partial = Join-Path $downloadPath (".{0}.{1}.partial" -f $Dependency.Archive, [guid]::NewGuid().ToString("N"))
    try {
        Invoke-WebRequest -Uri $Dependency.Url -OutFile $partial -UseBasicParsing
        $observed = Get-Sha256 $partial
        if ($observed -ne $Dependency.Sha256) {
            throw "Archive hash mismatch for $($Dependency.Name): expected $($Dependency.Sha256), observed $observed"
        }
        Move-Item -LiteralPath $partial -Destination $archivePath -Force
        return $archivePath
    } finally {
        if (Test-Path -LiteralPath $partial) {
            Remove-Item -LiteralPath $partial -Force
        }
    }
}

function Expand-VerifiedArchive($Dependency, [string]$ArchivePath) {
    $destination = Join-Path $sourcePath $Dependency.Name
    $expectedRoot = Join-Path $destination $Dependency.ExtractedDirectory
    if (Test-Path -LiteralPath $expectedRoot -PathType Container) {
        return $expectedRoot
    }

    $staging = Join-Path $sourcePath (".{0}.{1}.partial" -f $Dependency.Name, [guid]::NewGuid().ToString("N"))
    $backup = Join-Path $sourcePath (".{0}.{1}.backup" -f $Dependency.Name, [guid]::NewGuid().ToString("N"))
    try {
        Expand-Archive -LiteralPath $ArchivePath -DestinationPath $staging
        $stagedRoot = Join-Path $staging $Dependency.ExtractedDirectory
        if (-not (Test-Path -LiteralPath $stagedRoot -PathType Container)) {
            throw "Archive layout is unexpected for $($Dependency.Name)"
        }
        if (Test-Path -LiteralPath $destination) {
            Move-Item -LiteralPath $destination -Destination $backup
        }
        Move-Item -LiteralPath $staging -Destination $destination
        if (Test-Path -LiteralPath $backup) {
            Remove-Item -LiteralPath $backup -Recurse -Force
        }
        return $expectedRoot
    } catch {
        if ((Test-Path -LiteralPath $backup) -and -not (Test-Path -LiteralPath $destination)) {
            Move-Item -LiteralPath $backup -Destination $destination
        }
        throw
    } finally {
        if (Test-Path -LiteralPath $staging) {
            Remove-Item -LiteralPath $staging -Recurse -Force
        }
    }
}

$resolved = @{}
foreach ($dependency in $dependencies) {
    $archive = Get-VerifiedArchive $dependency
    $resolved[$dependency.Name] = Expand-VerifiedArchive $dependency $archive
}

# --- OCR pack sources (spec E-11, DECISION-4) -------------------------------
# The engine ships as an NSIS installer; it is unpacked with 7-Zip instead of
# being executed. Traineddata files are fetched separately so eng/chi_sim are
# the pinned upstream artifacts rather than whatever the installer bundles.
$TesseractVersion = "5.4.0.20240606"
$tesseractInstaller = [ordered]@{
    Name = "tesseract-$TesseractVersion"
    Archive = "tesseract-ocr-w64-setup-$TesseractVersion.exe"
    Url = "https://github.com/UB-Mannheim/tesseract/releases/download/v$TesseractVersion/tesseract-ocr-w64-setup-$TesseractVersion.exe"
    Sha256 = "c885fff6998e0608ba4bb8ab51436e1c6775c2bafc2559a19b423e18678b60c9"
}
$tessdataFiles = @(
    [ordered]@{
        File = "eng.traineddata"
        Url = "https://github.com/tesseract-ocr/tessdata/raw/main/eng.traineddata"
        Sha256 = "daa0c97d651c19fba3b25e81317cd697e9908c8208090c94c3905381c23fc047"
    },
    [ordered]@{
        File = "chi_sim.traineddata"
        Url = "https://github.com/tesseract-ocr/tessdata/raw/main/chi_sim.traineddata"
        Sha256 = "fc05d89ab31d8b4e226910f16a8bcbf78e43bae3e2580bb5feefd052efdab363"
    },
    [ordered]@{
        File = "tessdata-LICENSE"
        Url = "https://raw.githubusercontent.com/tesseract-ocr/tessdata/main/LICENSE"
        Sha256 = $null
    }
)
foreach ($data in $tessdataFiles) {
    $target = Join-Path $downloadPath $data.File
    if (-not ((Test-Path -LiteralPath $target -PathType Leaf) -and
        ($null -eq $data.Sha256 -or (Get-Sha256 $target) -eq $data.Sha256))) {
        Invoke-WebRequest -Uri $data.Url -OutFile $target
    }
    if ($null -ne $data.Sha256 -and (Get-Sha256 $target) -ne $data.Sha256) {
        throw "tessdata hash mismatch for $($data.File)"
    }
}
$tesseractRoot = Join-Path $sourcePath "tesseract-$TesseractVersion"
$tesseractReady = (Test-Path -LiteralPath (Join-Path $tesseractRoot "tesseract.exe") -PathType Leaf) -and
    (Test-Path -LiteralPath (Join-Path $tesseractRoot "tessdata/chi_sim.traineddata") -PathType Leaf)
if (-not $tesseractReady) {
    $installerPath = Get-VerifiedArchive $tesseractInstaller
    # GitHub-hosted downloads arrive with Mark-of-the-Web on developer PCs;
    # 7-Zip extraction never executes the installer either way.
    Unblock-File -LiteralPath $installerPath -ErrorAction SilentlyContinue
    $unpack = Join-Path $sourcePath ".tesseract-unpack.$([Guid]::NewGuid().ToString('N'))"
    New-Item -ItemType Directory -Path $unpack -Force | Out-Null
    $sevenZip = @("7z.exe", "7za.exe") |
        ForEach-Object { Get-Command $_ -ErrorAction SilentlyContinue } |
        Select-Object -First 1
    if ($null -eq $sevenZip) {
        throw "7-Zip is required to unpack the Tesseract installer (no installer execution). Install 7-Zip or place an unpacked tree at $tesseractRoot"
    }
    & $sevenZip.Source x -y "-o$unpack" $installerPath | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "7-Zip could not unpack the Tesseract installer"
    }
    if (Test-Path -LiteralPath $tesseractRoot) {
        Remove-Item -LiteralPath $tesseractRoot -Recurse -Force
    }
    New-Item -ItemType Directory -Path $tesseractRoot -Force | Out-Null
    # The installer payload unpacks flat: executables, DLLs, and tessdata all
    # land next to each other, which is the layout the engine pack expects.
    foreach ($item in Get-ChildItem -LiteralPath $unpack) {
        if ($item.PSIsContainer -and $item.Name -eq '$PLUGINSDIR') { continue }
        Move-Item -LiteralPath $item.FullName -Destination $tesseractRoot
    }
    Remove-Item -LiteralPath $unpack -Recurse -Force
}
# Pin the traineddata files into the unpacked tree (installer-bundled eng is
# replaced by the pinned upstream artifact; chi_sim is added).
foreach ($data in $tessdataFiles) {
    if ($data.File -like "*.traineddata") {
        Copy-Item -LiteralPath (Join-Path $downloadPath $data.File) `
            -Destination (Join-Path $tesseractRoot "tessdata/$($data.File)") -Force
    }
}
if (-not (Test-Path -LiteralPath (Join-Path $repoRoot ".devtools/downloads/tessdata-LICENSE") -PathType Leaf)) {
    New-Item -ItemType Directory -Path (Join-Path $repoRoot ".devtools/downloads") -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $downloadPath "tessdata-LICENSE") `
        -Destination (Join-Path $repoRoot ".devtools/downloads/tessdata-LICENSE") -Force
}

& (Join-Path $PSScriptRoot "build_windows_starter_pack.ps1") `
    -PopplerRoot $resolved["poppler-26.02.0-0"] `
    -FfmpegRoot $resolved["ffmpeg-9.0.1-essentials"] `
    -TesseractRoot $tesseractRoot `
    -OutputRoot $OutputRoot
