param(
    [string] $Executable = (Join-Path $env:LOCALAPPDATA "OpenUsage\build-target\release\openusage-windows.exe"),
    [switch] $ExpectHidden,
    [switch] $VerifyQuit
)

$ErrorActionPreference = "Stop"

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
using System.Text;

public static class OpenUsageWindowProbe
{
    private delegate bool EnumWindowsCallback(IntPtr handle, IntPtr parameter);

    [StructLayout(LayoutKind.Sequential)]
    public struct Rectangle
    {
        public int Left;
        public int Top;
        public int Right;
        public int Bottom;
    }

    [DllImport("user32.dll")]
    private static extern bool EnumWindows(EnumWindowsCallback callback, IntPtr parameter);

    [DllImport("user32.dll")]
    private static extern int GetWindowText(IntPtr handle, StringBuilder title, int maximumCharacters);

    [DllImport("user32.dll")]
    private static extern int GetWindowTextLength(IntPtr handle);

    [DllImport("user32.dll")]
    private static extern uint GetWindowThreadProcessId(IntPtr handle, out uint processId);

    [DllImport("user32.dll")]
    public static extern bool GetWindowRect(IntPtr handle, out Rectangle rectangle);

    [DllImport("user32.dll")]
    public static extern IntPtr GetForegroundWindow();

    [DllImport("user32.dll")]
    public static extern bool IsWindowVisible(IntPtr handle);

    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr handle);

    public static IntPtr FindVisibleApplicationWindow(int expectedProcessId)
    {
        IntPtr match = IntPtr.Zero;
        EnumWindows((handle, parameter) =>
        {
            uint processId;
            GetWindowThreadProcessId(handle, out processId);
            Rectangle rectangle;
            if (processId == expectedProcessId &&
                IsWindowVisible(handle) &&
                GetWindowTextLength(handle) > 0 &&
                GetWindowRect(handle, out rectangle) &&
                rectangle.Right - rectangle.Left > 100 &&
                rectangle.Bottom - rectangle.Top > 100)
            {
                match = handle;
                return false;
            }

            return true;
        }, IntPtr.Zero);
        return match;
    }

    public static string ReadWindowTitle(IntPtr handle)
    {
        int length = GetWindowTextLength(handle);
        StringBuilder title = new StringBuilder(length + 1);
        GetWindowText(handle, title, title.Capacity);
        return title.ToString();
    }
}
"@

if (-not (Test-Path -LiteralPath $Executable)) {
    throw "Release executable not found: $Executable"
}

$process = if ($VerifyQuit) {
    Start-Process -FilePath $Executable -ArgumentList "--verify-quit" -PassThru
}
elseif ($ExpectHidden) {
    Start-Process -FilePath $Executable -PassThru
}
else {
    Start-Process -FilePath $Executable -ArgumentList "--show" -PassThru
}
try {
    if ($VerifyQuit) {
        if (-not $process.WaitForExit(10000)) {
            throw "OpenUsage did not exit through its shared Quit action."
        }

        [ordered]@{
            executable = $Executable
            processId = $process.Id
            verifyQuit = $true
            exitCode = $process.ExitCode
        } | ConvertTo-Json -Compress
        return
    }

    if ($ExpectHidden) {
        Start-Sleep -Seconds 1
        $process.Refresh()
        if ($process.HasExited) {
            throw "OpenUsage exited before hidden tray verification completed."
        }

        $visibleWindow = [OpenUsageWindowProbe]::FindVisibleApplicationWindow($process.Id)
        if ($visibleWindow -ne 0) {
            throw "OpenUsage exposed an application window during hidden tray launch."
        }

        [ordered]@{
            executable = $Executable
            processId = $process.Id
            processName = $process.ProcessName
            hiddenLaunch = $true
            responding = $process.Responding
            threadCount = $process.Threads.Count
            handleCount = $process.HandleCount
            workingSetBytes = $process.WorkingSet64
            privateMemoryBytes = $process.PrivateMemorySize64
        } | ConvertTo-Json -Compress
        return
    }

    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    do {
        Start-Sleep -Milliseconds 250
        $process.Refresh()
        $windowHandle = [OpenUsageWindowProbe]::FindVisibleApplicationWindow($process.Id)
    } while ($windowHandle -eq 0 -and [DateTime]::UtcNow -lt $deadline)

    if ($process.HasExited) {
        throw "OpenUsage exited before native launch verification completed."
    }

    if ($windowHandle -eq 0) {
        throw "OpenUsage did not create a top-level window before the verification timeout."
    }

    $null = [OpenUsageWindowProbe]::SetForegroundWindow($windowHandle)
    Start-Sleep -Milliseconds 100

    $windowRectangle = New-Object OpenUsageWindowProbe+Rectangle
    if (-not [OpenUsageWindowProbe]::GetWindowRect($windowHandle, [ref] $windowRectangle)) {
        throw "OpenUsage window bounds could not be read."
    }

    $probe = [ordered]@{
        executable = $Executable
        processId = $process.Id
        processName = $process.ProcessName
        title = [OpenUsageWindowProbe]::ReadWindowTitle($windowHandle)
        windowVisible = [OpenUsageWindowProbe]::IsWindowVisible($windowHandle)
        windowFocused = [OpenUsageWindowProbe]::GetForegroundWindow() -eq $windowHandle
        windowWidth = $windowRectangle.Right - $windowRectangle.Left
        windowHeight = $windowRectangle.Bottom - $windowRectangle.Top
        responding = $process.Responding
        threadCount = $process.Threads.Count
        handleCount = $process.HandleCount
        workingSetBytes = $process.WorkingSet64
        privateMemoryBytes = $process.PrivateMemorySize64
    }

    $probe | ConvertTo-Json -Compress
}
finally {
    if (-not $process.HasExited) {
        Stop-Process -Id $process.Id -Force
        $process.WaitForExit()
    }

    if (Get-Process -Id $process.Id -ErrorAction SilentlyContinue) {
        throw "OpenUsage process remained after verification shutdown."
    }

    "PROCESS_CLEANUP=confirmed"
}
