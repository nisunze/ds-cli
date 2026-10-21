//! Retained hit geometry for freely placed cards. The host supplies rectangles
//! in paint order and a dense parent table once; pointer queries send only
//! coordinates and intent. No task, milestone or workflow semantics live here.

use crate::error::{finite, finite_non_negative};
use crate::{GeometryError, Point, Rect};

pub const PORT_SCREEN_RADIUS: f64 = 6.;
pub const PORT_MIN_SCREEN_RADIUS: f64 = 3.;

/// Numeric ordering is intentional: the successor (right) port wins a tie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PortEnd {
    Successor = 0,
    Predecessor = 1,
}

impl TryFrom<u8> for PortEnd {
    type Error = GeometryError;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Successor),
            1 => Ok(Self::Predecessor),
            _ => Err(GeometryError::InvalidMetric {
                field: "portEnd".into(),
                value: value.to_string(),
                expected: "0 (successor) or 1 (predecessor)".into(),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CardPort {
    pub anchor: Point,
    pub radius: f64,
    pub visible: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortHit {
    pub index: usize,
    pub end: PortEnd,
}

pub struct FreeCards {
    rects: Vec<Rect>,
    children: Vec<Vec<usize>>,
}

impl FreeCards {
    /// Four coordinates per rectangle. The parent table includes one row per
    /// card, followed optionally by external parent identities. Keeping those
    /// identities preserves orphan-subtree exclusion without inventing cards.
    /// -1 is no parent; other entries address this same table. Cycles remain
    /// drawable and exclusion traversals terminate after visiting each id once.
    pub fn new(rectangles: &[f64], parents: &[i32]) -> Result<Self, GeometryError> {
        if !rectangles.len().is_multiple_of(4) || parents.len() < rectangles.len() / 4 {
            return Err(GeometryError::InvalidBuffer {
                detail:
                    "free cards require four coordinates per card and a parent entry for every card"
                        .into(),
            });
        }
        if parents.len() > i32::MAX as usize {
            return Err(GeometryError::InputTooLarge {
                field: "freeCardParents".into(),
                count: parents.len(),
                max: i32::MAX as usize,
            });
        }
        let rects = rectangles
            .chunks_exact(4)
            .enumerate()
            .map(|(index, values)| {
                let field = |name: &str| format!("cards[{index}].{name}");
                let rect = Rect {
                    x: finite(&field("x"), values[0])?,
                    y: finite(&field("y"), values[1])?,
                    width: finite_non_negative(&field("width"), values[2])?,
                    height: finite_non_negative(&field("height"), values[3])?,
                };
                finite(&field("maxX"), rect.max_x())?;
                finite(&field("maxY"), rect.max_y())?;
                Ok(rect)
            })
            .collect::<Result<Vec<_>, GeometryError>>()?;
        let mut children = vec![Vec::new(); parents.len()];
        for (index, parent) in parents.iter().copied().enumerate() {
            if parent == -1 {
                continue;
            }
            if parent < 0 || parent as usize >= parents.len() {
                return Err(GeometryError::InvalidBuffer {
                    detail: format!(
                        "free card parent {parent} at {index} is outside the parent table"
                    ),
                });
            }
            children[parent as usize].push(index);
        }
        Ok(Self { rects, children })
    }

    fn rect(&self, index: usize) -> Result<Rect, GeometryError> {
        self.rects
            .get(index)
            .copied()
            .ok_or_else(|| GeometryError::UnknownNode {
                reference: "freeCardIndex".into(),
                node: index.to_string(),
            })
    }

    pub fn port(&self, index: usize, end: PortEnd, zoom: f64) -> Result<CardPort, GeometryError> {
        let rect = self.rect(index)?;
        let anchor = Point::new(
            if end == PortEnd::Successor {
                rect.max_x()
            } else {
                rect.x
            },
            rect.y + rect.height / 2.,
        );
        // Preserve the established affordance rule: an unusable zoom hides the
        // port. It does not replace authored geometry or manufacture a hit.
        let radius = if zoom.is_finite() && zoom > 0. {
            (PORT_SCREEN_RADIUS / zoom)
                .min(rect.width / 3.)
                .min(rect.height / 2.)
        } else {
            0.
        };
        Ok(CardPort {
            anchor,
            radius,
            visible: radius * zoom >= PORT_MIN_SCREEN_RADIUS,
        })
    }

    pub fn boundary(&self, index: usize, toward: Point) -> Result<Point, GeometryError> {
        checked_point(toward)?;
        let rect = self.rect(index)?;
        let center = rect.center();
        let dx = finite("boundaryDx", toward.x - center.x)?;
        let dy = finite("boundaryDy", toward.y - center.y)?;
        if dx == 0. && dy == 0. {
            return Ok(Point::new(rect.max_x(), center.y));
        }
        let scale = 1.
            / ((dx.abs() / (rect.width / 2.).max(1.)).max(dy.abs() / (rect.height / 2.).max(1.)));
        let anchor = Point::new(center.x + dx * scale, center.y + dy * scale);
        checked_point(anchor)?;
        Ok(anchor)
    }

    pub fn hit(&self, point: Point) -> Result<Option<usize>, GeometryError> {
        checked_point(point)?;
        Ok(self.rects.iter().rposition(|rect| rect.contains(point)))
    }

    pub fn hit_port(
        &self,
        point: Point,
        zoom: f64,
        minimum_screen_radius: f64,
    ) -> Result<Option<PortHit>, GeometryError> {
        checked_point(point)?;
        finite_non_negative("minimumScreenRadius", minimum_screen_radius)?;
        for index in (0..self.rects.len()).rev() {
            for end in [PortEnd::Successor, PortEnd::Predecessor] {
                let port = self.port(index, end, zoom)?;
                if !port.visible {
                    continue;
                }
                let radius = port.radius.max(minimum_screen_radius / zoom);
                if radius > 0. && point.distance_to(port.anchor) <= radius {
                    return Ok(Some(PortHit { index, end }));
                }
            }
        }
        Ok(None)
    }

    pub fn drop_target(
        &self,
        point: Point,
        dragged: Option<usize>,
    ) -> Result<Option<usize>, GeometryError> {
        checked_point(point)?;
        let mut excluded = vec![false; self.children.len()];
        if let Some(dragged) = dragged {
            if dragged >= excluded.len() {
                return Err(GeometryError::UnknownNode {
                    reference: "draggedCardIndex".into(),
                    node: dragged.to_string(),
                });
            }
            let mut pending = vec![dragged];
            while let Some(index) = pending.pop() {
                if excluded[index] {
                    continue;
                }
                excluded[index] = true;
                pending.extend(self.children[index].iter().copied());
            }
        }
        Ok(self
            .rects
            .iter()
            .enumerate()
            .rfind(|(index, rect)| !excluded[*index] && rect.contains(point))
            .map(|(index, _)| index))
    }
}

fn checked_point(point: Point) -> Result<(), GeometryError> {
    finite("pointX", point.x)?;
    finite("pointY", point.y)?;
    Ok(())
}
