//! Physical sheet diagnostics shared by native rendering and WASM hosts.
//! Hosts supply measured occupied rectangles after table flow/composition.
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Occupied {
    pub element_id: String,
    pub rect_mm: [f64; 4],
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Finding {
    pub code: String,
    pub elements: Vec<String>,
    pub rect_mm: [f64; 4],
    pub message: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Assessment {
    pub findings: Vec<Finding>,
    pub total: usize,
    pub truncated: bool,
}
pub fn assess(page_mm: [f64; 2], occupied: &[Occupied]) -> Result<Assessment, String> {
    if occupied.len() > 2000 || page_mm.iter().any(|v| !v.is_finite() || *v <= 0.) {
        return Err("invalid or unbounded print diagnostic input".into());
    }
    let mut findings = Vec::new();
    let mut total = 0;
    let mut add = |finding| {
        total += 1;
        if findings.len() < 200 {
            findings.push(finding);
        }
    };
    for (i, a) in occupied.iter().enumerate() {
        let [x, y, w, h] = a.rect_mm;
        if a.element_id.is_empty()
            || a.element_id.len() > 256
            || a.rect_mm.iter().any(|v| !v.is_finite())
            || w < 0.
            || h < 0.
        {
            return Err("invalid measured print element".into());
        }
        if x < -0.01 || y < -0.01 || x + w > page_mm[0] + 0.01 || y + h > page_mm[1] + 0.01 {
            add(Finding { code:"print_element_clipped".into(), elements:vec![a.element_id.clone()],rect_mm:a.rect_mm,
                message:format!("{} occupied rectangle [{x:.2}, {y:.2}, {w:.2}, {h:.2}] mm exceeds the {:.2} × {:.2} mm sheet",a.element_id,page_mm[0],page_mm[1]) });
        }
        for b in &occupied[..i] {
            if a.element_id == b.element_id {
                continue;
            }
            let bx = x.max(b.rect_mm[0]);
            let by = y.max(b.rect_mm[1]);
            let bw = (x + w).min(b.rect_mm[0] + b.rect_mm[2]) - bx;
            let bh = (y + h).min(b.rect_mm[1] + b.rect_mm[3]) - by;
            if bw > 0.01 && bh > 0.01 {
                add(Finding { code:"print_furniture_overlap".into(),elements:vec![b.element_id.clone(),a.element_id.clone()],rect_mm:[bx,by,bw,bh],
                    message:format!("{} and {} occupied rectangles overlap by {bw:.2} × {bh:.2} mm; move or resize the named elements",b.element_id,a.element_id) });
            }
        }
    }
    Ok(Assessment {
        truncated: total > findings.len(),
        findings,
        total,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn item(id: &str, r: [f64; 4]) -> Occupied {
        Occupied {
            element_id: id.into(),
            rect_mm: r,
        }
    }
    #[test]
    fn adjacent_unequal_panels_are_valid_but_overlap_and_clipping_are_named() {
        let a = item("poles", [5., 5., 20., 80.]);
        let b = item("customers", [25., 5., 20., 40.]);
        assert_eq!(assess([100., 100.], &[a.clone(), b]).unwrap().total, 0);
        let result = assess([100., 100.], &[a, item("legend", [20., 70., 20., 40.])]).unwrap();
        assert_eq!(result.total, 2);
        assert_eq!(result.findings[1].rect_mm, [20., 70., 5., 15.]);
    }
    #[test]
    fn details_are_bounded_without_losing_total() {
        let rows = (0..30)
            .map(|i| item(&i.to_string(), [1., 1., 5., 5.]))
            .collect::<Vec<_>>();
        let result = assess([100., 100.], &rows).unwrap();
        assert_eq!(result.total, 435);
        assert_eq!(result.findings.len(), 200);
        assert!(result.truncated);
    }
}
