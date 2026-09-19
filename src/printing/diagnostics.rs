//! Physical sheet diagnostics shared by native rendering and WASM hosts.
//!
//! The contract. A sheet is a set of rectangles with meaning. The **furniture**
//! — title block, legend, information table, schedules, scale bar, north
//! arrow, logos, texts — must not overlap one another, must stay inside the
//! **paper** less a small **collision margin** (0 mm allowed, 1 mm the
//! preferred least), and must not cover the transformer's **design area**:
//! the network's fitted footprint on the map. The map frame around that
//! footprint is displayed map, not a hazard — furniture may sit over it
//! without a finding — and decorative frames are not furniture at all. Every
//! violation is a typed **warning**, never an error, measured in millimetres
//! and carrying the smallest move that would clear it, so a person or an
//! agent can improve a placement deliberately (`report layout edit`) instead
//! of guessing from a picture. Hosts supply the measured occupied rectangles
//! after table flow and composition; the engine adds the design area.
use serde::{Deserialize, Serialize};

/// What a measured rectangle is on the sheet. Ordering is mobility: when two
/// rectangles collide, the one with the higher role moves in the suggestion.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// The network's fitted footprint on the map: never moved, never clipped.
    DesignArea,
    TitleBlock,
    Logo,
    NorthArrow,
    ScaleBar,
    Text,
    InfoTable,
    Legend,
    /// Other measured furniture with no finer meaning.
    Furniture,
    /// Workbook schedules: the most movable, and the usual risk.
    Schedule,
}

impl Role {
    /// Design areas are what furniture must keep off; they never collide
    /// with each other and never clip.
    pub fn is_design(self) -> bool {
        self == Self::DesignArea
    }
}

/// The role of a page element, or `None` for what is not furniture: the map
/// frame (the design area inside it is measured by the engine) and decorative
/// rectangles.
pub fn role_of(element: &super::Element) -> Option<Role> {
    use super::ElementKind;
    Some(match element.kind {
        ElementKind::Map | ElementKind::Rectangle => return None,
        ElementKind::Legend => Role::Legend,
        ElementKind::ScaleBar => Role::ScaleBar,
        ElementKind::NorthArrow => Role::NorthArrow,
        ElementKind::Logo => Role::Logo,
        ElementKind::Text => Role::Text,
        ElementKind::Table => match &element.table {
            Some(table)
                if table.presentation == super::TablePresentation::WorkbookInfo
                    || table.layer == "lv_print_info" =>
            {
                Role::InfoTable
            }
            _ => Role::Schedule,
        },
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Occupied {
    pub element_id: String,
    pub rect_mm: [f64; 4],
    #[serde(default = "furniture")]
    pub role: Role,
}
fn furniture() -> Role {
    Role::Furniture
}

/// The preferred least collision margin: furniture inside 1 mm of the paper
/// edge is clipped for a printer, not merely close.
pub const DEFAULT_MARGIN_MM: f64 = 1.;
pub const MAX_MARGIN_MM: f64 = 20.;

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Finding {
    /// `print_furniture_overlap`, `print_design_overlap` or
    /// `print_element_clipped`. Every finding is a warning.
    pub code: String,
    pub elements: Vec<String>,
    /// The overlapping region, or the clipped element's rectangle.
    pub rect_mm: [f64; 4],
    pub message: String,
    #[serde(default)]
    pub roles: Vec<Role>,
    /// Width and height of the overlap, or of the part outside the paper.
    #[serde(default)]
    pub overlap_mm: [f64; 2],
    /// The element the suggestion moves, and the smallest axis-aligned move
    /// in millimetres that clears this finding alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub movable: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_move_mm: Option<[f64; 2]>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Assessment {
    pub findings: Vec<Finding>,
    pub total: usize,
    pub truncated: bool,
    #[serde(default = "default_margin")]
    pub margin_mm: f64,
}
fn default_margin() -> f64 {
    DEFAULT_MARGIN_MM
}

fn round(v: f64) -> f64 {
    (v * 100.).round() / 100.
}

/// The smallest of the four axis moves that separates `mover` from `fixed`.
fn clearing_move(mover: [f64; 4], fixed: [f64; 4]) -> [f64; 2] {
    let right = fixed[0] + fixed[2] - mover[0];
    let left = mover[0] + mover[2] - fixed[0];
    let down = fixed[1] + fixed[3] - mover[1];
    let up = mover[1] + mover[3] - fixed[1];
    let candidates = [[right, 0.], [-left, 0.], [0., down], [0., -up]];
    let best = candidates
        .iter()
        .min_by(|a, b| {
            (a[0].abs() + a[1].abs())
                .partial_cmp(&(b[0].abs() + b[1].abs()))
                .unwrap()
        })
        .unwrap();
    [round(best[0]), round(best[1])]
}

/// Assess with the preferred 1 mm collision margin.
pub fn assess(page_mm: [f64; 2], occupied: &[Occupied]) -> Result<Assessment, String> {
    assess_sheet(page_mm, DEFAULT_MARGIN_MM, occupied)
}

/// Assess the measured rectangles against the contract above.
pub fn assess_sheet(
    page_mm: [f64; 2],
    margin_mm: f64,
    occupied: &[Occupied],
) -> Result<Assessment, String> {
    assess_boundary([0., 0., page_mm[0], page_mm[1]], margin_mm, occupied)
}

/// Assess the actual printable border, retaining absolute sheet coordinates.
pub fn assess_boundary(
    boundary_mm: [f64; 4],
    margin_mm: f64,
    occupied: &[Occupied],
) -> Result<Assessment, String> {
    let page_mm = [boundary_mm[2], boundary_mm[3]];
    if boundary_mm.iter().any(|v| !v.is_finite())
        || occupied.len() > 2000
        || page_mm.iter().any(|v| !v.is_finite() || *v <= 0.)
    {
        return Err("invalid or unbounded print diagnostic input".into());
    }
    if !margin_mm.is_finite() || !(0. ..=MAX_MARGIN_MM).contains(&margin_mm) {
        return Err(format!(
            "the collision margin is 0 through {MAX_MARGIN_MM} mm"
        ));
    }
    let mut findings = Vec::new();
    let mut total = 0;
    let mut add = |finding| {
        total += 1;
        if findings.len() < 200 {
            findings.push(finding);
        }
    };
    let printable = [
        boundary_mm[0] + margin_mm,
        boundary_mm[1] + margin_mm,
        page_mm[0] - 2. * margin_mm,
        page_mm[1] - 2. * margin_mm,
    ];
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
        let beyond_left = (printable[0] - x).max(0.);
        let beyond_top = (printable[1] - y).max(0.);
        let beyond_right = (x + w - (printable[0] + printable[2])).max(0.);
        let beyond_bottom = (y + h - (printable[1] + printable[3])).max(0.);
        let outside = [beyond_left.max(beyond_right), beyond_top.max(beyond_bottom)];
        if !a.role.is_design() && (outside[0] > 0.01 || outside[1] > 0.01) {
            let dx = if beyond_left > 0.01 {
                beyond_left
            } else {
                -beyond_right
            };
            let dy = if beyond_top > 0.01 {
                beyond_top
            } else {
                -beyond_bottom
            };
            add(Finding {
                code: "print_element_clipped".into(),
                elements: vec![a.element_id.clone()],
                rect_mm: a.rect_mm,
                message: format!(
                    "{} [{x:.2}, {y:.2}, {w:.2}, {h:.2}] mm crosses the printable area of the {:.2} × {:.2} mm sheet ({margin_mm:.1} mm margin) by {:.2} × {:.2} mm; move it {} mm",
                    a.element_id,
                    page_mm[0],
                    page_mm[1],
                    outside[0],
                    outside[1],
                    describe([round(dx), round(dy)])
                ),
                roles: vec![a.role],
                overlap_mm: [round(outside[0]), round(outside[1])],
                movable: Some(a.element_id.clone()),
                suggested_move_mm: Some([round(dx), round(dy)]),
            });
        }
        for b in &occupied[..i] {
            if a.element_id == b.element_id || (a.role.is_design() && b.role.is_design()) {
                continue;
            }
            let bx = x.max(b.rect_mm[0]);
            let by = y.max(b.rect_mm[1]);
            let bw = (x + w).min(b.rect_mm[0] + b.rect_mm[2]) - bx;
            let bh = (y + h).min(b.rect_mm[1] + b.rect_mm[3]) - by;
            if bw <= 0.01 || bh <= 0.01 {
                continue;
            }
            // The more movable rectangle carries the suggestion; equal roles
            // move the later one, which is what a stack grows with.
            let (mover, fixed) = if a.role >= b.role { (a, b) } else { (b, a) };
            let shift = clearing_move(mover.rect_mm, fixed.rect_mm);
            let design = a.role.is_design() || b.role.is_design();
            add(Finding {
                code: if design {
                    "print_design_overlap".into()
                } else {
                    "print_furniture_overlap".into()
                },
                elements: vec![b.element_id.clone(), a.element_id.clone()],
                rect_mm: [bx, by, bw, bh],
                message: if design {
                    format!(
                        "{} covers the design area by {bw:.2} × {bh:.2} mm; move it {} mm, or let the map fit around it",
                        mover.element_id,
                        describe(shift)
                    )
                } else {
                    format!(
                        "{} and {} overlap by {bw:.2} × {bh:.2} mm; move {} {} mm or resize one of them",
                        b.element_id,
                        a.element_id,
                        mover.element_id,
                        describe(shift)
                    )
                },
                roles: vec![b.role, a.role],
                overlap_mm: [round(bw), round(bh)],
                movable: Some(mover.element_id.clone()),
                suggested_move_mm: Some(shift),
            });
        }
    }
    Ok(Assessment {
        truncated: total > findings.len(),
        findings,
        total,
        margin_mm,
    })
}

fn describe(shift: [f64; 2]) -> String {
    match shift {
        [dx, dy] if dy.abs() < 0.005 && dx < 0. => format!("{:.2} left", -dx),
        [dx, dy] if dy.abs() < 0.005 => format!("{dx:.2} right"),
        [_, dy] if dy < 0. => format!("{:.2} up", -dy),
        [_, dy] => format!("{dy:.2} down"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn item(id: &str, r: [f64; 4]) -> Occupied {
        Occupied {
            element_id: id.into(),
            rect_mm: r,
            role: Role::Furniture,
        }
    }
    fn role(id: &str, r: [f64; 4], role: Role) -> Occupied {
        Occupied {
            element_id: id.into(),
            rect_mm: r,
            role,
        }
    }
    #[test]
    fn adjacent_unequal_panels_are_valid_but_overlap_and_clipping_are_named() {
        let a = item("poles", [5., 5., 20., 80.]);
        let b = item("customers", [25., 5., 20., 40.]);
        assert_eq!(assess([100., 100.], &[a.clone(), b]).unwrap().total, 0);
        let result = assess([100., 100.], &[a, item("legend", [20., 70., 20., 40.])]).unwrap();
        assert_eq!(result.total, 2);
        assert_eq!(result.findings[0].code, "print_element_clipped");
        assert_eq!(result.findings[0].overlap_mm, [0., 11.]);
        assert_eq!(result.findings[0].suggested_move_mm, Some([0., -11.]));
        assert_eq!(result.findings[1].code, "print_furniture_overlap");
        assert_eq!(result.findings[1].rect_mm, [20., 70., 5., 15.]);
        assert_eq!(result.findings[1].movable.as_deref(), Some("legend"));
        assert_eq!(result.findings[1].suggested_move_mm, Some([5., 0.]));
        assert_eq!(result.margin_mm, DEFAULT_MARGIN_MM);
    }
    #[test]
    fn the_margin_is_the_printable_edge_and_zero_is_allowed() {
        let edge = item("scale", [0.5, 50., 20., 5.]);
        let one = assess_sheet([100., 100.], 1., &[edge.clone()]).unwrap();
        assert_eq!(one.total, 1);
        assert_eq!(one.findings[0].suggested_move_mm, Some([0.5, 0.]));
        assert_eq!(assess_sheet([100., 100.], 0., &[edge]).unwrap().total, 0);
        assert!(assess_sheet([100., 100.], -1., &[]).is_err());
        assert!(assess_sheet([100., 100.], 21., &[]).is_err());
    }
    #[test]
    fn the_design_area_is_a_hazard_but_the_map_frame_is_not() {
        let design = role("map", [30., 20., 40., 40.], Role::DesignArea);
        // A schedule over the map frame but off the network: no finding.
        let beside = role("customers", [75., 20., 20., 60.], Role::Schedule);
        assert_eq!(
            assess([100., 100.], &[design.clone(), beside])
                .unwrap()
                .total,
            0
        );
        // A schedule over the network: a typed warning that moves the schedule.
        let over = role("customers", [60., 50., 30., 40.], Role::Schedule);
        let result = assess([100., 100.], &[design.clone(), over]).unwrap();
        assert_eq!(result.total, 1);
        let finding = &result.findings[0];
        assert_eq!(finding.code, "print_design_overlap");
        assert_eq!(finding.roles, [Role::DesignArea, Role::Schedule]);
        assert_eq!(finding.rect_mm, [60., 50., 10., 10.]);
        assert_eq!(finding.movable.as_deref(), Some("customers"));
        assert_eq!(finding.suggested_move_mm, Some([10., 0.]));
        // The design area itself is never clipped and never collides with itself.
        let off = role("map2", [-5., -5., 20., 20.], Role::DesignArea);
        assert_eq!(assess([100., 100.], &[design, off]).unwrap().total, 0);
    }
    #[test]
    fn the_more_movable_role_carries_the_suggestion() {
        let title = role("title", [70., 80., 25., 15.], Role::TitleBlock);
        let schedule = role("poles", [60., 70., 20., 20.], Role::Schedule);
        let result = assess([100., 100.], &[title, schedule]).unwrap();
        assert_eq!(result.findings[0].movable.as_deref(), Some("poles"));
        assert_eq!(result.findings[0].suggested_move_mm, Some([-10., 0.]));
        assert!(result.findings[0].message.contains("move poles 10.00 left"));
        let legend = role("legend", [60., 70., 20., 20.], Role::Legend);
        let info = role("info", [70., 80., 25., 15.], Role::InfoTable);
        let result = assess([100., 100.], &[info, legend]).unwrap();
        assert_eq!(result.findings[0].movable.as_deref(), Some("legend"));
    }
    #[test]
    fn roles_follow_the_element_kind_and_its_binding() {
        let layout = super::super::default_layout();
        let roles: Vec<(String, Option<Role>)> = layout
            .elements
            .iter()
            .map(|e| (e.id.clone(), role_of(e)))
            .collect();
        assert!(roles.iter().any(|(_, r)| *r == Some(Role::Legend)));
        assert!(roles.iter().any(|(_, r)| *r == Some(Role::ScaleBar)));
        assert!(roles.iter().any(|(_, r)| r.is_none()));
        let (added, index) =
            super::super::elements::add(layout, super::super::ElementKind::Table, None, "")
                .unwrap();
        let mut table = added.elements[index].clone();
        assert_eq!(table.table.as_ref().unwrap().layer, "lv_print_info");
        assert_eq!(role_of(&table), Some(Role::InfoTable));
        table.table.as_mut().unwrap().layer = "customers".into();
        table.table.as_mut().unwrap().presentation = super::super::TablePresentation::Auto;
        assert_eq!(role_of(&table), Some(Role::Schedule));
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
