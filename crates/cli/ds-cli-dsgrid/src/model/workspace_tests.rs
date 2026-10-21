use super::*;

#[test]
fn a_minted_id_is_one_path_segment() {
    for _ in 0..16 {
        let id = mint_id();
        assert!(
            ds_command_kernel::local_models::id_is_safe(&id),
            "{id} is not one safe path segment"
        );
    }
}

#[test]
fn every_kernel_refusal_this_family_can_raise_is_documented() {
    // The kernel's vocabulary is closed; a code it can return that no
    // command documents is exactly what `refusal_coverage.rs` refuses.
    for code in [
        ds_command_kernel::local_models::UNKNOWN_MODEL,
        ds_command_kernel::local_models::DUPLICATE_NAME,
        ds_command_kernel::local_models::CATALOGUE_FULL,
        ds_command_kernel::local_models::MALFORMED,
        ds_command_kernel::local_models::SCOPE_MISMATCH,
    ] {
        assert!(
            REFUSALS.iter().any(|refusal| refusal.code == code),
            "the kernel can refuse with `{code}` and no command says so"
        );
    }
}

#[test]
fn a_refusal_keeps_the_kernels_own_name() {
    let refused = refuse(StoreError::Refused {
        code: ds_command_kernel::local_models::UNKNOWN_MODEL.to_owned(),
        message: "no local model m-9 on this machine".into(),
    });
    assert_eq!(
        refused.code(),
        ds_command_kernel::local_models::UNKNOWN_MODEL
    );

    let unavailable = refuse(StoreError::Store("disk is full".into()));
    assert_eq!(unavailable.code(), STORE_UNAVAILABLE.code);
}
