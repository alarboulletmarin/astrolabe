//! GPX 1.1: a loop as a track a watch or another app can follow.

use std::fmt::Write;

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// A track named `name`: its points (lat, lon) and their altitudes. The map data's licence goes
/// in the metadata, as the ODbL asks of a work made from it.
pub fn write(name: &str, line: &[[f64; 2]], eles: &[Option<f32>]) -> String {
    let mut s = String::with_capacity(64 * line.len() + 512);
    s.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    s.push_str("<gpx version=\"1.1\" creator=\"Astrolabe\" xmlns=\"http://www.topografix.com/GPX/1/1\">\n");
    let _ = writeln!(s, "  <metadata>\n    <name>{}</name>", escape(name));
    s.push_str("    <copyright author=\"OpenStreetMap contributors\">\n      <license>https://opendatacommons.org/licenses/odbl/</license>\n    </copyright>\n");
    s.push_str("    <link href=\"https://www.openstreetmap.org/copyright\"><text>Map data © OpenStreetMap contributors</text></link>\n  </metadata>\n");
    let _ = writeln!(s, "  <trk>\n    <name>{}</name>\n    <type>running</type>\n    <trkseg>", escape(name));
    for (i, p) in line.iter().enumerate() {
        match eles.get(i).copied().flatten() {
            Some(e) => {
                let _ = writeln!(s, "      <trkpt lat=\"{:.7}\" lon=\"{:.7}\"><ele>{e:.1}</ele></trkpt>", p[0], p[1]);
            }
            None => {
                let _ = writeln!(s, "      <trkpt lat=\"{:.7}\" lon=\"{:.7}\"/>", p[0], p[1]);
            }
        }
    }
    s.push_str("    </trkseg>\n  </trk>\n</gpx>\n");
    s
}

/// What a GPX file holds that a route needs.
#[derive(Debug, PartialEq)]
pub struct Track {
    /// The track's name (else the route's, else the file's).
    pub name: Option<String>,
    /// Its points (lat, lon), segments one after the other, and their altitudes.
    pub line: Vec<[f64; 2]>,
    pub eles: Vec<Option<f32>>,
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// The value of attribute `name` in a tag's text (`lat="43.7"` or `lat='43.7'`).
fn attr(tag: &str, name: &str) -> Option<f64> {
    let mut rest = tag;
    while let Some(i) = rest.find(name) {
        let before = rest[..i].chars().last();
        let after = rest[i + name.len()..].trim_start();
        rest = &rest[i + name.len()..];
        if !before.is_some_and(char::is_whitespace) {
            continue;
        }
        let Some(after) = after.strip_prefix('=') else { continue };
        let after = after.trim_start();
        let q = after.chars().next()?;
        let body = after.get(1..)?;
        return body[..body.find(q)?].trim().parse().ok();
    }
    None
}

/// The text between `<tag>` and `</tag>` in `s`, the first time.
fn inner<'a>(s: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let i = s.find(&open)? + open.len();
    Some(&s[i..i + s[i..].find(&format!("</{tag}>"))?])
}

/// Reads a GPX file: the points of its tracks (`trkpt`), else of its routes (`rtept`). `None`
/// when it holds fewer than two good points. Points out of range are left out.
pub fn read(text: &str) -> Option<Track> {
    let kind = if text.contains("<trkpt") { "trkpt" } else { "rtept" };
    let open = format!("<{kind}");
    let close = format!("</{kind}>");
    let (mut line, mut eles) = (Vec::new(), Vec::new());
    let mut rest = text;
    while let Some(i) = rest.find(&open) {
        rest = &rest[i + open.len()..];
        // `<trkpt` and not `<trkptx`.
        if !rest.starts_with(|c: char| c.is_whitespace() || c == '>' || c == '/') {
            continue;
        }
        let Some(end) = rest.find('>') else { break };
        let tag = &rest[..end];
        let body = if tag.ends_with('/') { "" } else { rest[end + 1..].find(&close).map_or("", |j| &rest[end + 1..end + 1 + j]) };
        let (Some(lat), Some(lon)) = (attr(tag, "lat"), attr(tag, "lon")) else { continue };
        if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
            continue;
        }
        line.push([lat, lon]);
        eles.push(inner(body, "ele").and_then(|e| e.trim().parse::<f32>().ok()).filter(|e| e.is_finite()));
    }
    if line.len() < 2 {
        return None;
    }
    let owner = text.find(if kind == "trkpt" { "<trk>" } else { "<rte>" }).map_or(text, |i| &text[i..]);
    let name = inner(owner, "name").or_else(|| inner(text, "name")).map(|n| unescape(n.trim())).filter(|n| !n.is_empty());
    Some(Track { name, line, eles })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_back_what_it_writes() {
        let line = [[43.7, 7.25], [43.71, 7.26], [43.72, 7.27]];
        let eles = [Some(12.3), None, Some(-4.5)];
        let t = read(&write("Tour <du> parc & co", &line, &eles)).unwrap();
        assert_eq!(t.name.as_deref(), Some("Tour <du> parc & co"));
        assert_eq!(t.line, line);
        assert_eq!(t.eles, eles);
    }

    #[test]
    fn reads_tracks_of_other_apps() {
        // Single quotes, lon before lat, several segments, a time, spaces, an unnamed track.
        let g = "<?xml version='1.0'?><gpx><metadata><name>Morning run</name></metadata><trk><trkseg>\n\
                 <trkpt lon='7.25' lat='43.7'><ele> 10 </ele><time>2026-01-01T00:00:00Z</time></trkpt>\n\
                 </trkseg><trkseg><trkpt  lat=\"43.71\"  lon=\"7.26\"></trkpt>\n\
                 <trkpt lat=\"95\" lon=\"7.26\"/><trkpt lat=\"abc\" lon=\"7\"/><trkpt lat=\"43.72\" lon=\"7.27\"><ele>nan</ele></trkpt></trkseg></trk></gpx>";
        let t = read(g).unwrap();
        assert_eq!(t.name.as_deref(), Some("Morning run"), "no track name: the file's");
        assert_eq!(t.line, [[43.7, 7.25], [43.71, 7.26], [43.72, 7.27]], "bad points left out");
        assert_eq!(t.eles, [Some(10.0), None, None]);
    }

    #[test]
    fn reads_a_route_when_there_is_no_track() {
        let g = r#"<gpx><rte><name>Plan</name><rtept lat="43.7" lon="7.25"/><rtept lat="43.8" lon="7.3"/></rte></gpx>"#;
        let t = read(g).unwrap();
        assert_eq!((t.name.as_deref(), t.line.len()), (Some("Plan"), 2));
    }

    #[test]
    fn too_little_is_nothing() {
        assert_eq!(read(""), None);
        assert_eq!(read("not a gpx at all"), None);
        assert_eq!(read(r#"<gpx><trk><trkseg><trkpt lat="43.7" lon="7.25"/></trkseg></trk></gpx>"#), None, "one point");
        assert_eq!(read(r#"<gpx><wpt lat="43.7" lon="7.25"/><wpt lat="43.8" lon="7.3"/></gpx>"#), None, "waypoints only");
    }

    #[test]
    fn writes_a_track_with_altitudes_and_the_licence() {
        let g = write("Loop <12 km> & co", &[[43.7, 7.25], [43.71, 7.26]], &[Some(12.34), None]);
        assert!(g.starts_with("<?xml"));
        assert!(g.contains("<name>Loop &lt;12 km&gt; &amp; co</name>"));
        assert!(g.contains("<trkpt lat=\"43.7000000\" lon=\"7.2500000\"><ele>12.3</ele></trkpt>"));
        assert!(g.contains("<trkpt lat=\"43.7100000\" lon=\"7.2600000\"/>"));
        assert!(g.contains("odbl"));
        assert_eq!(g.matches("<trkpt").count(), 2);
    }
}
