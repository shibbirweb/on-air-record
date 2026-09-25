<#
.SYNOPSIS
    On Air Record installer for Windows.

.DESCRIPTION
    Everything lands in an `on-air-record` folder created in whatever directory you run this from: the
    program, your settings, and the recordings. Nothing is written anywhere else on the machine, so moving
    or removing the whole installation is moving or removing that one folder.

    It works out which build this machine needs, downloads the latest release, checks it against the
    published checksum, asks once which port to use, remembers the answer, and starts the service.

.EXAMPLE
    irm https://raw.githubusercontent.com/shibbirweb/on-air-record/master/scripts/install.ps1 | iex

.EXAMPLE
    # With options, which need the script block form because `iex` cannot take parameters:
    & ([scriptblock]::Create((irm https://raw.githubusercontent.com/shibbirweb/on-air-record/master/scripts/install.ps1))) -Port 9000
#>

[CmdletBinding()]
param(
    # Install into this folder instead of .\on-air-record
    [string] $Dir,

    # Use this port and do not ask
    [int] $Port,

    # Install this exact version, like v0.1.0, instead of the latest
    [string] $Release,

    # Ask for the port again, even if a config already exists
    [switch] $Reconfigure,

    # Download the latest release even if a program is already installed
    [switch] $Update,

    # Use beta releases: the newest release, beta or stable. Remembered for -Update
    [switch] $Beta,

    # Go back to stable releases only, once one is newer than what is installed
    [switch] $Stable,

    # Install and configure, but do not start the service
    [switch] $NoStart
)

$ErrorActionPreference = 'Stop'

# Invoke-WebRequest spends most of a large download painting this, so turning it off is worth several
# times the transfer speed on Windows PowerShell.
$ProgressPreference = 'SilentlyContinue'

# Windows PowerShell 5.1 still negotiates TLS 1.0 by default, which GitHub refuses.
try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
} catch {
    # PowerShell 7 manages this itself and the type may not be settable. Not a problem.
}

$Repository  = 'shibbirweb/on-air-record'
$DefaultPort = 8080
$FolderName  = 'on-air-record'

function Write-Step { param([string] $Text) Write-Host ''; Write-Host "==> $Text" -ForegroundColor Cyan }
function Write-Detail { param([string] $Text) Write-Host "  $Text" }
function Write-Warn { param([string] $Text) Write-Host "warning: $Text" -ForegroundColor Yellow }
function Stop-WithError { param([string] $Text) Write-Host "error: $Text" -ForegroundColor Red; exit 1 }

# ---------------------------------------------------------------- which build

function Get-Target {
    $arch = $env:PROCESSOR_ARCHITEW6432
    if (-not $arch) { $arch = $env:PROCESSOR_ARCHITECTURE }

    switch ($arch) {
        'AMD64' { return 'x86_64-pc-windows-msvc' }
        'ARM64' {
            # Windows on ARM runs x64 programs under emulation, so the 64 bit build is the right one.
            Write-Detail 'ARM64 Windows: using the x64 build, which runs under emulation.'
            return 'x86_64-pc-windows-msvc'
        }
        'x86' {
            Stop-WithError @"
this is 32 bit Windows, and only a 64 bit build is published.
If the machine is actually 64 bit, run this from a 64 bit PowerShell.
"@
        }
        default { Stop-WithError "unsupported architecture: $arch" }
    }
}

# The redirect on /releases/latest names the tag, which keeps this off the API and its hourly rate limit.
function Get-LatestVersion {
    $url = "https://github.com/$Repository/releases/latest"
    try {
        $response = Invoke-WebRequest -Uri $url -UseBasicParsing -MaximumRedirection 10
    } catch {
        Stop-WithError "could not reach GitHub to find the latest release: $($_.Exception.Message)"
    }

    # Windows PowerShell and PowerShell 7 expose the final address in different places.
    $final = $null
    if ($response.BaseResponse.PSObject.Properties['ResponseUri']) {
        $final = $response.BaseResponse.ResponseUri.AbsoluteUri
    }
    if (-not $final -and $response.BaseResponse.PSObject.Properties['RequestMessage']) {
        $final = $response.BaseResponse.RequestMessage.RequestUri.AbsoluteUri
    }
    if (-not $final) { Stop-WithError 'could not work out the latest version from the releases page' }

    $version = $final.Split('/')[-1]
    if ($version -notmatch '^v\d') { Stop-WithError "could not work out the latest version from $final" }
    return $version
}

# The newest release of any kind, beta or stable, for the beta channel. /releases/latest never counts a
# pre-release, so this one asks the API, which lists releases newest first. That costs one call against
# the unauthenticated hourly limit, which only people on betas ever spend. A GITHUB_TOKEN in the environment
# is used when present, which CI relies on: its runners share addresses and so share the limit.
function Get-NewestRelease {
    $headers = @{ Accept = 'application/vnd.github+json' }
    if ($env:GITHUB_TOKEN) { $headers['Authorization'] = "Bearer $env:GITHUB_TOKEN" }
    try {
        $releases = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repository/releases?per_page=1" `
            -Headers $headers -UseBasicParsing
    } catch {
        Stop-WithError "could not reach GitHub to find the newest release: $($_.Exception.Message)"
    }
    $version = @($releases)[0].tag_name
    if ($version -notmatch '^v\d') { Stop-WithError "could not work out the newest release from GitHub's answer" }
    return $version
}

# True when version $A is older than $B. Both look like 0.4.0 or 0.4.0-beta.2, with or without the v, and a
# beta comes before the release it leads up to: 0.4.0-beta.2 is older than 0.4.0.
function Test-IsOlder {
    param([string] $A, [string] $B)
    # PowerShell names ignore case, so these must not be called $a and $b: those are the parameters.
    $left = $A.TrimStart('v'); $right = $B.TrimStart('v')
    $coreA = [version] ($left -split '-', 2)[0]
    $coreB = [version] ($right -split '-', 2)[0]
    if ($coreA -ne $coreB) { return $coreA -lt $coreB }
    $betaA = if ($left -match '-beta\.(\d+)$') { [int] $Matches[1] } else { -1 }
    $betaB = if ($right -match '-beta\.(\d+)$') { [int] $Matches[1] } else { -1 }
    if ($betaA -eq $betaB -or $betaA -eq -1) { return $false }
    if ($betaB -eq -1) { return $true }
    return $betaA -lt $betaB
}

# Record the channel in the config, keeping every other setting as it was.
function Save-Channel {
    param([string] $Path, [string] $Channel)
    if (-not (Test-Path $Path)) { return }
    $lines = @(Get-Content -Path $Path | Where-Object { $_ -notmatch '^OAR_CHANNEL=' })
    $lines + "OAR_CHANNEL=$Channel" | Set-Content -Path $Path -Encoding ASCII
}

function Install-Release {
    param([string] $Target, [string] $Version, [string] $InstallDir)

    $name = "on-air-record-$Version-$Target"
    $base = "https://github.com/$Repository/releases/download/$Version"
    $temp = Join-Path ([System.IO.Path]::GetTempPath()) ("oar-" + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $temp -Force | Out-Null

    try {
        $archive = Join-Path $temp "$name.zip"
        Write-Detail "downloading $name.zip"
        try {
            Invoke-WebRequest -Uri "$base/$name.zip" -OutFile $archive -UseBasicParsing
        } catch {
            Stop-WithError "could not download $base/$name.zip : $($_.Exception.Message)"
        }

        # Saved to a file rather than read from .Content, because GitHub serves this as octet-stream and
        # PowerShell then hands back a byte array, whose first element is a number rather than a digit.
        $checksumFile = Join-Path $temp "$name.zip.sha256"
        $havePublished = $true
        try {
            Invoke-WebRequest -Uri "$base/$name.zip.sha256" -OutFile $checksumFile -UseBasicParsing
        } catch {
            $havePublished = $false
        }

        if ($havePublished) {
            $expected = ((Get-Content -Path $checksumFile -Raw) -split '\s+')[0].ToLower()
            $actual   = (Get-FileHash -Path $archive -Algorithm SHA256).Hash.ToLower()
            if ($expected -ne $actual) {
                Stop-WithError "the download is corrupt: expected $expected, got $actual"
            }
            Write-Detail 'checksum verified'
        } else {
            Write-Warn 'no published checksum for this build, so the download was not verified'
        }

        $unpacked = Join-Path $temp 'unpacked'
        Expand-Archive -Path $archive -DestinationPath $unpacked -Force

        $exe = Join-Path (Join-Path $unpacked $name) 'on-air-record.exe'
        if (-not (Test-Path $exe)) { Stop-WithError 'the archive did not contain the program' }

        Copy-Item -Path $exe -Destination (Join-Path $InstallDir 'on-air-record.exe') -Force
        foreach ($extra in @('README.md', 'LICENSE')) {
            $from = Join-Path (Join-Path $unpacked $name) $extra
            if (Test-Path $from) { Copy-Item -Path $from -Destination $InstallDir -Force }
        }
    } finally {
        Remove-Item -Path $temp -Recurse -Force -ErrorAction SilentlyContinue
    }
}

# ---------------------------------------------------------------- asking

function Test-PortFree {
    param([int] $Number)
    try {
        $listening = Get-NetTCPConnection -LocalPort $Number -State Listen -ErrorAction SilentlyContinue
        return -not $listening
    } catch {
        # Get-NetTCPConnection is Windows only and not on every edition. Not being able to check is not a
        # reason to refuse a port; the service says so itself if the port is taken.
        return $true
    }
}

function Read-Port {
    # With input redirected, or no user session at all, Read-Host keeps returning nothing, so the default
    # would come back and fail the same way forever. Under CI that is a job hanging until its time limit.
    $interactive = [Environment]::UserInteractive -and -not [Console]::IsInputRedirected

    while ($true) {
        $answer = Read-Host "Which port should the web interface use? [$DefaultPort]"
        if ([string]::IsNullOrWhiteSpace($answer)) { $answer = "$DefaultPort" }

        $number = 0
        $problem = $null
        if (-not [int]::TryParse($answer.Trim(), [ref] $number)) {
            $problem = "'$answer' is not a number"
        } elseif ($number -lt 1024 -or $number -gt 65535) {
            $problem = "$number is not between 1024 and 65535"
        } elseif (-not (Test-PortFree -Number $number)) {
            $problem = "something is already listening on $number"
        }

        if (-not $problem) { return $number }
        if (-not $interactive) {
            Stop-WithError "$problem, and there is nobody to ask for another. Run it again with -Port <number>."
        }
        Write-Detail "$problem, pick another."
    }
}

# The same KEY=VALUE file the shell installer writes, so both platforms have one format to explain.
function Read-Config {
    param([string] $Path)
    $settings = @{}
    if (Test-Path $Path) {
        foreach ($line in Get-Content -Path $Path) {
            $trimmed = $line.Trim()
            if ($trimmed -eq '' -or $trimmed.StartsWith('#')) { continue }
            $pair = $trimmed.Split('=', 2)
            if ($pair.Count -eq 2) { $settings[$pair[0].Trim()] = $pair[1].Trim() }
        }
    }
    return $settings
}

function Write-Config {
    param([string] $Path, [int] $Number)
    @"
# On Air Record settings, read every time start.cmd runs.
# Edit by hand, or re-run the installer with -Reconfigure.
# Everything else (microphone, retention, quality) is set in the web interface itself.

OAR_PORT=$Number
OAR_HOST=0.0.0.0
OAR_LOG_LEVEL=info
"@ | Set-Content -Path $Path -Encoding ASCII
}

# Two launchers on purpose: start.cmd so the folder can be double clicked in Explorer, and start.ps1
# because doing the work in batch would mean parsing the config file in batch.
function Write-Launcher {
    param([string] $InstallDir)

    @'
# Starts On Air Record with the settings in the config file beside this script.
# Written by the installer. Arguments are passed through, so a flag beats the config file for one run.
$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path

$config = Join-Path $here 'config'
if (Test-Path $config) {
    foreach ($line in Get-Content -Path $config) {
        $trimmed = $line.Trim()
        if ($trimmed -eq '' -or $trimmed.StartsWith('#')) { continue }
        $pair = $trimmed.Split('=', 2)
        if ($pair.Count -eq 2) {
            [Environment]::SetEnvironmentVariable($pair[0].Trim(), $pair[1].Trim(), 'Process')
        }
    }
}

# Recordings always live beside the program, so the folder stays self contained.
$env:OAR_DATA_DIR = Join-Path $here 'data'

& (Join-Path $here 'on-air-record.exe') @args
exit $LASTEXITCODE
'@ | Set-Content -Path (Join-Path $InstallDir 'start.ps1') -Encoding ASCII

    @'
@echo off
rem Double click this, or run it from a command prompt, to start On Air Record.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0start.ps1" %*
'@ | Set-Content -Path (Join-Path $InstallDir 'start.cmd') -Encoding ASCII

    @'
# Stops the On Air Record started from this folder.
# Written by the installer.
#
# It matches on this installation's own program path rather than on the name, so a second copy running
# from somewhere else is left alone.
#
# Windows has no polite signal for a console program, so this is a hard stop. The recording up to the last
# completed segment is safe, because segments are indexed as they close; only the few seconds still being
# written are lost. Ctrl+C in the window it is running in shuts down cleanly and keeps those too.
$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$program = Join-Path $here 'on-air-record.exe'

# Get-Process rather than Win32_Process: it is present in every PowerShell on every platform, and its
# Path is the same absolute path this installation was written to.
function Get-Running {
    param([string] $Program)
    return @(Get-Process -Name 'on-air-record' -ErrorAction SilentlyContinue |
        Where-Object { $_.Path -eq $Program })
}

$running = Get-Running -Program $program
if ($running.Count -eq 0) {
    Write-Host "Nothing is running from $here."
    exit 0
}

Write-Host "Stopping $($running.Id -join ', ')"
foreach ($process in $running) {
    Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
}

foreach ($attempt in 1..20) {
    if ((Get-Running -Program $program).Count -eq 0) {
        Write-Host 'Stopped.'
        exit 0
    }
    Start-Sleep -Seconds 1
}

Write-Error 'It is still running after 20s.'
exit 1
'@ | Set-Content -Path (Join-Path $InstallDir 'stop.ps1') -Encoding ASCII

    @'
@echo off
rem Double click this, or run it from a command prompt, to stop On Air Record.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0stop.ps1" %*
'@ | Set-Content -Path (Join-Path $InstallDir 'stop.cmd') -Encoding ASCII
}

function Get-RelativePath {
    param([string] $Path)
    $here = (Get-Location).Path
    if ($Path.StartsWith($here + [IO.Path]::DirectorySeparatorChar)) {
        return '.' + $Path.Substring($here.Length)
    }
    return $Path
}

# ---------------------------------------------------------------- run

Write-Host 'On Air Record installer'

$target = Get-Target
$installDir = if ($Dir) { $Dir } else { Join-Path (Get-Location).Path $FolderName }
$installDir = [IO.Path]::GetFullPath($installDir)

$binary     = Join-Path $installDir 'on-air-record.exe'
$configFile = Join-Path $installDir 'config'
$dataDir    = Join-Path $installDir 'data'
$launcher   = Join-Path $installDir 'start.cmd'
$stopper    = Join-Path $installDir 'stop.cmd'

Write-Detail "this machine:  Windows $env:PROCESSOR_ARCHITECTURE  ->  $target"
Write-Detail "installing to: $(Get-RelativePath $installDir)"

$parent = Split-Path -Parent $installDir
if (-not (Test-Path $parent)) { Stop-WithError "$parent does not exist" }

New-Item -ItemType Directory -Path $installDir -Force | Out-Null
New-Item -ItemType Directory -Path $dataDir -Force | Out-Null

# Which releases this folder follows: a switch wins, then what it was installed with, then stable.
if ($Beta -and $Stable) { Stop-WithError '-Beta and -Stable cannot be used together' }
$savedChannel = (Read-Config -Path $configFile)['OAR_CHANNEL']
$forcedChannel = if ($Beta) { 'beta' } elseif ($Stable) { 'stable' } else { $null }
$channel = if ($forcedChannel) { $forcedChannel } elseif ($savedChannel) { $savedChannel } else { 'stable' }
# Asking for the other channel is asking for its release, so it installs without needing -Update too.
$previousChannel = if ($savedChannel) { $savedChannel } else { 'stable' }
if ($forcedChannel -and $forcedChannel -ne $previousChannel) { $Update = $true }

if ($Release) {
    # An explicit version is an instruction, not a preference, so it overwrites whatever is already here.
    if ($Release -notmatch '^v\d') { Stop-WithError "-Release wants a tag like v0.1.0, not $Release" }
    Write-Step "Installing $Release"
    Install-Release -Target $target -Version $Release -InstallDir $installDir
    Write-Detail 'installed'
} elseif ((Test-Path $binary) -and -not $Update) {
    # `on-air-record 0.1.0` -> `0.1.0`. Anything unexpected, including a binary that will not run at all,
    # falls back rather than printing an empty version.
    $installed = 'unknown'
    try {
        $reported = & $binary --version 2>$null
        if ($reported) { $installed = ($reported -split '\s+')[-1] }
    } catch { }
    Write-Step "Already installed here: version $installed"
    if ($channel -eq 'beta') {
        Write-Detail 'on beta releases; run with -Update to fetch the newest one'
    } else {
        Write-Detail 'run with -Update to fetch the latest release'
    }
} else {
    $version = if ($channel -eq 'beta') { Get-NewestRelease } else { Get-LatestVersion }

    $installed = $null
    if (Test-Path $binary) {
        try {
            $reported = & $binary --version 2>$null
            if ($reported) { $installed = ($reported -split '\s+')[-1] }
        } catch { }
    }

    # Never step backwards on the way to a channel. Going from a beta back to an older stable version can
    # drop features the data now relies on; stable 0.3.0, for one, has no logins at all, so a recorder that
    # had accounts would quietly open up. The switch is remembered and happens once stable catches up.
    if ($installed -and $installed -match '^\d' -and (Test-IsOlder -A $version -B $installed)) {
        Write-Step "Keeping version $installed"
        Write-Detail "the newest $channel release, $version, is older than what is installed here."
        Write-Detail "Going back could lose features this version's data relies on, so it stays until a newer"
        Write-Detail "$channel release is out, which -Update will then install."
    } else {
        Write-Step "Installing $version"
        Install-Release -Target $target -Version $version -InstallDir $installDir
        Write-Detail 'installed'
    }
}

if ($PSBoundParameters.ContainsKey('Port')) {
    Write-Step 'Configuring'
    Write-Config -Path $configFile -Number $Port
    Write-Detail "port $Port, from -Port"
} elseif ((Test-Path $configFile) -and -not $Reconfigure) {
    $existing = Read-Config -Path $configFile
    Write-Step 'Using the settings already saved in this folder'
    Write-Detail "port $($existing['OAR_PORT'])"
    Write-Detail 're-run with -Reconfigure to change them'
} else {
    Write-Step 'First run, so one question'
    Write-Detail "The web interface needs a port. $DefaultPort is the default; press enter to accept it."
    Write-Host ''
    Write-Config -Path $configFile -Number (Read-Port)
    Write-Host ''
    Write-Detail 'saved'
}

Save-Channel -Path $configFile -Channel $channel
Write-Launcher -InstallDir $installDir

$settings = Read-Config -Path $configFile
$chosenPort = if ($settings['OAR_PORT']) { $settings['OAR_PORT'] } else { $DefaultPort }

Write-Step 'Ready'
Write-Detail "start it again:  $(Get-RelativePath $launcher)"
Write-Detail "stop it:         $(Get-RelativePath $stopper)"
Write-Detail "open:            http://localhost:$chosenPort"
Write-Detail "settings:        $(Get-RelativePath $configFile)"
Write-Detail "recordings:      $(Get-RelativePath $dataDir)"
if ($channel -eq 'beta') {
    Write-Detail 'releases:        beta; -Stable returns to stable ones'
}
Write-Host ''
Write-Detail "To remove it completely, delete $(Get-RelativePath $installDir)."
Write-Host ''
Write-Detail 'Windows will ask whether to allow it through the firewall the first time. Say yes for'
Write-Detail 'private networks, or no other machine will be able to listen.'

if (-not $NoStart) {
    Write-Step "Starting on port $chosenPort"
    Write-Detail 'Ctrl+C stops it cleanly while this window is open. Afterwards, or if the window goes'
    Write-Detail "away with the service still running, use $(Get-RelativePath $stopper)."
    Write-Host ''
    & $launcher
}
