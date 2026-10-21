@{
    Schema = 'ds.pls.report_coverage_profile.v1'
    ProjectFileName = 'Asbuilt_Rutsiro.xyz'

    # Coverage bounds for the ACCEPTED 2026-08-16 Rutsiro submission
    # (Desktop\Rutsiro-Submission-20260816\Asbuilt_Rutsiro.bak, ZIP wrapper
    # sha256 6e7edced6d49c2bcc8e43554d15ac48a86d400c521b5f3e699f6344890409604,
    # native PLSBACKUPFILE 3.1 payload sha256
    # 2b897e20ba63eafad6cbcc70cc5fb4c9344056e05345809070f89f81bbaefe7e).
    #
    # These are NOT the 2026-08-02 bounds in pls-rutsiro-report-coverage.psd1.
    # That file pins 1,129 structures / 1,878 native sections; the accepted
    # model's live design block is 1,129 structures / 1,614 sections.  Keep both
    # files: each characterizes the model it names.
    #
    # STRUCTURE counts come from the .don carried inside the accepted backup,
    # from the LIVE design block only -- the block whose design-line header
    # three rows above `N;# structures` carries live bit 1, per
    # ds-io/src/pls_cadd_native/don.rs `don_design_block_is_active`.  The .don
    # also holds a stale 1,124-structure / 1,598-section block with live bit 0;
    # it is not what PLS-CADD reports on.  All values below were then confirmed
    # against the reports generated on 2026-08-17.
    #
    # UnstrungStructureIds is derived as "every attachment set of the structure
    # references section 0".  That derivation was validated by replaying it on
    # the pre-2026-08-16 model state
    # (Asbuilt_Rutsiro.don.before-pi-snap-20260816-133738.bak), where it
    # reproduces the independently recorded 638, 639, 640, 641, 811 exactly.
    # On the accepted model only structure 811 is still unstrung -- confirmed by
    # the Wind & Weight Span report, whose by-attachment-set and by-side tables
    # both omit exactly structure 811.
    #
    # SECTION counts are NOT the .don's section count and must not be derived
    # from it.  The live block declares 1,614 sections; PLS-CADD splits sections
    # at intermediate dead-end attachments when the project is opened (this
    # model: "280 dead end attachments found within sections") and drops six
    # sections whose end connections name attachment sets that do not exist
    # (611, 612, 835, 836, 1582, 1601).  The reported working total is 1,886.
    # This is the same mechanism behind the 1,878 in the 2026-08-02 profile.
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
            UnstrungStructureIds = @(811)
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
            AllowedStructuresUsed = @(1128)
            ExpectedNativeSections = 1886
        }
        section_usage = @{
            MinimumBytes = 100000
            RequiredLiterals = @('Sections Evaluated')
            ExpectedRows = 1886
        }
        section_tension = @{
            MinimumBytes = 1000000
            RequiredLiterals = @(
                'Section Sagging Data'
                'Ruling Span Sag Tension Report'
            )
            ExpectedRows = 1886
            ExpectedDetailReports = 1886
        }
    }
}
