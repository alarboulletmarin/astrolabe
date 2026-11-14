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

#[cfg(test)]
mod tests {
    use super::*;

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
