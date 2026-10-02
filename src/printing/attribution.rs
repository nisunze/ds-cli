//! The licensing mark is page furniture, never a template option.
pub const CREDIT: &str = "DS GridDesign by datasolutions.rw";
pub const SVG_ID: &str = "ds-grid-credit-v1";
pub const FONT_PT: f64 = 7.;
pub const COLOR: &str = "#595959";
pub const RIGHT_INSET_PT: f64 = 146.;
pub const BASELINE_PT: f64 = 7.;

/// Physical bottom-right placement shared by raster previews and PDF hosts.
pub fn svg(page_mm: [f64; 2]) -> String {
    format!("<g id=\"{SVG_ID}\"><text x=\"{}\" y=\"{}\" text-anchor=\"start\" font-family=\"DejaVu Sans\" font-size=\"{}\" fill=\"{COLOR}\">{CREDIT}</text></g>",
        page_mm[0] - RIGHT_INSET_PT * 25.4 / 72., page_mm[1] - BASELINE_PT * 25.4 / 72., FONT_PT * 25.4 / 72.)
}

/// PDF hosts draw the same mark as accessible native text. Remove only our
/// generated SVG group before conversion, so outlined and native credits
/// cannot accumulate during assembly.
pub fn without_svg_credit(svg: &str) -> String {
    let start = format!("<g id=\"{SVG_ID}\">");
    let Some(begin) = svg.find(&start) else {
        return svg.into();
    };
    let Some(end) = svg[begin..].find("</g>") else {
        return svg.into();
    };
    format!("{}{}", &svg[..begin], &svg[begin + end + 4..])
}
