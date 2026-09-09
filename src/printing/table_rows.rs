//! Physical table spacing shared by renderer and editor. Automatic row height
//! may use less padding; it never shrinks authored text or drops a data row.
#[derive(Debug)]
pub struct RowPlan {
    pub step_mm: f64,
    pub panels: usize,
    pub rows_per_panel: usize,
}
pub fn plan(
    id: &str,
    rows: usize,
    height: f64,
    font_pt: f64,
    row_mm: Option<f64>,
    maximum_panels: usize,
    styled: bool,
) -> Result<RowPlan, String> {
    if !height.is_finite()
        || height <= 0.
        || !font_pt.is_finite()
        || font_pt <= 0.
        || maximum_panels == 0
        || maximum_panels > 8
        || rows > 1_000_000
    {
        return Err("Invalid table row geometry.".into());
    }
    let minimum = font_pt * 25.4 / 72. * 1.25;
    let permitted = if styled { 1 } else { maximum_panels };
    let ideal = (font_pt * 25.4 / 72. * 1.6).max(3.);
    // Unspecified spacing is automatic. Spend available panel height before
    // refusing a table that fits perfectly at the same readable type size.
    let fit = height / (rows.max(1).div_ceil(permitted) + 2) as f64;
    let step = row_mm.unwrap_or(ideal.min(fit.max(minimum.max(3.))));
    if !step.is_finite() || step < minimum {
        return Err(format!(
            "table {id} row height cannot contain its readable type"
        ));
    }
    let capacity = ((height / step + 1e-9).floor() as usize)
        .checked_sub(2)
        .filter(|n| *n > 0)
        .ok_or("table frame needs room for its title, header and one row")?;
    let panels = rows.max(1).div_ceil(capacity);
    if panels > permitted {
        return Err(format!("table {id} has {rows} rows; needs {panels} panels at the authored type size (maximum {permitted})"));
    }
    Ok(RowPlan {
        step_mm: step,
        panels,
        rows_per_panel: rows.max(1).div_ceil(panels),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a3_agatare_keeps_all_26_rows_and_seven_point_type() {
        let p = plan("lv-print-info", 26, 99., 7., None, 1, false).unwrap();
        assert_eq!(p.panels, 1);
        assert_eq!(p.rows_per_panel, 26);
        assert!((p.step_mm * 28. - 99.).abs() < 1e-6);
        assert!(p.step_mm >= 7. * 25.4 / 72. * 1.25);
    }
    #[test]
    fn never_overrides_authored_spacing_or_omits_overflow() {
        assert!(plan("lv", 26, 99., 7., Some(4.), 1, false)
            .unwrap_err()
            .contains("needs 2 panels"));
        assert!(plan("lv", 100, 99., 7., None, 1, false).is_err());
        assert!(plan("lv", 26, 99., 7., Some(2.), 1, false).is_err());
        let p = plan("lv", 26, 99., 7., None, 2, false).unwrap();
        assert_eq!(p.panels, 2);
        assert_eq!(p.rows_per_panel, 13);
    }
}
