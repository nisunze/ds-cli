//! Script-free rendering of owner-produced chart options, with exact data tables.
use serde_json::Value;
use std::collections::BTreeMap;

const COLORS: [&str; 6] = [
    "#38bdf8", "#4ade80", "#fb923c", "#c084fc", "#f87171", "#facc15",
];
pub(super) fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
fn text(v: &Value) -> String {
    match v {
        Value::Null => "—".into(),
        Value::String(s) => s.clone(),
        v => v.to_string(),
    }
}
fn color(v: &Value, i: usize) -> String {
    v.as_str()
        .filter(|s| {
            (s.len() == 4 || s.len() == 7)
                && s.starts_with('#')
                && s[1..].bytes().all(|c| c.is_ascii_hexdigit())
        })
        .unwrap_or(COLORS[i % COLORS.len()])
        .into()
}
fn number(v: &Value) -> Option<f64> {
    v.as_f64().filter(|n| n.is_finite())
}
fn datum(v: &Value) -> Option<f64> {
    number(if v.is_array() { &v[1] } else { v })
}
fn axis(s: &Value) -> usize {
    s["yAxisIndex"].as_u64().unwrap_or(0).min(1) as usize
}
fn scale(v: f64, range: (f64, f64)) -> f64 {
    300. - (v - range.0) / (range.1 - range.0) * 250.
}
fn expand(range: &mut (f64, f64), v: f64) {
    range.0 = range.0.min(v);
    range.1 = range.1.max(v);
}

pub(super) fn render(key: &str, chart: &Value) -> String {
    let Some(series) = chart["series"]
        .as_array()
        .filter(|s| !s.is_empty() && s.len() <= 512)
    else {
        return String::new();
    };
    if series
        .iter()
        .any(|s| s["data"].as_array().is_none_or(|d| d.len() > 10_000))
    {
        return String::new();
    }
    let title = chart["title"]["text"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| key.replace('_', " "));
    let mut svg = String::new();
    let mut table = String::new();
    let mut legend = String::new();
    if series[0]["type"] == "pie" {
        let rows = series[0]["data"].as_array().unwrap();
        let total: f64 = rows
            .iter()
            .filter_map(|r| number(&r["value"]).filter(|n| *n >= 0.))
            .sum();
        let mut angle: f64 = -std::f64::consts::FRAC_PI_2;
        table.push_str("<thead><tr><th>Component</th><th>Value</th></tr></thead><tbody>");
        for (i, row) in rows.iter().enumerate() {
            let label = escape(&text(&row["name"]));
            legend.push_str(&format!(
                "<li><span style='color:{}'>●</span> {label}: {}</li>",
                color(&row["itemStyle"]["color"], i),
                escape(&text(&row["value"]))
            ));
            table.push_str(&format!(
                "<tr><th scope=row>{label}</th><td>{}</td></tr>",
                escape(&text(&row["value"]))
            ));
            if let Some(value) =
                number(&row["value"]).filter(|n| *n > 0. && total > 0. && total.is_finite())
            {
                let c = color(&row["itemStyle"]["color"], i);
                let next = angle + value / total * std::f64::consts::TAU;
                if value == total {
                    svg.push_str(&format!("<circle cx=330 cy=170 r=110 fill='{c}'><title>{label}: {value}</title></circle>"));
                } else {
                    let (x, y) = (330. + 110. * angle.cos(), 170. + 110. * angle.sin());
                    let (nx, ny) = (330. + 110. * next.cos(), 170. + 110. * next.sin());
                    let large = u8::from(next - angle > std::f64::consts::PI);
                    svg.push_str(&format!("<path d='M330 170 L{x:.2} {y:.2} A110 110 0 {large} 1 {nx:.2} {ny:.2} Z' fill='{c}'><title>{label}: {value}</title></path>"));
                }
                angle = next;
            }
        }
        svg.push_str("<circle cx=330 cy=170 r=55 fill='#0f172a'/>");
    } else {
        let labels = chart["xAxis"]["data"].as_array();
        let count = labels.map_or_else(
            || {
                series
                    .iter()
                    .filter_map(|s| s["data"].as_array().map(Vec::len))
                    .max()
                    .unwrap_or(0)
            },
            Vec::len,
        );
        if count == 0 || count > 10_000 {
            return String::new();
        }
        let mut ranges = [(0., 0.), (0., 0.)];
        let mut x_range = (f64::INFINITY, f64::NEG_INFINITY);
        let mut sums: BTreeMap<(usize, String, usize, bool), f64> = BTreeMap::new();
        for s in series {
            let a = axis(s);
            for (i, d) in s["data"].as_array().unwrap().iter().enumerate() {
                if let Some(x) = d.as_array().and_then(|d| d.first()).and_then(number) {
                    expand(&mut x_range, x);
                }
                if let Some(v) = datum(d) {
                    expand(&mut ranges[a], v);
                    if s["type"] == "bar" {
                        if let Some(stack) = s["stack"].as_str() {
                            let sum = sums.entry((a, stack.into(), i, v >= 0.)).or_default();
                            *sum += v;
                            if sum.is_finite() {
                                expand(&mut ranges[a], *sum);
                            }
                        }
                    }
                }
            }
            if let Some(marks) = s["markLine"]["data"].as_array() {
                for mark in marks {
                    if let Some(v) = number(&mark["yAxis"]) {
                        expand(&mut ranges[a], v);
                    }
                }
            }
        }
        for r in &mut ranges {
            if r.0 == r.1 {
                r.1 = r.0 + 1.;
            }
        }
        let x = |i: usize, d: &Value| {
            if let Some(v) = d.as_array().and_then(|d| d.first()).and_then(number) {
                if x_range.1 > x_range.0 {
                    return 80. + (v - x_range.0) / (x_range.1 - x_range.0) * 510.;
                }
            }
            80. + (i as f64 + 0.5) / count as f64 * 510.
        };
        for (a, range) in ranges.iter().enumerate() {
            if a == 1 && !series.iter().any(|s| axis(s) == 1) {
                continue;
            }
            let left = if a == 0 { 75 } else { 595 };
            let name = if chart["yAxis"].is_array() {
                &chart["yAxis"][a]["name"]
            } else {
                &chart["yAxis"]["name"]
            };
            svg.push_str(&format!(
                "<text x='{left}' y=23 text-anchor='middle' fill='#cbd5e1' font-size=11>{}</text>",
                escape(name.as_str().unwrap_or(""))
            ));
            for tick in 0..=4 {
                let value = range.0 + (range.1 - range.0) * tick as f64 / 4.;
                let y = scale(value, *range);
                if a == 0 {
                    svg.push_str(&format!("<path d='M80 {y:.2} H590' stroke='#334155'/>"));
                }
                svg.push_str(&format!("<text x='{left}' y='{:.2}' text-anchor='{}' fill='#94a3b8' font-size=10>{value:.2}</text>", y + 4., if a == 0 { "end" } else { "start" }));
            }
        }
        sums.clear();
        let bar_group = |j: usize, s: &Value| {
            (
                axis(s),
                s["stack"]
                    .as_str()
                    .map(|v| format!("stack:{v}"))
                    .unwrap_or_else(|| format!("series:{j}")),
            )
        };
        let mut bar_groups = Vec::new();
        for (j, s) in series
            .iter()
            .enumerate()
            .filter(|(_, s)| s["type"] == "bar")
        {
            let group = bar_group(j, s);
            if !bar_groups.contains(&group) {
                bar_groups.push(group);
            }
        }
        for (j, s) in series.iter().enumerate() {
            let a = axis(s);
            let c = color(&s["itemStyle"]["color"], j);
            legend.push_str(&format!(
                "<li><span style='color:{c}'>●</span> {}</li>",
                escape(s["name"].as_str().unwrap_or(key))
            ));
            let mut path = String::new();
            let mut continuous = false;
            for (i, d) in s["data"].as_array().unwrap().iter().enumerate() {
                let Some(v) = datum(d) else {
                    continuous = false;
                    continue;
                };
                let px = x(i, d);
                let py = scale(v, ranges[a]);
                if s["type"] == "bar" {
                    let base = if let Some(stack) = s["stack"].as_str() {
                        let accumulated = sums.entry((a, stack.into(), i, v >= 0.)).or_default();
                        let base = *accumulated;
                        *accumulated += v;
                        base
                    } else {
                        0.
                    };
                    let y0 = scale(base, ranges[a]);
                    let y1 = scale(base + v, ranges[a]);
                    let groups = bar_groups.len().max(1);
                    let position = bar_groups
                        .iter()
                        .position(|g| *g == bar_group(j, s))
                        .unwrap_or(0);
                    let slot = (510. / count as f64 * 0.65 / groups as f64).min(30.);
                    let width = slot * if groups > 1 { 0.9 } else { 1. };
                    let center = px + (position as f64 - (groups - 1) as f64 / 2.) * slot;
                    svg.push_str(&format!("<rect x='{:.2}' y='{:.2}' width='{width:.2}' height='{:.2}' fill='{c}'><title>{}: {v}</title></rect>", center - width / 2., y0.min(y1), (y0-y1).abs(), escape(s["name"].as_str().unwrap_or(key))));
                } else {
                    path.push_str(&format!(
                        "{} {px:.2} {py:.2} ",
                        if continuous { "L" } else { "M" }
                    ));
                    continuous = true;
                    svg.push_str(&format!("<circle cx='{px:.2}' cy='{py:.2}' r=2 fill='{c}'><title>{}: {v}</title></circle>", escape(s["name"].as_str().unwrap_or(key))));
                }
            }
            svg.push_str(&format!(
                "<path d='{path}' fill=none stroke='{c}' stroke-width=2/>"
            ));
            if let Some(marks) = s["markLine"]["data"].as_array() {
                for mark in marks {
                    let c = color(&mark["lineStyle"]["color"], j);
                    if let Some(v) = number(&mark["yAxis"]) {
                        let y = scale(v, ranges[a]);
                        svg.push_str(&format!("<path d='M80 {y:.2} H590' stroke='{c}' stroke-dasharray='5 4'><title>{}: {v}</title></path>", escape(mark["label"]["formatter"].as_str().unwrap_or("Threshold"))));
                    }
                    let px = if let Some(labels) = labels {
                        if let Some(label) = mark["xAxis"].as_str() {
                            labels
                                .iter()
                                .position(|v| text(v) == label)
                                .map(|i| x(i, &Value::Null))
                        } else {
                            mark["xAxis"]
                                .as_u64()
                                .filter(|i| *i < count as u64)
                                .map(|i| x(i as usize, &Value::Null))
                        }
                    } else {
                        number(&mark["xAxis"])
                            .filter(|v| *v >= x_range.0 && *v <= x_range.1 && x_range.1 > x_range.0)
                            .map(|v| 80. + (v - x_range.0) / (x_range.1 - x_range.0) * 510.)
                    };
                    if let Some(px) = px {
                        let label =
                            escape(mark["label"]["formatter"].as_str().unwrap_or("Baseline"));
                        svg.push_str(&format!("<path d='M{px:.2} 50 V300' stroke='{c}' stroke-dasharray='5 4'><title>{label}</title></path>"));
                    }
                }
            }
        }
        if let Some(labels) = labels {
            for (i, label) in labels.iter().enumerate() {
                if i % count.div_ceil(10) == 0 {
                    svg.push_str(&format!("<text x='{:.2}' y=320 text-anchor='middle' fill='#94a3b8' font-size=10>{}</text>", x(i, &Value::Null), escape(&text(label))));
                }
            }
        }
        table.push_str("<thead><tr><th>Coordinate</th>");
        for s in series {
            table.push_str(&format!(
                "<th>{}</th>",
                escape(s["name"].as_str().unwrap_or(key))
            ));
        }
        table.push_str("</tr></thead><tbody>");
        for i in 0..count {
            let label = labels
                .and_then(|v| v.get(i))
                .map(text)
                .unwrap_or_else(|| i.to_string());
            table.push_str(&format!("<tr><th scope=row>{}</th>", escape(&label)));
            for s in series {
                table.push_str(&format!("<td>{}</td>", escape(&text(&s["data"][i]))));
            }
            table.push_str("</tr>");
        }
    }
    table.push_str("</tbody>");
    format!(
        "<figure><figcaption>{}</figcaption><svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 670 340' role=img aria-label='{}'><title>{}</title>{svg}</svg><ul class=legend>{legend}</ul><details><summary>Exact chart data</summary><table>{table}</table></details></figure>",
        escape(&title),
        escape(&title),
        escape(&title)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn script_free_charts_keep_null_gaps_independent_axes_and_escaped_labels() {
        let figure = render(
            "cashflow",
            &json!({"xAxis":{"data":["1","2","3"]},
            "yAxis":[{"name":"Cash"},{"name":"Cumulative"}],"series":[
                {"name":"<script>","type":"line","data":[0,null,2]},
                {"name":"Cumulative","type":"line","yAxisIndex":1,"data":[0,100,200]}
            ]}),
        );
        assert!(figure.contains("M 165.00 300.00 M 505.00 50.00"));
        assert!(figure.contains("Cumulative"));
        assert!(figure.contains("&lt;script&gt;"));
        assert!(figure.contains("<td>—</td>"));
        assert!(!figure.contains("<script>"));
    }
    #[test]
    fn grouped_bars_stay_separate_while_stacks_and_vertical_markers_align() {
        let figure = render(
            "cash",
            &json!({"xAxis":{"data":["1"]},"series":[
                {"name":"A","type":"bar","stack":"a","data":[2]},
                {"name":"B","type":"bar","data":[3],"markLine":{"data":[{"xAxis":"1","label":{"formatter":"Payback"}}]}},
                {"name":"A2","type":"bar","stack":"a","data":[1]}
            ]}),
        );
        let positions: Vec<f64> = figure
            .split("<rect x='")
            .skip(1)
            .map(|s| s.split('\'').next().unwrap().parse().unwrap())
            .collect();
        assert_eq!(positions.len(), 3);
        assert_eq!(positions[0], positions[2]);
        assert!(positions[0] + 27. <= positions[1]);
        assert!(figure.contains("V300") && figure.contains("<title>Payback</title>"));
    }
    #[test]
    fn tiny_pie_values_remain_in_exact_tables_and_svg() {
        let figure = render(
            "costs",
            &json!({"series":[{"type":"pie","data":[
            {"name":"A","value":0.1},{"name":"B","value":0.2}]}]}),
        );
        assert!(figure.contains("<td>0.1</td>"));
        assert!(figure.contains("<path d='M330 170"));
    }
}
