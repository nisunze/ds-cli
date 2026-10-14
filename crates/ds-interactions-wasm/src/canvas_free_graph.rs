//! Retained free-card composition. Identity JSON crosses once; animated
//! placement updates and scene geometry use numeric buffers only.
use crate::canvas_graph::{CanvasGeometryBoundaryError, MAX_CANVAS_GRAPH_INPUT_BYTES};
use ds_canvas_graph::composition::{
    AutoArrangeInput, ComposeInput, ComposedCard, auto_arrange, compose,
};
use ds_canvas_graph::{GeometryError, Point};
use std::collections::BTreeMap;
use wasm_bindgen::prelude::*;

fn parse<T: serde::de::DeserializeOwned>(json: &str) -> Result<T, CanvasGeometryBoundaryError> {
    if json.len() > MAX_CANVAS_GRAPH_INPUT_BYTES {
        return Err(GeometryError::InputTooLarge {
            field: "freeGraphJsonBytes".into(),
            count: json.len(),
            max: MAX_CANVAS_GRAPH_INPUT_BYTES,
        }
        .into());
    }
    serde_json::from_str(json).map_err(|error| {
        GeometryError::InvalidJson {
            detail: error.to_string(),
        }
        .into()
    })
}

#[wasm_bindgen]
pub struct CanvasFreeGraph {
    input: ComposeInput,
    indices: BTreeMap<String, usize>,
    baseline: Vec<ComposedCard>,
}

#[wasm_bindgen]
impl CanvasFreeGraph {
    #[wasm_bindgen(constructor)]
    pub fn new(input_json: &str) -> Result<CanvasFreeGraph, CanvasGeometryBoundaryError> {
        let input: ComposeInput = parse(input_json)?;
        let baseline = compose(&input)?;
        let indices = input
            .nodes
            .iter()
            .enumerate()
            .map(|(i, node)| (node.id.clone(), i))
            .collect();
        Ok(Self {
            input,
            indices,
            baseline,
        })
    }

    /// `[count, ...[index,x,y,width,height,depth,container,expanded,collapsed,
    /// childCount,hiddenCount,...hiddenIndices]]`. Input is `[index,x,y,...]`.
    #[wasm_bindgen(js_name = composeBuffer)]
    pub fn compose_buffer(
        &self,
        positions: &[f64],
    ) -> Result<Vec<f64>, CanvasGeometryBoundaryError> {
        if !positions.len().is_multiple_of(3) || positions.len() > self.input.nodes.len() * 3 {
            return Err(GeometryError::InvalidBuffer {
                detail: "free graph positions require at most one index,x,y triple per node".into(),
            }
            .into());
        }
        if positions.is_empty() {
            return Ok(self.encode(&self.baseline));
        }
        let mut input = self.input.clone();
        let mut seen = vec![false; input.nodes.len()];
        for triple in positions.chunks_exact(3) {
            let index = triple[0];
            if !index.is_finite()
                || index.fract() != 0.0
                || index < 0.0
                || index >= input.nodes.len() as f64
            {
                return Err(GeometryError::InvalidBuffer {
                    detail: "free graph placement index is outside this scene".into(),
                }
                .into());
            }
            let index = index as usize;
            if seen[index] {
                return Err(GeometryError::InvalidBuffer {
                    detail: "duplicate free graph placement index".into(),
                }
                .into());
            }
            seen[index] = true;
            input.nodes[index].position = Some(Point::new(triple[1], triple[2]));
        }
        Ok(self.encode(&compose(&input)?))
    }
}

impl CanvasFreeGraph {
    fn encode(&self, cards: &[ComposedCard]) -> Vec<f64> {
        let mut out = vec![cards.len() as f64];
        for card in cards {
            out.extend([
                self.indices[&card.id] as f64,
                card.x,
                card.y,
                card.width,
                card.height,
                card.depth as f64,
                u8::from(card.container) as f64,
                u8::from(card.expanded) as f64,
                u8::from(card.collapsed) as f64,
                card.child_count as f64,
                card.hidden_ids.len() as f64,
            ]);
            out.extend(card.hidden_ids.iter().map(|id| self.indices[id] as f64));
        }
        out
    }
}

#[wasm_bindgen(js_name = canvasGraphAutoArrange)]
pub fn canvas_graph_auto_arrange(input_json: &str) -> Result<String, CanvasGeometryBoundaryError> {
    let input: AutoArrangeInput = parse(input_json)?;
    serde_json::to_string(&auto_arrange(&input)?).map_err(|error| {
        GeometryError::InvalidJson {
            detail: error.to_string(),
        }
        .into()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input() -> String {
        serde_json::json!({
        "nodes":[{"id":"a"},{"id":"b","parent":"a"}],"edges":[],
        "metrics":{"nodeWidth":192,"nodeHeight":64,"expandedWidth":286,"expandedHeight":188,"collapsedWidth":228,"collapsedHeight":76,"gridStep":24,"organizedLeft":32,"organizedTop":36,"organizedRow":88,"containerHeader":40,"containerPad":14,"childGapX":40,"childGapY":26,"dayWidth":7,"laneGap":44,"columnGap":64}
    }).to_string()
    }
    #[test]
    fn numeric_placements_are_transactional_and_keep_identity_order() {
        let session = CanvasFreeGraph::new(&input()).unwrap();
        let baseline = session.compose_buffer(&[]).unwrap();
        let moved = session.compose_buffer(&[0.0, 400.0, 800.0]).unwrap();
        assert_eq!(moved[0], 2.0);
        assert_eq!(&moved[1..4], &[0.0, 400.0, 800.0]);
        assert!(session.compose_buffer(&[0.0, f64::NAN, 0.0]).is_err());
        assert!(
            session
                .compose_buffer(&[0.0, 0.0, 0.0, 0.0, 1.0, 1.0])
                .is_err()
        );
        assert_eq!(session.compose_buffer(&[]).unwrap(), baseline);
    }
}
