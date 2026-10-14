# Native managed-source deletion and preservation checks in disposable checkouts.
$ErrorActionPreference = 'Stop'
$scratch = Join-Path ([IO.Path]::GetTempPath()) ([Guid]::NewGuid().ToString())
$destination = Join-Path $scratch 'checkout'
$transfer = Join-Path $scratch 'transfer'
$origin = 'https://github.com/noelcserepy/snappyref.git'
$apply = Join-Path $PSScriptRoot 'windows-sync-apply.ps1'
function Require($condition, [string]$message) { if (-not $condition) { throw $message } }
function Transfer([string[]]$files, [string]$repository = $origin) {
    $source = Join-Path $transfer 'source'
    New-Item -ItemType Directory -Force $source | Out-Null
    foreach ($name in $files) {
        $path = Join-Path $source $name
        New-Item -ItemType Directory -Force (Split-Path $path -Parent) | Out-Null
        Set-Content -LiteralPath $path -Value "current $name"
    }
    @{origin=$repository; files=@($files)} | ConvertTo-Json | Set-Content (Join-Path $transfer 'manifest.json')
    & $apply -Source $transfer -Destination $destination
}
try {
    New-Item -ItemType Directory -Force $destination | Out-Null
    git -C $destination init -q
    git -C $destination remote add origin $origin
    foreach ($name in @('removed.rs', 'renamed.rs', 'modified.rs')) {
        Set-Content (Join-Path $destination $name) 'old'
    }
    git -C $destination add .
    git -C $destination -c user.name=Fixture -c user.email=fixture@example.com commit -qm fixture
    Set-Content (Join-Path $destination '.env.local') 'private'
    Set-Content (Join-Path $destination 'unrelated.txt') 'unrelated'
    New-Item -ItemType Directory (Join-Path $destination 'target') | Out-Null
    Set-Content (Join-Path $destination 'target/cache') 'cache'
    Transfer @('Cargo.toml', 'src/main.rs', 'modified.rs', 'new.rs')
    Require (-not (Test-Path (Join-Path $destination 'removed.rs'))) 'committed removal survived'
    Require (-not (Test-Path (Join-Path $destination 'renamed.rs'))) 'committed rename survived'
    Require ((Get-Content (Join-Path $destination 'modified.rs')) -eq 'current modified.rs') 'modification missing'
    Transfer @('Cargo.toml', 'src/main.rs', 'next.rs')
    Require (-not (Test-Path (Join-Path $destination 'new.rs'))) 'managed unstaged rename survived'
    Require (-not (Test-Path (Join-Path $destination 'modified.rs'))) 'managed unstaged deletion survived'
    Require ((Get-Content (Join-Path $destination 'next.rs')) -eq 'current next.rs') 'new source missing'
    Require ((git -C $destination remote get-url origin) -eq $origin) 'git metadata changed'
    Require ((git -C $destination log -1 --format=%s) -eq 'fixture') 'git history changed'
    foreach ($entry in @(@('.env.local', 'private'), @('unrelated.txt', 'unrelated'), @('target/cache', 'cache'))) {
        Require ((Get-Content (Join-Path $destination $entry[0])) -eq $entry[1]) "unrelated contents changed: $($entry[0])"
    }
    $rejected = $false
    try { Transfer @('Cargo.toml', 'src/main.rs') 'https://github.com/other/repo.git' } catch { $rejected = $true }
    Require $rejected 'wrong repository accepted'
    Require ((Get-Content (Join-Path $destination 'next.rs')) -eq 'current next.rs') 'wrong repository mutated source'
    @{origin=$origin; files=@('Cargo.toml', 'src/main.rs', '../outside')} | ConvertTo-Json | Set-Content (Join-Path $transfer 'manifest.json')
    $rejected = $false
    try { & $apply -Source $transfer -Destination $destination } catch { $rejected = $true }
    Require $rejected 'unsafe path accepted'
    Require ((Get-Content (Join-Path $destination 'next.rs')) -eq 'current next.rs') 'unsafe inventory mutated source'
    Write-Output 'Windows source synchronization checks passed.'
} finally {
    Remove-Item -LiteralPath $scratch -Recurse -Force -ErrorAction SilentlyContinue
}
