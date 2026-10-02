@{
    Schema = 'ds.pls.report_profile.v1'
    ProductVersion = '16.81x64'
    ExecutableSha256 = 'bf5cc5c3cde126ed2119303b5530f81d80222508e2815253a29709879c858650'
    Reports = @(
        @{
            Key = 'structure_usage'
            CommandId = 40014
            MenuPattern = 'Structure &Usage'
            OptionDialogTitle = $null
            ReportTitlePattern = 'Structure Usage Report'
            FileName = 'Structure Usage Report.txt'
        },
        @{
            Key = 'wind_weight_span'
            CommandId = 40020
            MenuPattern = '&Wind && Weight Span Report'
            OptionDialogTitle = 'Structure Wind and Weight Span Report'
            ReportTitlePattern = 'Wind & Weight Span Report'
            FileName = 'Wind and Weight Span Report.txt'
        },
        @{
            Key = 'summary'
            CommandId = 40019
            MenuPattern = '&Summary'
            OptionDialogTitle = $null
            ReportTitlePattern = 'Summary Report'
            FileName = 'Summary Report.txt'
        },
        @{
            Key = 'section_usage'
            CommandId = 40015
            MenuPattern = 'S&ection Usage'
            OptionDialogTitle = 'Select Structures for Section Usage'
            ReportTitlePattern = 'Section Usage Report'
            FileName = 'Section Usage Report.txt'
        },
        @{
            Key = 'section_tension'
            CommandId = 40403
            MenuPattern = 'Sectio&n Sag-Tension Report'
            OptionDialogTitle = 'Span Range for Section Sag-Tension'
            ReportTitlePattern = 'Section Sag-Tension Report'
            FileName = 'Section Sag-Tension Report.txt'
        }
    )
    ReportSaveAsCommandId = 33356
    ExitCommandId = 57665
    ProductDialogTitle = 'PLS-CADD'
    ExitSaveBodyPatterns = @(
        '^Save changes to .+\?$'
        '^OK to save project .+\?$'
    )
    ExitNoControlId = 7
    ExitNoTexts = @('No', '&No')
    AllowedPromptRules = @(
        @{
            Name = 'opposite_direction_warning'
            Title = 'Wires in Opposite Directions Warning'
            BodyPattern = '(?m)^\d+ spans have wires strung in opposite directions\.'
            ResponseControlId = 7
            ResponseText = 'No'
        },
        @{
            Name = 'insufficient_strength_criteria'
            Title = 'PLS-CADD'
            BodyPattern = '(?m)^Insufficient criteria to verify structure strength of '
            ResponseControlId = 7
            ResponseText = 'No'
        }
        @{
            Name = 'undefined_feature_codes'
            Title = 'Undefined Feature Codes'
            BodyPattern = '(?m)^39 Undefined feature codes found in terrain\. Program doesn''t know what these points are or what their required clearances are\. 7088 XYZ points with unknown feature codes\. 0 PFL points with unknown feature codes\. Continue displaying warning messages \(click No to redirect this and future messages to a report window for remainder of this operation\)\?$'
            ResponseControlId = 7
            ResponseText = 'No'
        }
    )
}
