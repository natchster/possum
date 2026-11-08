# Runs Possum in both modes and takes screenshots of the whole screen
# (scream-a.png, scream-b.png, silent-a.png, silent-b.png) and logs.
#
#   powershell -ExecutionPolicy Bypass -File windows-screenshots.ps1 <possum.exe>
#
# Windows PowerShell (.NET Framework) compiles the small screenshot helper
# below without extra packages.
param([Parameter(Mandatory)][string]$Exe)
$ErrorActionPreference = "Stop"

Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @"
using System;
using System.Drawing;
using System.Runtime.InteropServices;
public static class Snap {
    [DllImport("user32.dll")] static extern IntPtr GetDC(IntPtr hwnd);
    [DllImport("user32.dll")] static extern int ReleaseDC(IntPtr hwnd, IntPtr dc);
    [DllImport("user32.dll")] static extern int GetSystemMetrics(int index);
    [DllImport("gdi32.dll")] static extern bool BitBlt(IntPtr dst, int x, int y, int w, int h, IntPtr src, int sx, int sy, int rop);
    public static void Save(string path) {
        int w = GetSystemMetrics(0), h = GetSystemMetrics(1);
        using (var bmp = new Bitmap(w, h)) {
            using (var g = Graphics.FromImage(bmp)) {
                IntPtr dst = g.GetHdc(), src = GetDC(IntPtr.Zero);
                // SRCCOPY | CAPTUREBLT: include layered (transparent) windows.
                BitBlt(dst, 0, 0, w, h, src, 0, 0, 0x00CC0020 | 0x40000000);
                ReleaseDC(IntPtr.Zero, src);
                g.ReleaseHdc(dst);
            }
            bmp.Save(path);
        }
    }
}
"@

foreach ($run in @(@{ name = "scream"; args = @("--start", "0.02") },
                   @{ name = "silent"; args = @("--silent", "--start", "0.3") })) {
    $all = @("--verbose", "--volume", "0") + $run.args
    $p = Start-Process -FilePath $Exe -ArgumentList $all `
           -RedirectStandardError "$($run.name).log" -PassThru
    Start-Sleep -Seconds 3
    [Snap]::Save("$PWD\$($run.name)-a.png")
    Start-Sleep -Seconds 1
    [Snap]::Save("$PWD\$($run.name)-b.png")
    if ($p.HasExited) { Get-Content "$($run.name).log"; throw "possum exited with $($p.ExitCode)" }
    Stop-Process -Id $p.Id
    Start-Sleep -Milliseconds 500
    Get-Content "$($run.name).log"
}
