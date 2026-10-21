@{
    Schema = 'ds.pls.report_coverage_profile.v1'
    ProjectFileName = 'Asbuilt_Rutsiro.xyz'

    # These bounds are properties of the qualified Rutsiro native model/report
    # set, not PLS-CADD defaults.  The structure endpoints are confirmed by the
    # characterized 16.81 Wind & Weight Span report.  The section bound and
    # Summary bounds are confirmed by the native 2026-08-02 report set and are
    # deliberately kept separate from dsgrid's canonical tension-row count.
    Reports = @{
        structure_usage = @{
            MinimumBytes = 100000
            RequiredLiterals = @(
                'Structure Locations and Usage Report'
                'Multiple Structure Minimum Vertical Load (Uplift) Summary'
            )
            ExpectedRows = 1129
            FirstStructureName = 'ex-w-S325.014'
            LastStructureName = 'm-c-2stay-NPD1000.012'
        }
        wind_weight_span = @{
            MinimumBytes = 1000000
            RequiredLiterals = @(
                'Structure Wind and Weight Spans Report'
                'Wind & Weight Span Report'
            )
            ExpectedRows = 1129
            FirstStructureName = 'ex-w-S325.014'
            LastStructureName = 'm-c-2stay-NPD1000.012'
            UnstrungStructureIds = @(638, 639, 640, 641, 811)
        }
        summary = @{
            MinimumBytes = 1000000
            RequiredLiterals = @(
                'Line Statistics:'
                'Total number of structures used:'
                'Total number of sections:'
                'Total number of alignment line angles:'
                'Structure List Report'
                'Structure Coordinates Report'
                'Structure Material List Report'
                'Cable Material List Report'
            )
            ExpectedRows = 1129
            AllowedStructuresUsed = @(1124, 1129)
            ExpectedNativeSections = 1878
        }
        section_usage = @{
            MinimumBytes = 100000
            RequiredLiterals = @('Sections Evaluated')
            ExpectedRows = 1878
        }
        section_tension = @{
            MinimumBytes = 1000000
            RequiredLiterals = @(
                'Section Sagging Data'
                'Ruling Span Sag Tension Report'
            )
            ExpectedRows = 1878
            ExpectedDetailReports = 1878
        }
    }
}
