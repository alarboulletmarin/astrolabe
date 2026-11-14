//! The regions whose OpenStreetMap extract can be downloaded (from Geofabrik), and where they are.

use crate::lang::tr;

pub struct Region {
    /// Its name on disk and in the settings.
    pub id: &'static str,
    /// Where Geofabrik keeps its extract, from the server's root.
    pub path: &'static str,
    /// South, west, north, east, in degrees.
    pub bbox: [f64; 4],
}

impl Region {
    pub fn name(&self) -> &'static str {
        match self.id {
            "provence-alpes-cote-d-azur" => tr("Provence-Alpes-Côte d'Azur", "Provence-Alpes-Côte d'Azur"),
            _ => self.id,
        }
    }

    /// Whether a position lies in the region's box.
    pub fn holds(&self, lat: f64, lon: f64) -> bool {
        let [s, w, n, e] = self.bbox;
        (s..=n).contains(&lat) && (w..=e).contains(&lon)
    }

    pub fn centre(&self) -> (f64, f64) {
        let [s, w, n, e] = self.bbox;
        ((s + n) / 2.0, (w + e) / 2.0)
    }
}

// ponytail: one region to start with; Geofabrik's index (index-v1.json) would give them all.
pub const REGIONS: &[Region] = &[Region { id: "provence-alpes-cote-d-azur", path: "europe/france/provence-alpes-cote-d-azur-latest.osm.pbf", bbox: [42.98, 4.23, 45.13, 7.72] }];

/// The region of an id, the first one when it is unknown.
pub fn find(id: &str) -> &'static Region {
    REGIONS.iter().find(|r| r.id == id).unwrap_or(&REGIONS[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nice_is_in_provence() {
        let r = find("provence-alpes-cote-d-azur");
        assert!(r.holds(43.70, 7.26));
        assert!(!r.holds(48.85, 2.35));
        assert_eq!(find("nowhere").id, r.id);
    }
}
