@{
    Schema = 'ds.pls.backup_restore_profile.v1'
    ProductVersion = '16.81'
    ExecutableSha256 = 'bf5cc5c3cde126ed2119303b5530f81d80222508e2815253a29709879c858650'

    Commands = @{
        Backup = 33347
        Restore = 33348
        Exit = 57665
    }

    DialogTitles = @{
        Product = 'PLS-CADD'
        RestoreFile = 'Restore Backup'
        RestoreMapping = 'Directory Mapping For Restore'
        RestoreCommonPath = 'Change Common Directory Path'
        RestoreDirectory = "Select directory (Currently '')"
        RestoreReport = 'Restore Backup Report'
        BackupFile = 'Backup'
        BackupOptions = 'Backup Options'
        OppositeDirection = 'Wires in Opposite Directions Warning'
    }

    # The wizard appends the selected project name on some paths.  This is an
    # anchored allowlist, not a loose title prefix.
    RepairTitlePattern = "^PLS-CADD Project Repair Wizard(?: for '[^']+')?$"

    # Common-dialog and Win32 control ids. Every use is preceded by an exact
    # dialog-title, class, control-class, control-text and enabled-state check.
    Controls = @{
        Accept = 1
        Cancel = 2
        Yes = 6
        No = 7
        CommonFileName = 1148
        RestoreCommonPath = 1729
        RestoreNewPath = 1708
    }

    ExactButtons = @{
        RestorePickDirectory = @('Change Common Directory Path', '&Change Common Directory Path')
        RestoreNewPath = @('New Directory Path', '&New Directory Path')
        RestoreSelectDirectory = @('Select Folder', '&Select Folder')
        RestoreNewDirectory = @('New Directory', '&New Directory')
        Accept = @('OK', '&OK', 'Open', '&Open', 'Save', '&Save')
        Cancel = @('Cancel', '&Cancel')
        Yes = @('Yes', '&Yes')
        No = @('No', '&No')
    }

    AllowedOpenPrompts = @(
        @{
            Name = 'opposite_direction_warning'
            Title = 'Wires in Opposite Directions Warning'
            BodyPattern = '^\d+ spans have wires strung in opposite directions\.'
            ResponseControlId = 7
            ResponseTexts = @('No', '&No')
        },
        @{
            Name = 'insufficient_strength_criteria'
            Title = 'PLS-CADD'
            BodyPattern = '^Insufficient criteria to verify structure strength of '
            ResponseControlId = 7
            ResponseTexts = @('No', '&No')
        },
        @{
            Name = 'undefined_feature_codes'
            Title = 'Undefined Feature Codes'
            BodyPattern = '^39 Undefined feature codes found in terrain\. Program doesn''t know what these points are or what their required clearances are\. 7088 XYZ points with unknown feature codes\. 0 PFL points with unknown feature codes\. Continue displaying warning messages \(click No to redirect this and future messages to a report window for remainder of this operation\)\?$'
            ResponseControlId = 7
            ResponseTexts = @('No', '&No')
        },
        @{
            # Visual only (owner 2026-09-23): multi-alignment sheet paging cannot cut
            # pages; engineering data untouched. Catalogue: pp_paging_unable_to_cut.
            Name = 'pp_paging_unable_to_cut'
            Title = 'Warning'
            BodyPattern = '^Unable to cut pages\.\s+This may be because of the presence of multiple alignments'
            ResponseControlId = 2
            ResponseTexts = @('OK', '&OK')
        },
        @{
            # Follows pp_paging_unable_to_cut; visual only. Catalogue: pp_paging_no_progress.
            Name = 'pp_paging_no_progress'
            Title = 'Problem'
            BodyPattern = '^No progress in (find_next_page_start_station|place_sheets loop)'
            ResponseControlId = 2
            ResponseTexts = @('OK', '&OK')
        }
    )

    AllowedStartupPrompts = @(
        @{
            Name = 'about_pls_cadd_16_81'
            Title = 'About PLS-CADD'
            BodyPattern = '^PLS-CADD Version 16\.81x64\b.*Licensed to:'
            ResponseControlId = 1
            ResponseTexts = @('OK', '&OK')
        },
        @{
            Name = 'tip_of_the_day'
            Title = 'Tip of the Day'
            BodyPattern = 'Did you know\?'
            ResponseControlId = 1
            ResponseTexts = @('Close', '&Close')
        }
    )

    SaveBeforeBackupBodyPattern = '^OK to save project ''.+'' files before backing up\?\s+Note: If you do not save, the changes will not be in the backup file\.$'
    ExitSaveBodyPatterns = @(
        '^Save changes to .+\?$',
        '^OK to save project .+\?$'
    )

    # Backup Options is stateful. These exact checkbox texts are dangerous and
    # must be unchecked before the dialog may be accepted. Unknown checked
    # checkboxes are recorded but left unchanged because they control inclusion.
    DangerousBackupOptionTextPatterns = @(
        '(?i)^Compress backup file$',
        '(?i)transmit.+Power Line Systems',
        '(?i)technical support',
        '(?i)additional models'
    )

    AllowedFreshRestoreExtras = @()
    MaximumRepairDialogs = 400
    StartupTimeoutSeconds = 90
    DialogTimeoutSeconds = 90
    BackupTimeoutSeconds = 900
    ExitTimeoutSeconds = 60
}
