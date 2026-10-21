//! Physical paper exclusions shared by native and WASM print planners.
//! A furniture collision is a refusal, never a weighted preference.
pub fn intersects(a: [f64; 4], b: [f64; 4], gap: f64) -> bool {
    a[0] < b[0] + b[2] + gap
        && a[0] + a[2] + gap > b[0]
        && a[1] < b[1] + b[3] + gap
        && a[1] + a[3] + gap > b[1]
}

pub fn clear(rect: [f64; 4], occupied: &[[f64; 4]], gap: f64) -> bool {
    occupied.iter().all(|other| !intersects(rect, *other, gap))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn furniture_rejects_label_and_linework_even_when_their_ink_is_sparse() {
        let occupied = [[120., 174., 18., 4.], [146., 180., 1., 36.]];
        assert!(!clear([112., 171., 58., 28.], &occupied, 1.));
        assert!(!clear([140., 172., 20., 22.], &occupied, 1.));
        assert!(clear([200., 174., 58., 28.], &occupied, 1.));
    }
}
