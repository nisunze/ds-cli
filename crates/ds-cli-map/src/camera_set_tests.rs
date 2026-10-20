use super::*;

#[test]
fn center_is_exactly_one_bounded_lon_lat_pair() {
    assert_eq!(center("30.0619,-1.9441").unwrap(), [30.0619, -1.9441]);
    for raw in ["30", "30,-1,8", "181,-1", "30,-86", "a,-1"] {
        assert!(center(raw).is_err(), "{raw}");
    }
}
