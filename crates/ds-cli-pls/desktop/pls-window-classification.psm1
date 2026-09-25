Set-StrictMode -Version 2.0

function Split-PlsWindowRows {
    param(
        [Parameter(Mandatory = $true)][object[]] $Rows,
        [Parameter(Mandatory = $true)][long] $MainWindowHandle
    )
    if ($MainWindowHandle -le 0) { throw 'PLS-CADD main window handle is unavailable' }
    $frame = @($Rows | Where-Object {
        [long](($_ -split ' ')[0]) -eq $MainWindowHandle
    })
    if ($frame.Count -ne 1) {
        throw "Expected one PLS-CADD main frame by handle $MainWindowHandle; found $($frame.Count)"
    }
    return [ordered]@{
        frame = [string] $frame[0]
        others = @($Rows | Where-Object {
            [long](($_ -split ' ')[0]) -ne $MainWindowHandle
        })
    }
}

Export-ModuleMember -Function Split-PlsWindowRows
