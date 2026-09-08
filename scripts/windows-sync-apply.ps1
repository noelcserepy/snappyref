param([Parameter(Mandatory=$true)][string]$Source, [Parameter(Mandatory=$true)][string]$Destination)
$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath($Destination).TrimEnd('\', '/')
if (-not (Test-Path -LiteralPath (Join-Path $root '.git'))) { throw 'destination must be a Git checkout' }
$manifest = Get-Content -LiteralPath (Join-Path $Source 'manifest.json') -Raw | ConvertFrom-Json
function Normalize-Origin([string]$origin) {
    return ($origin.Trim().ToLowerInvariant() -replace '^https?://', '' -replace '^ssh://git@', '' -replace '^git@([^:]+):', '$1/' -replace '\.git/?$', '').TrimEnd('/')
}
$remoteOrigin = & git -C $root remote get-url origin
if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($manifest.origin) -or
    (Normalize-Origin $remoteOrigin) -ne (Normalize-Origin $manifest.origin)) {
    throw 'destination origin does not match the source repository'
}
$state = (& git -C $root rev-parse --git-path snappyref-sync.json)
if ($LASTEXITCODE -ne 0) { throw 'cannot resolve destination Git directory' }
if (-not [IO.Path]::IsPathRooted($state)) { $state = Join-Path $root $state }
$protected = @('.git', 'target', 'library', 'models', 'originals', 'thumbs', 'library.db', '.env.windows')
function Resolve-SourcePath([string]$name) {
    $parts = $name.Split('/')
    if ([string]::IsNullOrWhiteSpace($name) -or $name -match '[\\:\r\n\t"<>|?*]' -or
        $parts[0] -in $protected -or ($parts[0] -like '.env*' -and $parts[0] -notlike '*.example') -or ($parts | Where-Object { $_ -in @('', '.', '..') -or $_ -match '[ .]$' })) {
        throw "unsafe managed source path: $name"
    }
    $path = [IO.Path]::GetFullPath((Join-Path $root $name))
    if (-not $path.StartsWith($root + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'source path escapes checkout' }
    $cursor = $path
    while ($cursor -ne $root) {
        if (Test-Path -LiteralPath $cursor) {
            $item = Get-Item -LiteralPath $cursor -Force
            if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -and ($cursor -ne $path -or $item.PSIsContainer)) { throw "source path crosses a link: $name" }
        }
        $cursor = Split-Path $cursor -Parent
    }
    return $path
}
$current = @($manifest.files)
if ('Cargo.toml' -notin $current -or 'src/main.rs' -notin $current) { throw 'source inventory lacks repository markers' }
$previous = @()
if (Test-Path -LiteralPath $state) { $previous = Get-Content -LiteralPath $state -Raw | ConvertFrom-Json
    $previous = @($previous) }
$tracked = @(& git -C $root -c core.quotepath=false ls-files)
if ($LASTEXITCODE -ne 0) { throw 'cannot read destination tracked source files' }
# Ignored machine files and caches never become deletion candidates.
$previous += $tracked
$previous = @($previous | Where-Object {
    $top = $_.Split('/')[0]
    $top -notin $protected -and -not ($top -like '.env*' -and $top -notlike '*.example')
})
$resolved = @{}
foreach ($name in @($current) + @($previous)) { $resolved[$name] = Resolve-SourcePath $name }
foreach ($name in $current) {
    $inputPath = Join-Path (Join-Path $Source 'source') $name
    if (-not (Test-Path -LiteralPath $inputPath -PathType Leaf)) { throw "missing transferred source: $name" }
    if (Test-Path -LiteralPath $resolved[$name] -PathType Container) { throw "source file conflicts with directory: $name" }
}
foreach ($name in $previous | Sort-Object -Unique) {
    if ($name -notin $current -and (Test-Path -LiteralPath $resolved[$name])) {
        if (-not (Test-Path -LiteralPath $resolved[$name] -PathType Leaf)) { throw "refusing directory deletion: $name" }
        Remove-Item -LiteralPath $resolved[$name] -Force
    }
}
foreach ($name in $current) {
    New-Item -ItemType Directory -Force -Path (Split-Path $resolved[$name] -Parent) | Out-Null
    $existing = Get-Item -LiteralPath $resolved[$name] -Force -ErrorAction SilentlyContinue
    if ($existing -and ($existing.Attributes -band [IO.FileAttributes]::ReparsePoint)) { Remove-Item -LiteralPath $resolved[$name] -Force }
    Copy-Item -LiteralPath (Join-Path (Join-Path $Source 'source') $name) -Destination $resolved[$name] -Force
}
ConvertTo-Json -InputObject @($current) | Set-Content -LiteralPath $state -Encoding UTF8
Write-Output "Synced $($current.Count) managed source files."
