# Spark CLI installer for Windows.
#
#   irm https://raw.githubusercontent.com/AcelateOrg/Spark/main/install.ps1 | iex
#
# Downloads spark.exe (and spark-editor.exe, when the archive has it) from the latest GitHub release into
# %LOCALAPPDATA%\Spark\bin, verifies the SHA-256 published next to the archive and adds the folder to the user PATH.
# Run it again (or `spark update`) to update. Options (environment variables):
#   SPARK_VERSION = v0.1.0     install this release instead of the latest
#   SPARK_BIN     = C:\tools   install into this folder
#   SPARK_NO_PATH = 1          do not touch PATH
#   SPARK_ZIP     = file.zip   install from a local spark-windows-x64.zip (testing)
#   SPARK_REPO    = owner/repo download from a fork (default AcelateOrg/Spark)
#   SPARK_ALLOW_UNVERIFIED = 1 install a release that has no .sha256 file
#
# Linux / macOS: curl -fsSL https://raw.githubusercontent.com/AcelateOrg/Spark/main/install.sh | sh

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$repo = if ($env:SPARK_REPO) { $env:SPARK_REPO } else { 'AcelateOrg/Spark' }
$assetName = 'spark-windows-x64.zip'

$bin = if ($env:SPARK_BIN) { $env:SPARK_BIN } else { Join-Path $env:LOCALAPPDATA 'Spark\bin' }
$tmp = Join-Path ([IO.Path]::GetTempPath()) ('spark-install-' + [guid]::NewGuid())
New-Item -ItemType Directory -Force -Path $tmp, $bin | Out-Null

try {
    $zip = Join-Path $tmp 'spark.zip'
    if ($env:SPARK_ZIP) {
        Copy-Item $env:SPARK_ZIP $zip
        $tag = 'local'
    } else {
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
        $api = if ($env:SPARK_VERSION) { "https://api.github.com/repos/$repo/releases/tags/$($env:SPARK_VERSION)" }
               else { "https://api.github.com/repos/$repo/releases/latest" }
        $release = Invoke-RestMethod -Uri $api -Headers @{ 'User-Agent' = 'spark-installer' }
        $asset = $release.assets | Where-Object { $_.name -eq $assetName } | Select-Object -First 1
        if (-not $asset) { throw "release $($release.tag_name) has no $assetName" }
        $tag = $release.tag_name
        Write-Host "Downloading Spark $tag ..."
        Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $zip -UseBasicParsing
        $sumAsset = $release.assets | Where-Object { $_.name -eq "$assetName.sha256" } | Select-Object -First 1
        if ($sumAsset) {
            $sumFile = Join-Path $tmp 'spark.sha256'
            Invoke-WebRequest -Uri $sumAsset.browser_download_url -OutFile $sumFile -UseBasicParsing
            $expected = ((Get-Content $sumFile -TotalCount 1) -split '\s+')[0].ToLower()
            $actual = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
            if ($actual -ne $expected) { throw "checksum mismatch for $assetName (expected $expected, got $actual)" }
        } elseif (-not $env:SPARK_ALLOW_UNVERIFIED) {
            throw "release $tag has no $assetName.sha256, so the download cannot be verified (older release? set SPARK_ALLOW_UNVERIFIED=1 to install anyway)"
        }
    }
    Expand-Archive -Path $zip -DestinationPath (Join-Path $tmp 'x') -Force

    # Installs one exe without ever leaving the folder without a working copy:
    #   1. copy the new exe next to the old one as <name>.new.exe (never over it);
    #   2. $verify: run `<name>.new.exe --version`; a broken download or a blocked exe stops here, nothing was replaced;
    #   3. rename the old exe away (a running spark.exe, e.g. `spark update`, can be renamed but not overwritten),
    #      then rename the new one into place; if either step fails the old exe is renamed back.
    function Install-Exe([string]$name, [bool]$required, [bool]$verify) {
        $src = Join-Path $tmp "x\$name"
        if (-not (Test-Path $src)) { if ($required) { throw "$assetName does not contain $name" } else { return } }
        $dst = Join-Path $bin $name
        $base = [IO.Path]::GetFileNameWithoutExtension($name)
        $new = Join-Path $bin "$base.new.exe"
        # Leftovers of earlier updates (deleted once they are no longer running).
        Get-ChildItem $bin -Filter "$name.old*" -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue
        Remove-Item $new -Force -ErrorAction SilentlyContinue
        Copy-Item $src $new -Force
        if ($verify) {
            $ok = $false
            try { & $new --version | Out-Null; $ok = ($LASTEXITCODE -eq 0) } catch { $ok = $false }
            if (-not $ok) {
                Remove-Item $new -Force -ErrorAction SilentlyContinue
                throw "the downloaded $name does not run (--version failed); the installed version was left untouched"
            }
        }
        $old = $null
        try {
            if (Test-Path $dst) { $old = "$dst.old-$([DateTime]::Now.Ticks)"; Move-Item $dst $old -Force }
            Move-Item $new $dst -Force
        } catch {
            $err = $_.Exception.Message
            if ($old -and -not (Test-Path $dst) -and (Test-Path $old)) { Move-Item $old $dst -Force -ErrorAction SilentlyContinue }
            Remove-Item $new -Force -ErrorAction SilentlyContinue
            throw "cannot install ${name}: $err (the previous version was kept)"
        }
        # Not running (fresh install, installer started by hand): remove it right away.
        Get-ChildItem $bin -Filter "$name.old*" -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue
    }
    $exe = Join-Path $bin 'spark.exe'
    Install-Exe 'spark.exe' $true $true
    # spark-editor is a GUI app: it is not started here, only swapped atomically with rollback.
    Install-Exe 'spark-editor.exe' $false $false
} finally {
    Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
}

if (-not $env:SPARK_NO_PATH) {
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $parts = if ($userPath) { $userPath -split ';' | Where-Object { $_ } } else { @() }
    if ($parts -notcontains $bin) {
        [Environment]::SetEnvironmentVariable('Path', (($parts + $bin) -join ';'), 'User')
        Write-Host "Added $bin to your PATH. Open a new terminal to use 'spark'."
    }
    if (($env:Path -split ';') -notcontains $bin) { $env:Path = "$env:Path;$bin" }
}

& $exe --version
Write-Host "Spark $tag installed to $exe"
if (Test-Path (Join-Path $bin 'spark-editor.exe')) { Write-Host "Editor: spark editor" }
Write-Host "Next:  spark new mygame   then   spark mygame"
