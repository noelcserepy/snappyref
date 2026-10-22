# Helpers for a windows-ui step. A capture holds the SnappyRef window alone,
# so nothing else on the desktop can reach a screenshot. Click coordinates are
# pixels in such a capture.
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class SnappyRefUi {
    public struct RECT { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr window);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr window, int command);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window, out RECT rect);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr window, IntPtr dc, uint flags);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, int x, int y, uint data, UIntPtr extra);
}
'@
# Physical pixels, so window rectangles, captures and the cursor agree.
[SnappyRefUi]::SetProcessDPIAware() | Out-Null

function Wait-AppWindow([int]$Seconds = 30) {
    $deadline = (Get-Date).AddSeconds($Seconds)
    do {
        $app = Get-Process snappyref -ErrorAction SilentlyContinue |
            Where-Object MainWindowHandle -ne 0 | Select-Object -First 1
        if ($app) { return $app.MainWindowHandle }
        Start-Sleep -Milliseconds 200
    } while ((Get-Date) -lt $deadline)
    throw "No SnappyRef window after $Seconds s."
}

function Show-AppWindow {
    $window = Wait-AppWindow
    if ([SnappyRefUi]::IsIconic($window)) { [SnappyRefUi]::ShowWindow($window, 9) | Out-Null }
    [SnappyRefUi]::SetForegroundWindow($window) | Out-Null
    Start-Sleep -Milliseconds 150
    $rect = New-Object SnappyRefUi+RECT
    [SnappyRefUi]::GetWindowRect($window, [ref]$rect) | Out-Null
    [pscustomobject]@{ Handle = $window; Rect = $rect }
}

function Save-AppShot([string]$Name) {
    if ($Name -notmatch '^[A-Za-z0-9_-]+$') { throw "Capture name '$Name' takes letters, digits, - and _." }
    $app = Show-AppWindow
    $width = $app.Rect.Right - $app.Rect.Left
    $height = $app.Rect.Bottom - $app.Rect.Top
    $image = New-Object System.Drawing.Bitmap $width, $height
    $graphics = [System.Drawing.Graphics]::FromImage($image)
    $dc = $graphics.GetHdc()
    # PW_RENDERFULLCONTENT (2) has the window draw itself, GPU content included,
    # instead of copying whatever covers it on screen.
    try { $drawn = [SnappyRefUi]::PrintWindow($app.Handle, $dc, 2) }
    finally { $graphics.ReleaseHdc($dc); $graphics.Dispose() }
    try {
        if (-not $drawn) { throw 'PrintWindow could not draw the SnappyRef window.' }
        $image.Save((Join-Path $env:SNAPPYREF_UI_CAPTURES "$Name.png"), [System.Drawing.Imaging.ImageFormat]::Png)
    } finally { $image.Dispose() }
    "captured $Name.png ${width}x$height"
}

function Invoke-AppClick([int]$X, [int]$Y) {
    $app = Show-AppWindow
    [SnappyRefUi]::SetCursorPos($app.Rect.Left + $X, $app.Rect.Top + $Y) | Out-Null
    Start-Sleep -Milliseconds 80
    [SnappyRefUi]::mouse_event(0x2, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 60
    [SnappyRefUi]::mouse_event(0x4, 0, 0, 0, [UIntPtr]::Zero)
}

# SendKeys syntax: '^b' is Ctrl+B, '{ESC}' is Escape.
function Send-AppKeys([string]$Keys) {
    Show-AppWindow | Out-Null
    [System.Windows.Forms.SendKeys]::SendWait($Keys)
}
