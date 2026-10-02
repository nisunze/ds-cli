param(
    [Parameter(Mandatory = $true)]
    [int] $ProcessId,
    [Parameter(Mandatory = $true)]
    [string] $OutputDirectory,
    [Parameter(Mandatory = $true)]
    [string] $Label
)

Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName UIAutomationClient

$root = [System.Windows.Automation.AutomationElement]::RootElement
$windows = $root.FindAll(
    [System.Windows.Automation.TreeScope]::Children,
    [System.Windows.Automation.Condition]::TrueCondition)
$window = $windows | Where-Object { $_.Current.ProcessId -eq $ProcessId } | Select-Object -First 1
if ($null -eq $window) {
    throw "No top-level window for process $ProcessId"
}

[System.IO.Directory]::CreateDirectory($OutputDirectory) | Out-Null
$bounds = $window.Current.BoundingRectangle
$width = [Math]::Max(1, [int][Math]::Ceiling($bounds.Width))
$height = [Math]::Max(1, [int][Math]::Ceiling($bounds.Height))
$bitmap = New-Object System.Drawing.Bitmap $width, $height
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
try {
    $graphics.CopyFromScreen(
        [int][Math]::Floor($bounds.X),
        [int][Math]::Floor($bounds.Y),
        0,
        0,
        $bitmap.Size)
    $bitmap.Save(
        [System.IO.Path]::Combine($OutputDirectory, "$Label.png"),
        [System.Drawing.Imaging.ImageFormat]::Png)
} finally {
    $graphics.Dispose()
    $bitmap.Dispose()
}

$elements = $window.FindAll(
    [System.Windows.Automation.TreeScope]::Descendants,
    [System.Windows.Automation.Condition]::TrueCondition)
$lines = foreach ($element in $elements) {
    $name = $element.Current.Name
    if (-not [string]::IsNullOrWhiteSpace($name)) {
        "{0}`t{1}" -f $element.Current.ControlType.ProgrammaticName, $name
    }
}
$lines | Set-Content -Encoding UTF8 ([System.IO.Path]::Combine($OutputDirectory, "$Label.txt"))
