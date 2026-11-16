//! The regions whose OpenStreetMap extract can be downloaded (from Geofabrik's index), where
//! they are, and finding one by name.

use crate::lang::tr;

/// Where Geofabrik keeps its extracts: a region's `path` is from there.
pub const GEOFABRIK_ROOT: &str = "https://download.geofabrik.de/";

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Region {
    /// Its name on disk and in the settings (Geofabrik's id, `/` made `-`).
    pub id: String,
    /// Geofabrik's name ("Provence Alpes-Cote-d'Azur"), and the one of the region it is part of.
    pub name: String,
    #[serde(default)]
    pub parent: String,
    /// Where Geofabrik keeps its extract, from the server's root.
    pub path: String,
    /// South, west, north, east, in degrees.
    pub bbox: [f64; 4],
}

impl Region {
    /// Its name as shown: Geofabrik's, but for the region one starts with.
    pub fn name(&self) -> &str {
        match self.id.as_str() {
            "provence-alpes-cote-d-azur" => tr("Provence-Alpes-Côte d'Azur", "Provence-Alpes-Côte d'Azur"),
            _ => &self.name,
        }
    }

    /// Whether a position lies in the region's box.
    pub fn holds(&self, lat: f64, lon: f64) -> bool {
        let [s, w, n, e] = self.bbox;
        (s..=n).contains(&lat) && (w..=e).contains(&lon)
    }

    /// The box's area, in square degrees: the smaller of two regions holding a point is the
    /// one to use.
    pub fn area(&self) -> f64 {
        let [s, w, n, e] = self.bbox;
        (n - s) * (e - w)
    }
}

/// The region the app starts with, before any is chosen.
pub fn first() -> Region {
    Region {
        id: "provence-alpes-cote-d-azur".into(),
        name: "Provence Alpes-Cote-d'Azur".into(),
        parent: "France".into(),
        path: "europe/france/provence-alpes-cote-d-azur-latest.osm.pbf".into(),
        bbox: [42.98, 4.23, 45.13, 7.72],
    }
}

/// The regions of Geofabrik's index (`index-v1.json`, GeoJSON): those with an extract, each
/// with the box of its outline.
pub fn read_index(json: &str) -> Result<Vec<Region>, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let features = v["features"].as_array().ok_or("no features")?;
    let names: std::collections::HashMap<&str, &str> =
        features.iter().filter_map(|f| Some((f["properties"]["id"].as_str()?, f["properties"]["name"].as_str()?))).collect();
    Ok(features
        .iter()
        .filter_map(|f| {
            let p = &f["properties"];
            let id = p["id"].as_str()?;
            let path = p["urls"]["pbf"].as_str()?.strip_prefix(GEOFABRIK_ROOT)?;
            let mut bbox = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
            let mut stack = vec![&f["geometry"]["coordinates"]];
            while let Some(c) = stack.pop() {
                let a = c.as_array()?;
                match (a.first().and_then(|x| x.as_f64()), a.get(1).and_then(|x| x.as_f64())) {
                    (Some(lon), Some(lat)) => bbox = [bbox[0].min(lat), bbox[1].min(lon), bbox[2].max(lat), bbox[3].max(lon)],
                    _ => stack.extend(a.iter()),
                }
            }
            if bbox[0] > bbox[2] {
                return None;
            }
            let parent = p["parent"].as_str().and_then(|q| names.get(q)).unwrap_or(&"");
            Some(Region { id: id.replace('/', "-"), name: p["name"].as_str()?.to_string(), parent: parent.to_string(), path: path.to_string(), bbox })
        })
        .collect())
}

/// Lower case, without accents, hyphens and apostrophes as spaces: "Côte-d'Azur" → "cote d azur".
fn plain(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'à' | 'â' | 'ä' | 'á' | 'ã' | 'å' | 'À' | 'Â' | 'Ä' | 'Á' => 'a',
            'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => 'e',
            'î' | 'ï' | 'í' | 'ì' | 'Î' | 'Ï' => 'i',
            'ô' | 'ö' | 'ó' | 'ò' | 'õ' | 'ø' | 'Ô' | 'Ö' => 'o',
            'û' | 'ü' | 'ú' | 'ù' | 'Û' | 'Ü' => 'u',
            'ç' | 'Ç' => 'c',
            'ñ' | 'Ñ' => 'n',
            '-' | '\'' | '’' | '_' | ',' | '.' => ' ',
            c => c.to_ascii_lowercase(),
        })
        .collect()
}

/// The regions whose name (or the name of what they are part of) holds every word of `q`, at
/// most `n`: those whose name starts with it first, then the smaller ones.
pub fn search<'a>(all: &'a [Region], q: &str, n: usize) -> Vec<&'a Region> {
    let q = plain(q);
    let words: Vec<&str> = q.split_whitespace().collect();
    if words.is_empty() {
        return Vec::new();
    }
    let mut found: Vec<(bool, f64, &Region)> = all
        .iter()
        .filter_map(|r| {
            let name = plain(&r.name);
            let both = format!("{name} {}", plain(&r.parent));
            words.iter().all(|w| both.split_whitespace().any(|x| x.starts_with(w))).then(|| (!name.starts_with(words[0]), r.area(), r))
        })
        .collect();
    found.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    found.into_iter().take(n).map(|f| f.2).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const INDEX: &str = r#"{"type": "FeatureCollection", "features": [
        {"type": "Feature", "properties": {"id": "europe", "name": "Europe", "urls": {"pbf": "https://download.geofabrik.de/europe-latest.osm.pbf"}},
         "geometry": {"type": "MultiPolygon", "coordinates": [[[[-30.0, 30.0], [40.0, 30.0], [40.0, 80.0], [-30.0, 30.0]]]]}},
        {"type": "Feature", "properties": {"id": "france", "parent": "europe", "name": "France", "urls": {"pbf": "https://download.geofabrik.de/europe/france-latest.osm.pbf"}},
         "geometry": {"type": "MultiPolygon", "coordinates": [[[[-5.0, 42.0], [8.0, 42.0], [8.0, 51.0], [-5.0, 42.0]]]]}},
        {"type": "Feature", "properties": {"id": "provence-alpes-cote-d-azur", "parent": "france", "name": "Provence Alpes-Cote-d'Azur", "urls": {"pbf": "https://download.geofabrik.de/europe/france/provence-alpes-cote-d-azur-latest.osm.pbf"}},
         "geometry": {"type": "Polygon", "coordinates": [[[4.2, 42.9], [7.7, 43.5], [6.0, 45.1], [4.2, 42.9]]]}},
        {"type": "Feature", "properties": {"id": "us/georgia", "parent": "north-america", "name": "Georgia", "urls": {"pbf": "https://download.geofabrik.de/north-america/us/georgia-latest.osm.pbf"}},
         "geometry": {"type": "MultiPolygon", "coordinates": [[[[-85.6, 30.3], [-80.8, 30.3], [-80.8, 35.0], [-85.6, 30.3]]]]}},
        {"type": "Feature", "properties": {"id": "nothing", "name": "No extract", "urls": {}}, "geometry": {"type": "Polygon", "coordinates": [[[0, 0], [1, 1], [0, 0]]]}},
        {"type": "Feature", "properties": {"id": "elsewhere", "name": "Elsewhere", "urls": {"pbf": "https://other.example/x.osm.pbf"}}, "geometry": {"type": "Polygon", "coordinates": [[[0, 0], [1, 1], [0, 0]]]}},
        {"type": "Feature", "properties": {"id": "shapeless", "name": "Shapeless", "urls": {"pbf": "https://download.geofabrik.de/s.osm.pbf"}}, "geometry": null}
    ]}"#;

    #[test]
    fn reads_the_index() {
        let all = read_index(INDEX).unwrap();
        let ids: Vec<&str> = all.iter().map(|r| r.id.as_str()).collect();
        // No extract, another server, no outline: left out; `/` can't be in a file name.
        assert_eq!(ids, ["europe", "france", "provence-alpes-cote-d-azur", "us-georgia"]);
        let paca = &all[2];
        assert_eq!((paca.parent.as_str(), paca.path.as_str()), ("France", "europe/france/provence-alpes-cote-d-azur-latest.osm.pbf"));
        assert_eq!(paca.bbox, [42.9, 4.2, 45.1, 7.7]);
        assert!(paca.holds(43.70, 7.26) && !paca.holds(48.85, 2.35));
        assert_eq!(all[3].parent, "");
        assert!(read_index("{}").is_err() && read_index("<html>").is_err());
    }

    #[test]
    fn finds_regions_by_name() {
        let all = read_index(INDEX).unwrap();
        let ids = |q: &str| search(&all, q, 8).iter().map(|r| r.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids("provence"), ["provence-alpes-cote-d-azur"]);
        // Accents, case, hyphens and word order don't matter; words start words.
        assert_eq!(ids("Côte d'Azur"), ["provence-alpes-cote-d-azur"]);
        assert_eq!(ids("AZUR provence"), ["provence-alpes-cote-d-azur"]);
        assert_eq!(ids("zur"), Vec::<String>::new());
        // The name first, then what it is part of: France itself before its regions.
        assert_eq!(ids("france"), ["france", "provence-alpes-cote-d-azur"]);
        // Europe by its name, France by what it is part of.
        assert_eq!(ids("e"), ["europe", "france"]);
        assert!(ids("").is_empty() && ids("   ").is_empty() && ids("xyz").is_empty());
        assert_eq!(search(&all, "e", 1).len(), 1);
        assert_eq!(first().name(), "Provence-Alpes-Côte d'Azur");
    }
}
