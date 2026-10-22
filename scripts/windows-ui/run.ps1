# Runs over SSH. Starts host.ps1 on the signed-in desktop as a scheduled task,
# waits for the exit status it records, prints the step's log and exits with
# that status.
param([int]$TimeoutSeconds = 120)

$taskName = 'SnappyRefAgentUi'
$done = Join-Path $PSScriptRoot 'done.txt'
$log = Join-Path $PSScriptRoot 'step.log'

function Fail([string]$Message, [int]$Code = 1) {
    [Console]::Error.WriteLine($Message)
    exit $Code
}

# The task runs as the SSH account, so its own desktop must be signed in.
# Compare account SIDs: an SSH session's USERDOMAIN can read WORKGROUP while
# the desktop's explorer.exe belongs to the machine's domain.
function Get-OwnerSid($Process) { (Invoke-CimMethod -InputObject $Process -MethodName GetOwnerSid).Sid }
$processes = @(Get-CimInstance Win32_Process)
$account = Get-OwnerSid ($processes | Where-Object ProcessId -eq $PID)
$signedIn = @($processes | Where-Object {
    $_.Name -eq 'explorer.exe' -and $_.SessionId -gt 0 -and (Get-OwnerSid $_) -eq $account
})
if ($signedIn.Count -eq 0) {
    Fail 'No signed-in interactive session is available. Sign in at the PC first.'
}

# One step at a time owns the desktop's mouse and keyboard.
if ((Get-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue).State -eq 'Running') {
    Fail "Another step is running on the desktop. Stop it with Stop-ScheduledTask $taskName."
}

$hostScript = Join-Path $PSScriptRoot 'host.ps1'
$action = New-ScheduledTaskAction -Execute 'powershell.exe' `
    -Argument "-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File `"$hostScript`""
$principal = New-ScheduledTaskPrincipal -UserId $env:USERNAME -LogonType Interactive
Register-ScheduledTask -TaskName $taskName -Action $action -Principal $principal -Force | Out-Null
try {
    Start-ScheduledTask -TaskName $taskName
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while (-not (Test-Path $done) -and (Get-Date) -lt $deadline) {
        Start-Sleep -Milliseconds 250
    }
    $stillRunning = (Get-ScheduledTask -TaskName $taskName).State -eq 'Running'
    if ($stillRunning) { Stop-ScheduledTask -TaskName $taskName }
} finally {
    Unregister-ScheduledTask -TaskName $taskName -Confirm:$false
}

if (Test-Path $log) { Get-Content $log }
if (-not (Test-Path $done)) {
    if ($stillRunning) { Fail "Step timed out after $TimeoutSeconds s." 124 }
    Fail 'Step ended without recording an exit status.'
}
$status = (Get-Content $done -Raw).Trim()
if ($status -notmatch '^-?\d+$') { Fail 'Step ended without recording an exit status.' }
exit [int]$status
