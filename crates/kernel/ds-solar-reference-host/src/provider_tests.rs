use super::*;

fn request() -> ReferenceRequest {
    ReferenceRequest::new("A", "Country", 12.5, 13.5)
        .with_equipment(ReferenceEquipmentRequest { dc_ac_ratio: 1.3 })
        .with_losses(LossRequest {
            dc: DcLossRequest {
                soiling_percent: 5.0,
                shading_percent: 1.0,
                snow_percent: 0.0,
                mismatch_percent: 1.0,
                wiring_percent: 2.0,
                connections_percent: 0.5,
                lid_percent: 1.5,
                nameplate_percent: 1.0,
            },
            ac_inverter_percent: 2.0,
        })
        .with_capacity_override(CapacityOverride {
            target_inverter_capacity_kw: 100.0,
            target_module_capacity_w: 509.0,
            reason: "configured plant".to_string(),
        })
}

#[test]
fn cache_request_digest_moves_for_every_local_pv_input() {
    let baseline = request().cache_request_digest().expect("baseline digest");
    let mut variants = Vec::new();

    let mut dc_ac = request();
    dc_ac.equipment.as_mut().expect("equipment").dc_ac_ratio = 1.2;
    variants.push(dc_ac);

    for field in 0..8 {
        let mut changed = request();
        let dc = &mut changed.losses.as_mut().expect("losses").dc;
        match field {
            0 => dc.soiling_percent = 4.0,
            1 => dc.shading_percent = 2.0,
            2 => dc.snow_percent = 1.0,
            3 => dc.mismatch_percent = 2.0,
            4 => dc.wiring_percent = 3.0,
            5 => dc.connections_percent = 1.0,
            6 => dc.lid_percent = 2.0,
            _ => dc.nameplate_percent = 2.0,
        }
        variants.push(changed);
    }

    let mut ac = request();
    ac.losses.as_mut().expect("losses").ac_inverter_percent = 3.0;
    variants.push(ac);

    let mut inverter = request();
    inverter
        .capacity_override
        .as_mut()
        .expect("override")
        .target_inverter_capacity_kw = 125.0;
    variants.push(inverter);

    let mut module = request();
    module
        .capacity_override
        .as_mut()
        .expect("override")
        .target_module_capacity_w = 525.0;
    variants.push(module);

    let mut reason = request();
    reason.capacity_override.as_mut().expect("override").reason =
        "new source provenance".to_string();
    variants.push(reason);

    for changed in variants {
        assert_ne!(
            baseline,
            changed.cache_request_digest().expect("variant digest"),
            "a local PV request variant reused the baseline cache identity"
        );
    }
}

#[test]
fn cache_request_digest_ignores_display_labels() {
    let baseline = request().cache_request_digest().expect("baseline digest");
    let renamed = ReferenceRequest {
        city: "Renamed city".to_string(),
        country: "Renamed country".to_string(),
        ..request()
    };
    assert_eq!(
        baseline,
        renamed.cache_request_digest().expect("renamed digest")
    );
}
