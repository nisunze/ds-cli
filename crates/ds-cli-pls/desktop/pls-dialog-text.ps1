param(
    [Parameter(Mandatory = $true)]
    [long] $WindowHandle
)

# Dump every named UIA descendant of one dialog — the file the Repair Wizard
# is currently asking about lives in a Static/Text control.
Add-Type -AssemblyName UIAutomationClient

$element = [System.Windows.Automation.AutomationElement]::FromHandle([IntPtr] $WindowHandle)
if ($null -eq $element) {
    throw "No UIA element for handle $WindowHandle"
}
$all = $element.FindAll(
    [System.Windows.Automation.TreeScope]::Descendants,
    [System.Windows.Automation.Condition]::TrueCondition)
foreach ($child in $all) {
    $name = $child.Current.Name
    if ($name) {
        Write-Output ("{0} | id={1} | {2}" -f
            $child.Current.ControlType.ProgrammaticName,
            $child.Current.AutomationId,
            $name)
    }
}
