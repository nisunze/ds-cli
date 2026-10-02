param(
    [Parameter(Mandatory = $true)]
    [string] $ExecutablePath,
    [Parameter(Mandatory = $true)]
    [string] $ExpectedExecutableSha256
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$expected = [ordered]@{
    structure_usage = [ordered]@{ command_id = 40014; menu_pattern = 'Structure &Usage' }
    section_usage = [ordered]@{ command_id = 40015; menu_pattern = 'S&ection Usage' }
    summary = [ordered]@{ command_id = 40019; menu_pattern = '&Summary' }
    wind_weight_span = [ordered]@{ command_id = 40020; menu_pattern = '&Wind && Weight Span Report' }
    section_tension = [ordered]@{ command_id = 40403; menu_pattern = 'Sectio&n Sag-Tension Report' }
}

$item = Get-Item -LiteralPath $ExecutablePath -Force
if ($item.PSIsContainer) { throw "Executable path is a directory: $ExecutablePath" }
$digest = (Get-FileHash -LiteralPath $item.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
if ($digest -ne $ExpectedExecutableSha256.ToLowerInvariant()) {
    throw "PLS-CADD executable digest mismatch: expected $ExpectedExecutableSha256, got $digest"
}

Add-Type @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public static class DsGridMenuEvidence {
    public const uint LOAD_LIBRARY_AS_DATAFILE = 0x00000002;
    public const uint LOAD_LIBRARY_AS_IMAGE_RESOURCE = 0x00000020;
    public const uint RT_MENU = 4;

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern IntPtr LoadLibraryEx(string path, IntPtr file, uint flags);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool FreeLibrary(IntPtr module);

    [DllImport("user32.dll")]
    public static extern IntPtr LoadMenu(IntPtr module, IntPtr name);

    [DllImport("user32.dll")]
    public static extern bool DestroyMenu(IntPtr menu);

    [DllImport("user32.dll")]
    public static extern int GetMenuItemCount(IntPtr menu);

    [DllImport("user32.dll")]
    public static extern IntPtr GetSubMenu(IntPtr menu, int position);

    [DllImport("user32.dll")]
    public static extern uint GetMenuItemID(IntPtr menu, int position);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetMenuString(IntPtr menu, uint item, StringBuilder text, int count, uint flags);

    [UnmanagedFunctionPointer(CallingConvention.Winapi)]
    public delegate bool EnumResourceNameDelegate(IntPtr module, IntPtr type, IntPtr name, IntPtr parameter);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern bool EnumResourceNames(IntPtr module, IntPtr type, EnumResourceNameDelegate callback, IntPtr parameter);

    private static string ResourceName(IntPtr name) {
        long raw = name.ToInt64();
        return ((raw >> 16) == 0) ? raw.ToString() : Marshal.PtrToStringUni(name);
    }

    private static void Walk(IntPtr menu, string prefix, List<string> lines) {
        int count = GetMenuItemCount(menu);
        for (int position = 0; position < count; ++position) {
            StringBuilder text = new StringBuilder(512);
            GetMenuString(menu, (uint)position, text, text.Capacity, 0x400);
            string label = text.ToString().Replace("\t", " ").Trim();
            string path = String.IsNullOrEmpty(prefix) ? label : prefix + " > " + label;
            IntPtr child = GetSubMenu(menu, position);
            if (child != IntPtr.Zero) {
                Walk(child, path, lines);
            } else {
                uint command = GetMenuItemID(menu, position);
                if (command != 0 && command != UInt32.MaxValue) {
                    lines.Add(command.ToString() + "\t" + path);
                }
            }
        }
    }

    public static string[] ReadAll(string executable) {
        IntPtr module = LoadLibraryEx(executable, IntPtr.Zero,
            LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE);
        if (module == IntPtr.Zero) throw new System.ComponentModel.Win32Exception();
        List<string> lines = new List<string>();
        EnumResourceNameDelegate callback = delegate(IntPtr loaded, IntPtr type, IntPtr name, IntPtr ignored) {
            IntPtr menu = LoadMenu(loaded, name);
            if (menu != IntPtr.Zero) {
                Walk(menu, "resource " + ResourceName(name), lines);
                DestroyMenu(menu);
            }
            return true;
        };
        try {
            if (!EnumResourceNames(module, (IntPtr)RT_MENU, callback, IntPtr.Zero)) {
                throw new System.ComponentModel.Win32Exception();
            }
        } finally {
            GC.KeepAlive(callback);
            FreeLibrary(module);
        }
        return lines.ToArray();
    }
}
"@

$rows = @([DsGridMenuEvidence]::ReadAll($item.FullName) | ForEach-Object {
    $parts = $_ -split "`t", 2
    [pscustomobject]@{ command_id = [int] $parts[0]; path = $parts[1] }
})

$proofs = [ordered]@{}
foreach ($entry in $expected.GetEnumerator()) {
    $matches = @($rows | Where-Object {
        if ($_.command_id -ne $entry.Value.command_id) { return $false }
        $_.path.IndexOf($entry.Value.menu_pattern, [StringComparison]::OrdinalIgnoreCase) -ge 0
    })
    if ($matches.Count -eq 0) {
        throw "Command $($entry.Value.command_id) absent from expected paths in pinned executable menu resources"
    }
    $proofs[$entry.Key] = [ordered]@{
        command_id = $entry.Value.command_id
        matching_menu_paths = @($matches.path | Sort-Object -Unique)
    }
}

[ordered]@{
    schema = 'ds.pls.menu_evidence.v1'
    executable_path = $item.FullName
    executable_sha256 = $digest
    reports = $proofs
} | ConvertTo-Json -Depth 8
