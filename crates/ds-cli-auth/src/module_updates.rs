//! Native release authority capture; no caller-held JSON or URL enters here.
use crate::Lane;
use ds_cli_contract::Failure;

pub fn capture_release_authority(
    lane: Lane,
) -> Result<ds_update_runtime::modules::CapturedReleaseAuthority, Failure> {
    let (profile, digest, public_gateway_key) = crate::profile::load_installation(lane)?;
    ds_update_runtime::modules::CapturedReleaseAuthority::from_packaged_profile(
        profile,
        digest,
        public_gateway_key,
    )
    .map_err(|message| {
        Failure::unavailable("module_authority_unavailable", message)
            .remedy("reinstall the exact production native profile for this installed lane")
    })
}
