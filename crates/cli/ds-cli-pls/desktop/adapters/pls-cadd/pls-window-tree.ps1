param(
    [Parameter(Mandatory = $true)]
    [int] $ProcessId
)

Add-Type -AssemblyName UIAutomationClient

$root = [System.Windows.Automation.AutomationElement]::RootElement
$windows = $root.FindAll(
    [System.Windows.Automation.TreeScope]::Children,
    [System.Windows.Automation.Condition]::TrueCondition)

foreach ($window in $windows) {
    if ($window.Current.ProcessId -ne $ProcessId) {
        continue
    }
    Write-Output ("WINDOW handle={0} class={1} type={2} automation={3} name={4}" -f
        $window.Current.NativeWindowHandle,
        $window.Current.ClassName,
        $window.Current.ControlType.ProgrammaticName,
        $window.Current.AutomationId,
        $window.Current.Name)
    $children = $window.FindAll(
        [System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.Condition]::TrueCondition)
    foreach ($child in $children) {
        Write-Output ("  CHILD handle={0} class={1} type={2} automation={3} name={4}" -f
            $child.Current.NativeWindowHandle,
            $child.Current.ClassName,
            $child.Current.ControlType.ProgrammaticName,
            $child.Current.AutomationId,
            $child.Current.Name)
    }
}
