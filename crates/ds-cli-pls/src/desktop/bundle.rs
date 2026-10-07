//! The PLS-CADD desktop toolkit, embedded byte for byte, each file digest pinned.
//!
//! Every file below `crates/ds-cli-pls/desktop/` is product code owned here.
//! It is a thin layer facing third-party programs, kept in three areas:
//!
//! * `adapters/pls-cadd/` — PLS-CADD and PLS-POLE 16.81 GUI automation, the
//!   `ds-desktop-*.ps1` entries `ds` runs (one per verb, each writing the one
//!   result document `ds` reads), the dialog catalogue and native profiles;
//! * `adapters/word/` — Microsoft Word, RTF to PDF;
//! * `lab/` — authoring tools that still carry engineering logic and await a
//!   Rust owner. No verb runs them; they ship in the toolkit.
//!
//! Adapters operate the program and return raw evidence; Rust validates and
//! decides. The layer is disposable: without the default `desktop-adapters`
//! feature nothing is embedded, [`BUNDLE`] is empty, and every `ds pls
//! desktop` verb refuses `adapters_not_embedded`.
//!
//! A digest is written here by hand, on purpose. Changing a file means
//! changing its pin in the same commit, and the bundle test refuses a file on
//! disk that this table does not name, or a name whose bytes moved. The
//! repository's `.gitattributes` exempts `crates/ds-cli-pls/desktop/` from
//! line-ending conversion, so every checkout embeds the same bytes. Extracted
//! copies are disposable: [`extract`] writes and verifies a fresh folder for
//! every run and removes it, and nothing reads a previous extraction.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use ds_cli_contract::outcome::Failure;
use serde_json::json;
use sha2::{Digest, Sha256};

use super::BUNDLE_FAILED;

pub struct Script {
    /// Path below the bundle root, `/`-separated. The drivers find each other
    /// through `$PSScriptRoot`, so the layout is part of the contract.
    pub path: &'static str,
    pub bytes: &'static [u8],
    /// Lowercase hex sha256 of `bytes`.
    pub sha256: &'static str,
}

/// The areas of the bundle, in the order a toolkit answer counts them. Every
/// embedded path lies in exactly one.
pub const AREAS: &[&str] = &["adapters/pls-cadd/", "adapters/word/", "lab/"];

/// The entries and the PLS-CADD drivers live here; [`Entry::file`] and the
/// readers that quote a driver use it.
pub const PLS_CADD: &str = "adapters/pls-cadd/";

#[cfg(feature = "desktop-adapters")]
macro_rules! script {
    ($path:literal, $sha256:literal) => {
        Script {
            path: $path,
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/desktop/", $path)),
            sha256: $sha256,
        }
    };
}

#[cfg(feature = "desktop-adapters")]
pub static BUNDLE: &[Script] = &[
    script!(
        "adapters/README.md",
        "38c0a00f1a7afd8de2d4e0dd9bff8829d5a87fa1dfbe35bc91f4c0e7eeea36d7"
    ),
    script!(
        "adapters/pls-cadd/README.md",
        "e23378f90b9a0d1445f2943b7e3914b328e52cf1a846ad93c59e8043c9896e1c"
    ),
    script!(
        "adapters/pls-cadd/ds-desktop-autosag.ps1",
        "daaceaeaff5a972cc2d38ec4d7c98f340628f8d6ce205a2a5ceadb68b85f500e"
    ),
    script!(
        "adapters/pls-cadd/ds-desktop-check.ps1",
        "a0ea4caa370025125e74ba1b8d9d6dc42dd217e6e08289a83a0698828888e66c"
    ),
    script!(
        "adapters/pls-cadd/ds-desktop-deliver.ps1",
        "557c54c11b6e6fb4b6b0cb092a9b74c8eb8472aad3d8aa92dfdd612faf5428f2"
    ),
    script!(
        "adapters/pls-cadd/ds-desktop-lib.ps1",
        "a52498d3d1803bc44d0cb71c8cb18d384ee32cebbd111aec4c6db7ff27365995"
    ),
    script!(
        "adapters/pls-cadd/ds-desktop-qualify.ps1",
        "9caa1a5491598322bee3a8d560a88c6be2c68df5ec2dcd80f48c5fe345d6d8a8"
    ),
    script!(
        "adapters/pls-cadd/ds-desktop-reports.ps1",
        "1a1d2c740809e46ebab26e66f9605a8a98f2ebbeb29e7716890094f104d7b40a"
    ),
    script!(
        "adapters/pls-cadd/ds-desktop-restore.ps1",
        "8b87402aab2648ac7225b08ea342efaf5f7399c04089f1bfe6d4089cba24e8e2"
    ),
    script!(
        "adapters/pls-cadd/ds-desktop-sheets-pdf.ps1",
        "1a11fe770edcdd147e9b11745d22a4f4e530d069128e0d528b3d73c2fec78433"
    ),
    script!(
        "adapters/pls-cadd/interim/README.md",
        "9a278a49ce423a02616a2753d173531b6dfe67820272ee4f51726438d6d7be8b"
    ),
    script!(
        "adapters/pls-cadd/interim/pls-backup-open-interim.ps1",
        "5554b970ef0831f5ff6c30e3f007a9ca7effc09322c4bf06c13ade516efb972b"
    ),
    script!(
        "adapters/pls-cadd/interim/pls-close-interim.ps1",
        "1f520968b57002364153e1b16aaa6971f6d0445cbfccb9a2fad1337e2e065a1a"
    ),
    script!(
        "adapters/pls-cadd/interim/pls-interim-loader.ps1",
        "d358eadf79f2b1099d3f490178531abb71e7c9d54e054c72639b3dfe08f9ab4b"
    ),
    script!(
        "adapters/pls-cadd/interim/pls-quick-restore-interim.ps1",
        "9ecd51b3323ba4635f75f8dfd4656fa54e3059a400182ae5fbe1b78dc5f5940a"
    ),
    script!(
        "adapters/pls-cadd/interim/pls-restore-open-interim.ps1",
        "82abddb840968945c60d8125627e8af8a1a7b3cd35358b1c203881ff9602e177"
    ),
    script!(
        "adapters/pls-cadd/interim/pls-verify-restored-symmetric.ps1",
        "85ea2a710aed330b58e42b9ef9e6507cc5a25796911c63651dc3a8fe901afd8c"
    ),
    script!(
        "adapters/pls-cadd/open-verified-restore.ps1",
        "93b3c644c055270a22b477a3bd714f631157c338d3f2e2bda2980a9762b1804b"
    ),
    script!(
        "adapters/pls-cadd/pls-backup-restore-lib.psm1",
        "be6535c87fe737bbd81616d800063d22bd89f992c074e9c1e7b662f07add44dc"
    ),
    script!(
        "adapters/pls-cadd/pls-backup-restore-profile.psd1",
        "bc6ce626b1f20ec4e36e65caa4ddcfca58bff7582188215b771a1a7ab04edd8a"
    ),
    script!(
        "adapters/pls-cadd/pls-backup-restore-qualify.ps1",
        "b1c871a06746bd3b0c5886787170c1ae19a8a49f45ae1752637d3ebd3c33ecd1"
    ),
    script!(
        "adapters/pls-cadd/pls-bak-replace-files.py",
        "c27670a90d2bbebd814f19b4b07e595bed4562a46dfba8bffdfe3eca5965c74f"
    ),
    script!(
        "adapters/pls-cadd/pls-cable-edit.ps1",
        "0820f3232844fef04ecdc122257b8b84cdc2bf9170764ecd579e636981780428"
    ),
    script!(
        "adapters/pls-cadd/pls-cadd-16.81-menu-commands.tsv",
        "f73339663f95d954fea1f5e5159542e06df5a181aec2cb163cedffe32df870f8"
    ),
    script!(
        "adapters/pls-cadd/pls-capture-evidence.ps1",
        "09b08808dfa5769acde2ad9f62a4de57bfb2858d8fe345e268429d7a43df878e"
    ),
    script!(
        "adapters/pls-cadd/pls-click-at.ps1",
        "1d91d36cb9d62aa0a6a3d8d19d58fd1fbfe4aa06a3b956697bdaa90f9e887d1b"
    ),
    script!(
        "adapters/pls-cadd/pls-click.ps1",
        "cacc42c0038e5d5281c07116c83161eadabfcae1c5452b1c81861f5aa494d0ce"
    ),
    script!(
        "adapters/pls-cadd/pls-close-report-session.ps1",
        "24394f1ab36974361ca654f1fcb964045c9446104b4151014c92a75118dccedf"
    ),
    script!(
        "adapters/pls-cadd/pls-close-window.ps1",
        "24ce37c94436208289246d266aeedfebe385a518e0b98feec5670fbc45d2d03e"
    ),
    script!(
        "adapters/pls-cadd/pls-combo.ps1",
        "c6d5341267cdb879a2a621352f2a49d5c0d62f1ef65ac19d8c6f081bf3782dac"
    ),
    script!(
        "adapters/pls-cadd/pls-command.ps1",
        "303e6518e97862967c9314b9906b2aecb90994aac69c328dcacc3fefcb58119b"
    ),
    script!(
        "adapters/pls-cadd/pls-context-menu.ps1",
        "bb2e5f5b8713162d0890f089c8a4efced5c771efeab5ea8d99d9a8d6767364ed"
    ),
    script!(
        "adapters/pls-cadd/pls-control.ps1",
        "073bae78395205491b1b11f05ed2d249166c55ebf7fe7371025767b7bdb5e776"
    ),
    script!(
        "adapters/pls-cadd/pls-deliver-autosag.ps1",
        "78bb6bd83ddc32b3562fa2e821311db7c60a19834b1539328ee69759d2f56ce1"
    ),
    script!(
        "adapters/pls-cadd/pls-dialog-capture.ps1",
        "dd34d469ebaa3c0d2af762ca440b863cf6eb28eec38fcee4a3b334130c9f6ac7"
    ),
    script!(
        "adapters/pls-cadd/pls-dialog-catalog.psd1",
        "4b9831157354ee36160d2c8a347f212af87949c7e36ac8457a1a7d914f7632ab"
    ),
    script!(
        "adapters/pls-cadd/pls-dialog-fill.ps1",
        "39225daa2d59188c2458daaf430218f6dcbec83116a4f926ccebd7e6dfb7f33c"
    ),
    script!(
        "adapters/pls-cadd/pls-dialog-path.ps1",
        "f24582dd80e3c56c4460cdfb678799cab186875090551963301a6537b92f9e49"
    ),
    script!(
        "adapters/pls-cadd/pls-dialog-set.ps1",
        "7d36b5d055294e12ad4dbbf142367469272ba2d2b049f3cb59027f2842dc0cda"
    ),
    script!(
        "adapters/pls-cadd/pls-dialog-text.ps1",
        "4a5f369f666aef39ecf76497c6d1dc5acc4511d7e3fa378fb7f6c8dec7004fa6"
    ),
    script!(
        "adapters/pls-cadd/pls-dialog-watch.ps1",
        "bc3be8fec8b72d6638e17d86ca1bbd489b1f6055889e60be1da2832e79f5abe2"
    ),
    script!(
        "adapters/pls-cadd/pls-dismiss-startup.ps1",
        "8cb7e5aabcef29667f74e530aa88b00ec87fcefb252e7868e21a302ec643d78a"
    ),
    script!(
        "adapters/pls-cadd/pls-don-sheet-patch.py",
        "03646e7d5c3447a40e0e16ab86a36183e981442662723a708a9d795edb7ab7e9"
    ),
    script!(
        "adapters/pls-cadd/pls-exit-nosave.ps1",
        "16673a64a37f67213dda8feea4e11f3855b5d3b46d4a3fcf559c7ddbb5318c8d"
    ),
    script!(
        "adapters/pls-cadd/pls-goto-structure.ps1",
        "3ec9cb9f4831193dc0403f03c5a3ebda8ec8cf02a28d7866f5c785cb2ba6104e"
    ),
    script!(
        "adapters/pls-cadd/pls-handle-report-prompt.ps1",
        "e33d19f842bc24de85dd61fc44a51b0a866680470d7e54562f1cbef640e865a4"
    ),
    script!(
        "adapters/pls-cadd/pls-handle-rutsiro-open-prompts.ps1",
        "3040c978e2ce946679e5d340f0a448faf011007125a1db53a73690b9432eaa45"
    ),
    script!(
        "adapters/pls-cadd/pls-ini-set.py",
        "110473b8d178c12ccc72f47195399e3e2e80c13dabb4cba8fdc68a1c46247618"
    ),
    script!(
        "adapters/pls-cadd/pls-inspect.ps1",
        "594e92a3e5d468aea8040f86b5e95b05e25e3ea9c01079d03fcdfb70e7623100"
    ),
    script!(
        "adapters/pls-cadd/pls-known-restore-open.ps1",
        "e9ae1ab96405273f31fdbd3e08d47b47425ec44ecc69aa10af0a10c96a872bbf"
    ),
    script!(
        "adapters/pls-cadd/pls-launch-project.ps1",
        "eb09a372543864afc76cdd0c70b405a448ac7f5528089d325ab9437ab81ec9b0"
    ),
    script!(
        "adapters/pls-cadd/pls-load-file.ps1",
        "c069ac531b8d28b504e977653786967cb2d18dc8e0f1ac1f31b31c0b30cd558e"
    ),
    script!(
        "adapters/pls-cadd/pls-menu-dump.ps1",
        "b6bbcf4c3f7a61be64665dceb125a346d03d36cce9d2f28714f5a3af399c1a24"
    ),
    script!(
        "adapters/pls-cadd/pls-menu-evidence.ps1",
        "781b1735f19de7f342618a8878ac6e10bb79e4b0f75d024bae50b07593a6b11e"
    ),
    script!(
        "adapters/pls-cadd/pls-modal-children.ps1",
        "8d44989d0ff85480f3187ac5fc655e95f4073693a609fae37fa51d84430f26e9"
    ),
    script!(
        "adapters/pls-cadd/pls-native-check.ps1",
        "50ed1d4553543d1e294f521e170cdcbf876fef203d23b01fe872e7e37d13c6bd"
    ),
    script!(
        "adapters/pls-cadd/pls-navigate-open.ps1",
        "34cd6beebab323c52c6cb038c21785f051c111186639d4168bfcd523dc3e5334"
    ),
    script!(
        "adapters/pls-cadd/pls-open-project.ps1",
        "53d5cad6b7dfaa8c6fdf5990897bf5840f7dbe0e5ef9fb96dfa497a4aa7fae34"
    ),
    script!(
        "adapters/pls-cadd/pls-optimum-run.ps1",
        "cbfa704d176d6d52e9cf53b2133dde44d1a7b052d10a57f2de838745e98a52e1"
    ),
    script!(
        "adapters/pls-cadd/pls-pick-folder.ps1",
        "e01bd4e0158d07fec41cb2317532bac7172386831d3fa4d00dfa5b60839408a1"
    ),
    script!(
        "adapters/pls-cadd/pls-pole-hold.ps1",
        "ca90d9c93f79f3dd3d7d64462941bd5eccc81216fba7ec2a78d859aa208edc55"
    ),
    script!(
        "adapters/pls-cadd/pls-pole-run.ps1",
        "8808654bad90915def025a3cd56ab83593758a7afc0647d802e3261d477de36c"
    ),
    script!(
        "adapters/pls-cadd/pls-press.ps1",
        "3470a799144462a21e58e0352425343e4ecbd11dd0ad8b49372c2c6727de791a"
    ),
    script!(
        "adapters/pls-cadd/pls-print-sheets-pdf.ps1",
        "e4a880319e35a2bdc4734352359347b20d30975d7da76396182f432ef702dc46"
    ),
    script!(
        "adapters/pls-cadd/pls-printwindow.ps1",
        "18c41954fa427b82ae1aff199c8a6c7422686cc78b7499610e36270ed2688c34"
    ),
    script!(
        "adapters/pls-cadd/pls-repair-continue.ps1",
        "a9a8943570dd160e94894fd6450ed9cf81bd98ef8bab7ee9bd480597628fa183"
    ),
    script!(
        "adapters/pls-cadd/pls-report-any.ps1",
        "7e1e8518c153ed7749088fe30d2364f27725396f3bce97e2873c4510cb06ed59"
    ),
    script!(
        "adapters/pls-cadd/pls-report-bundle-tests.ps1",
        "74ce05074107e40f27d851414c114d8a7052fb396e85e9334a784542b9201b93"
    ),
    script!(
        "adapters/pls-cadd/pls-report-bundle.ps1",
        "8edfdf386be6fe927992e50db684d91ebfe3ca13a63fd0b9d4dc9de931c6ecdc"
    ),
    script!(
        "adapters/pls-cadd/pls-report-package.ps1",
        "e075e380b8292881ffa5737c962f086455f24916273889fcf4e22ed62528070c"
    ),
    script!(
        "adapters/pls-cadd/pls-report-profile.psd1",
        "8875545ff0a6ac48d3928d25b9a9848cdcd9087c57209fb3310c48742009d83c"
    ),
    script!(
        "adapters/pls-cadd/pls-report-rows.ps1",
        "d6d28b77c0100d99d8e796ac8898d0c67bdebf0c1e5495b048c953553fb5d4cf"
    ),
    script!(
        "adapters/pls-cadd/pls-report-run.ps1",
        "65de25b22c08eb671165a793993f9d09e7646ef79c70e26a9e72ae49deddd472"
    ),
    script!(
        "adapters/pls-cadd/pls-restore-provenance.ps1",
        "ecce57fe29f6baf554405cfdaefeb6e4b9c92d461a1de98d511ab4e8676bb066"
    ),
    script!(
        "adapters/pls-cadd/pls-rutsiro-report-coverage-20260816.psd1",
        "3e5d50212fe04f3bdc89cd9a45e25a7dd993dc0a3907c74e9dec08cb55f09d48"
    ),
    script!(
        "adapters/pls-cadd/pls-rutsiro-report-coverage.psd1",
        "6e6084b701f7d8bf33bea0702c1b5131de1b52f98c89e8e742337b65189dbe85"
    ),
    script!(
        "adapters/pls-cadd/pls-safe-dump.ps1",
        "e764e221c447fc7b46d7039e2ea90fd9f62b2877de8d8e0f7a1546dc16a674af"
    ),
    script!(
        "adapters/pls-cadd/pls-save-report-file.ps1",
        "e51c485ced26153a8d81439dec0388d38f3e61d4b04851529813a42ae193db07"
    ),
    script!(
        "adapters/pls-cadd/pls-save-sheets-pdf.ps1",
        "6e6268307cd5ef9e5abf243a4171d0bb343c58a0fef2f7efef79a91900d63b44"
    ),
    script!(
        "adapters/pls-cadd/pls-section-table-autosag.ps1",
        "0a5e5bb299c167afa478f58df4f1189564d5f45de5ef35133764fbec0eb78fd0"
    ),
    script!(
        "adapters/pls-cadd/pls-session-hold.ps1",
        "1234e734f7dc56dba46e24f18cfd2e2f5d36bb17c3ac00864d153b956b828ded"
    ),
    script!(
        "adapters/pls-cadd/pls-sheet-paging.ps1",
        "4ddd574a5855e0a8caeb2cdb61277b508a4ae1cda2dc15f84b857ad1a3a0241b"
    ),
    script!(
        "adapters/pls-cadd/pls-state.ps1",
        "62f2790aee356d0932fda1613616bde1e8ab422c55d3fb0b55182fde3a508a2a"
    ),
    script!(
        "adapters/pls-cadd/pls-type-path.ps1",
        "caef0ecc10d0559c27dadc9c46ec1186cefe8500a7849f95c02f6f1176f44f1c"
    ),
    script!(
        "adapters/pls-cadd/pls-units-check.ps1",
        "ee02f743a7032fe488af492ef0403b54afeae328724dae06a9b56a29463063c1"
    ),
    script!(
        "adapters/pls-cadd/pls-window-classification.psm1",
        "516a8f4651fa59c33724dfb0f780391530a74759bfbfa5a9e0045526dd2e4ec2"
    ),
    script!(
        "adapters/pls-cadd/pls-window-tree.ps1",
        "1dc5f0b430a7f03cd0bc351f3fdf5cb318f54ab5b2e8b9727e03cedd783b8984"
    ),
    script!(
        "adapters/pls-cadd/pls-windows.ps1",
        "c3f956a0d6ecec73faa02d9da3e5848e8eeecb9031838524569e7688ffe4b55b"
    ),
    script!(
        "adapters/pls-cadd/tests/pls-backup-restore-contract.tests.ps1",
        "4adc2997686ae52d802b373ba0ab2fbe718b84fc2bfffc6b3980e87562147816"
    ),
    script!(
        "adapters/word/README.md",
        "845ee115d8d7ae4614419e72ffac61a470f46872c515b0a3393ec004876e8297"
    ),
    script!(
        "adapters/word/pls-rtf-to-pdf.ps1",
        "7f41703daa33e60e6377e7dd07b6ac2b455f20dd77cba5e2493c903e6b99c6a8"
    ),
    script!(
        "lab/README.md",
        "80b9b75e6cf6de5feca3dc9b9bcbffd6ff6c9987ea303d5138c31b661ae06345"
    ),
    script!(
        "lab/pls-components-author.py",
        "7f71756764140f9df88de4674eaca27c047e791e11b91d5a04752ac1c9a832a2"
    ),
    script!(
        "lab/pls-library-manifest.py",
        "a9f96c9e48b9296173f91ed5604a8c9c791b89f46e69e937aeb20507508b2e5f"
    ),
    script!(
        "lab/pls-lic-write.py",
        "e3279ded32dbf416eaf630179437d0cdcdcd370734dfe3c06b044b71a5d7e702"
    ),
    script!(
        "lab/pls-make-fictional-sheet-assets.py",
        "5984c9d9f6b9d5008ec9693193bc385557303f1941fc406ca9882ebf58071f17"
    ),
    script!(
        "lab/pls-pole-author.py",
        "8e683028730315379f41de61656d0e489fe986a5d0bd6b8959344439ef3b7bbd"
    ),
    script!(
        "lab/pls-pole-family.py",
        "e44c95cd58404fd3391c801500adaec272019a6be44b6028762525c124cec11a"
    ),
    script!(
        "lab/pls-sagtension.py",
        "c033aa440576f70b506368e73552363c2e19e966800b91b4d8beb7ae7c82d54c"
    ),
    script!(
        "lab/pls-wire-loads.py",
        "eebf6dcabdfa4ccf55ce355ca20601920b8d78c86d3e9bbd5d7735f3e582d407"
    ),
];

/// Built without the adapter layer: nothing is embedded and every desktop verb
/// refuses before it would need a file.
#[cfg(not(feature = "desktop-adapters"))]
pub static BUNDLE: &[Script] = &[];

/// Whether this build carries the adapter layer at all.
pub const fn embedded() -> bool {
    cfg!(feature = "desktop-adapters")
}

/// One entry script `ds` may run. A closed set: the run boundary takes one of
/// these, never a file name from a caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    Check,
    Restore,
    Qualify,
    Deliver,
    Autosag,
    Reports,
    SheetsPdf,
}

impl Entry {
    pub const ALL: &[Self] = &[
        Self::Check,
        Self::Restore,
        Self::Qualify,
        Self::Deliver,
        Self::Autosag,
        Self::Reports,
        Self::SheetsPdf,
    ];

    pub const fn file(self) -> &'static str {
        match self {
            Self::Check => "adapters/pls-cadd/ds-desktop-check.ps1",
            Self::Restore => "adapters/pls-cadd/ds-desktop-restore.ps1",
            Self::Qualify => "adapters/pls-cadd/ds-desktop-qualify.ps1",
            Self::Deliver => "adapters/pls-cadd/ds-desktop-deliver.ps1",
            Self::Autosag => "adapters/pls-cadd/ds-desktop-autosag.ps1",
            Self::Reports => "adapters/pls-cadd/ds-desktop-reports.ps1",
            Self::SheetsPdf => "adapters/pls-cadd/ds-desktop-sheets-pdf.ps1",
        }
    }
}

pub fn script(path: &str) -> Option<&'static Script> {
    BUNDLE.iter().find(|script| script.path == path)
}

/// The UTF-8 text of an embedded file, for the readers that need it (the
/// dialog catalogue). Every file in the bundle is UTF-8.
pub fn text(path: &str) -> Option<&'static str> {
    script(path).and_then(|script| std::str::from_utf8(script.bytes).ok())
}

/// The text of a PLS-CADD adapter file, by its name in that folder.
pub fn pls_cadd_text(name: &str) -> Option<&'static str> {
    text(&format!("{PLS_CADD}{name}"))
}

/// The bundle's identity: one digest over every path and pin, in table order.
/// A receipt carries it, so a result can be traced to the exact drivers.
pub fn digest() -> String {
    let mut hasher = Sha256::new();
    for script in BUNDLE {
        hasher.update(script.path.as_bytes());
        hasher.update(b"\0");
        hasher.update(script.sha256.as_bytes());
        hasher.update(b"\n");
    }
    format!("sha256:{:x}", hasher.finalize())
}

/// The bundle, written into a fresh private folder for one run and removed
/// with it.
pub struct Extracted {
    root: PathBuf,
}

impl Extracted {
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn path(&self, relative: &str) -> PathBuf {
        below(&self.root, relative)
    }
}

impl Drop for Extracted {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Write every file into a new folder below `parent` and read each back
/// against its pin before anything runs from it.
///
/// The folder is created, never reused: a name that already exists is skipped
/// rather than written into, so no other process's files are ever executed.
pub fn extract(parent: &Path) -> Result<Extracted, Failure> {
    let root = fresh_folder(parent)?;
    let extracted = Extracted { root };
    materialize(extracted.root()).map_err(|unwritten| {
        Failure::failed(BUNDLE_FAILED.code, unwritten.message())
            .remedy(BUNDLE_FAILED.remedy)
            .detail(unwritten.detail())
    })?;
    Ok(extracted)
}

/// Why [`materialize`] stopped, before the caller names its own code.
#[derive(Debug)]
pub enum Unwritten {
    Io {
        path: PathBuf,
        error: std::io::Error,
    },
    Moved {
        path: &'static str,
        actual: String,
        expected: &'static str,
    },
}

impl Unwritten {
    pub fn message(&self) -> String {
        match self {
            Self::Io { path, .. } => format!("could not write {}", path.display()),
            Self::Moved { path, .. } => format!("{path} does not match its pinned digest"),
        }
    }

    pub fn detail(&self) -> serde_json::Value {
        match self {
            Self::Io { error, .. } => json!({ "detail": error.kind().to_string() }),
            Self::Moved {
                actual, expected, ..
            } => json!({ "expected": expected, "actual": actual }),
        }
    }
}

/// Write every embedded file below an existing, empty `root` with
/// `create_new`, then read each back against its pin.
pub fn materialize(root: &Path) -> Result<(), Unwritten> {
    let io = |path: &Path| {
        let path = path.to_path_buf();
        move |error| Unwritten::Io { path, error }
    };
    for script in BUNDLE {
        let path = below(root, script.path);
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder).map_err(io(&path))?;
        }
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(io(&path))?;
        file.write_all(script.bytes)
            .and_then(|()| file.sync_all())
            .map_err(io(&path))?;
    }
    for script in BUNDLE {
        let path = below(root, script.path);
        let bytes = std::fs::read(&path).map_err(io(&path))?;
        let actual = format!("{:x}", Sha256::digest(&bytes));
        if actual != script.sha256 {
            return Err(Unwritten::Moved {
                path: script.path,
                actual,
                expected: script.sha256,
            });
        }
    }
    Ok(())
}

/// A `/`-separated bundle path below `root`.
pub fn below(root: &Path, relative: &str) -> PathBuf {
    relative
        .split('/')
        .fold(root.to_path_buf(), |path, part| path.join(part))
}

fn fresh_folder(parent: &Path) -> Result<PathBuf, Failure> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    for attempt in 0..16u32 {
        let root = parent.join(format!(
            "ds-pls-desktop-{}-{:x}-{attempt}",
            std::process::id(),
            nanos
        ));
        match create_private(&root) {
            Ok(()) => return Ok(root),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(bundle_failure(&root, error)),
        }
    }
    Err(Failure::failed(
        BUNDLE_FAILED.code,
        "no fresh temporary folder name was free",
    )
    .remedy(BUNDLE_FAILED.remedy))
}

/// A folder only this user can read: explicit on Unix; on Windows the
/// per-user `%TEMP%` it is created in already carries that ACL.
fn create_private(root: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        std::fs::DirBuilder::new().mode(0o700).create(root)
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir(root)
    }
}

fn bundle_failure(path: &Path, error: std::io::Error) -> Failure {
    Failure::failed(
        BUNDLE_FAILED.code,
        format!("could not write {}", path.display()),
    )
    .remedy(BUNDLE_FAILED.remedy)
    .detail(json!({ "detail": error.kind().to_string() }))
}

#[cfg(all(test, feature = "desktop-adapters"))]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    fn on_disk() -> BTreeSet<String> {
        fn walk(root: &Path, folder: &Path, into: &mut BTreeSet<String>) {
            for entry in std::fs::read_dir(folder).expect("the bundle folder reads") {
                let path = entry.expect("an entry").path();
                if path.is_dir() {
                    walk(root, &path, into);
                } else {
                    let relative = path.strip_prefix(root).expect("below the root");
                    into.insert(relative.to_string_lossy().replace('\\', "/"));
                }
            }
        }
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("desktop");
        let mut files = BTreeSet::new();
        walk(&root, &root, &mut files);
        files
    }

    #[test]
    fn every_file_on_disk_is_embedded_and_nothing_else() {
        let declared: BTreeSet<String> = BUNDLE.iter().map(|s| s.path.to_string()).collect();
        assert_eq!(declared.len(), BUNDLE.len(), "a path is declared twice");
        assert_eq!(
            on_disk(),
            declared,
            "crates/ds-cli-pls/desktop and the BUNDLE table must name the same files"
        );
    }

    #[test]
    fn every_embedded_file_matches_its_pinned_digest() {
        for script in BUNDLE {
            assert_eq!(
                format!("{:x}", Sha256::digest(script.bytes)),
                script.sha256,
                "{} moved without its pin; if the change is deliberate, pin the new digest",
                script.path
            );
            assert!(
                std::str::from_utf8(script.bytes).is_ok(),
                "{} is not UTF-8",
                script.path
            );
        }
    }

    /// The layout is the third-party boundary: every file sits in one area,
    /// and every area says in its README which program it faces.
    #[test]
    fn every_file_lives_in_one_documented_area() {
        // The layer's own README states the rules for every area beneath it.
        for script in BUNDLE.iter().filter(|s| s.path != "adapters/README.md") {
            let areas = AREAS
                .iter()
                .filter(|area| script.path.starts_with(*area))
                .count();
            assert_eq!(areas, 1, "{} is outside the declared areas", script.path);
        }
        for readme in [
            "adapters/README.md",
            "adapters/pls-cadd/README.md",
            "adapters/word/README.md",
            "lab/README.md",
        ] {
            assert!(text(readme).is_some(), "{readme} is missing");
        }
        assert!(
            text("adapters/README.md")
                .unwrap()
                .contains("adapters_not_embedded")
        );
    }

    #[test]
    fn entries_write_their_outcome_through_the_shared_lib() {
        for entry in Entry::ALL {
            assert!(entry.file().starts_with(PLS_CADD), "{}", entry.file());
            let body = text(entry.file()).expect("an entry is UTF-8");
            assert!(
                body.contains("[Parameter(Mandatory = $true)][string] $ResultPath"),
                "{} must take the result path ds reads",
                entry.file()
            );
            assert!(
                body.contains(". (Join-Path $PSScriptRoot 'ds-desktop-lib.ps1')")
                    && body.contains("Invoke-DsEntry $ResultPath"),
                "{} must write its outcome through Invoke-DsEntry",
                entry.file()
            );
        }
        let deliver = pls_cadd_text("pls-deliver-autosag.ps1").unwrap();
        assert!(deliver.contains("[switch] $NoSheets"));
        assert!(deliver.contains("'ds.pls.deliver_autosag.v4'"));
    }

    /// Every `& (Join-Path $here '<file>')` and `"$here\<file>"` an embedded
    /// script runs must be in the bundle. A driver that calls a helper the
    /// bundle left behind fails only on the desktop, hours into a run. Only
    /// lines that locate a file beside the script count: the catalogue's notes
    /// mention helpers by name.
    #[test]
    fn every_script_a_driver_calls_is_embedded() {
        const LOCATORS: &[&str] = &["$here", "$PSScriptRoot", "$pls"];
        let names: BTreeSet<String> = BUNDLE
            .iter()
            .map(|s| s.path.rsplit('/').next().unwrap().to_string())
            .collect();
        let mut missing = Vec::new();
        for script in BUNDLE {
            let body = std::str::from_utf8(script.bytes).expect("UTF-8");
            let locating = body.lines().filter(|line| {
                !line.trim_start().starts_with('#')
                    && LOCATORS.iter().any(|locator| line.contains(locator))
            });
            for line in locating {
                for (at, _) in line.match_indices("pls-") {
                    let rest = &line[at..];
                    let end = rest
                        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '.'))
                        .unwrap_or(rest.len());
                    let name = rest[..end].trim_end_matches('.');
                    let callable = [".ps1", ".psm1", ".psd1"]
                        .iter()
                        .any(|suffix| name.ends_with(suffix));
                    if callable && !names.contains(name) {
                        missing.push(format!("{} -> {name}", script.path));
                    }
                }
            }
        }
        assert!(missing.is_empty(), "called but not embedded: {missing:?}");
    }

    /// The same, by location: a `Join-Path $here '<relative>'` or
    /// `Join-Path $PSScriptRoot '<relative>'` resolves from the caller's own
    /// folder to an embedded file. This is what holds a cross-area call such
    /// as `..\word\pls-rtf-to-pdf.ps1`.
    #[test]
    fn every_relative_call_resolves_inside_the_bundle() {
        let paths: BTreeSet<&str> = BUNDLE.iter().map(|s| s.path).collect();
        let mut unresolved = Vec::new();
        let mut resolved = 0;
        for script in BUNDLE.iter().filter(|s| s.path.ends_with(".ps1")) {
            let folder: Vec<&str> = script.path.split('/').collect();
            let folder = &folder[..folder.len() - 1];
            let body = std::str::from_utf8(script.bytes).expect("UTF-8");
            for line in body.lines().filter(|l| !l.trim_start().starts_with('#')) {
                for locator in ["Join-Path $here '", "Join-Path $PSScriptRoot '"] {
                    for (at, _) in line.match_indices(locator) {
                        let rest = &line[at + locator.len()..];
                        let Some(end) = rest.find('\'') else { continue };
                        let relative = &rest[..end];
                        if ![".ps1", ".psm1", ".psd1"]
                            .iter()
                            .any(|suffix| relative.ends_with(suffix))
                        {
                            continue;
                        }
                        let mut parts: Vec<&str> = folder.to_vec();
                        for part in relative.split(['\\', '/']) {
                            match part {
                                "" | "." => {}
                                ".." => {
                                    parts.pop();
                                }
                                part => parts.push(part),
                            }
                        }
                        let target = parts.join("/");
                        if paths.contains(target.as_str()) {
                            resolved += 1;
                        } else {
                            unresolved.push(format!("{} -> {relative}", script.path));
                        }
                    }
                }
            }
        }
        assert!(
            unresolved.is_empty(),
            "calls resolving outside the bundle: {unresolved:?}"
        );
        assert!(
            resolved > 20,
            "the scan found only {resolved} calls; it has stopped reading"
        );
        for caller in ["ds-desktop-reports.ps1", "pls-deliver-autosag.ps1"] {
            assert!(
                pls_cadd_text(caller)
                    .unwrap()
                    .contains(r"Join-Path $here '..\word\pls-rtf-to-pdf.ps1'"),
                "{caller} reaches Word through its adapter"
            );
        }
    }

    #[test]
    fn extraction_writes_the_layout_verifies_it_and_cleans_up() {
        let parent = std::env::temp_dir();
        let extracted = extract(&parent).expect("the bundle extracts");
        let root = extracted.root().to_path_buf();
        for script in BUNDLE {
            let bytes = std::fs::read(extracted.path(script.path)).expect("written");
            assert_eq!(bytes, script.bytes, "{}", script.path);
        }
        assert!(
            extracted
                .path("adapters/pls-cadd/interim/pls-interim-loader.ps1")
                .is_file()
        );
        assert!(extracted.path("adapters/word/pls-rtf-to-pdf.ps1").is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&root).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700, "the run folder is private");
        }
        let second = extract(&parent).expect("a second run gets its own folder");
        assert_ne!(second.root(), root.as_path());
        drop(extracted);
        assert!(!root.exists(), "the run folder is removed with the run");
    }

    #[test]
    fn materialize_refuses_a_folder_that_already_holds_a_file() {
        let root = std::env::temp_dir().join(format!("ds-pls-materialize-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir(&root).unwrap();
        materialize(&root).expect("an empty folder takes the bundle");
        let again = materialize(&root);
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            matches!(again, Err(Unwritten::Io { .. })),
            "create_new never overwrites"
        );
    }

    #[test]
    fn the_bundle_digest_is_stable_and_names_every_pin() {
        assert!(embedded());
        assert_eq!(digest(), digest());
        assert!(digest().starts_with("sha256:"));
        assert_eq!(digest().len(), "sha256:".len() + 64);
    }
}
