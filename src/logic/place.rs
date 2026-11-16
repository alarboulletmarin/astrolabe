//! A place typed in: coordinates read as they are, or the answer of OpenStreetMap's place search.

/// A place found: where, and what it is called.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub at: [f64; 2],
    pub name: String,
}

/// Coordinates typed in decimal degrees, latitude first: "43.6950, 7.2575", "43.6950 7.2575",
/// or with decimal commas "43,6950 7,2575" or "43,6950; 7,2575". `None` for anything else, or
/// out of range.
// ponytail: no degrees-minutes-seconds (43°41'42"N); add if pasted coordinates come that way.
pub fn coords(s: &str) -> Option<[f64; 2]> {
    let s = s.trim();
    // With decimal points, a comma parts the two; with decimal commas, a space or a semicolon does.
    let parts: Vec<&str> = if s.contains(';') {
        s.split(';').map(str::trim).collect()
    } else if s.contains('.') {
        s.split([',', ' ', '\t']).filter(|p| !p.is_empty()).collect()
    } else {
        s.split_whitespace().collect()
    };
    let [lat, lon] = parts[..] else { return None };
    let num = |p: &str| p.replace(',', ".").parse::<f64>().ok().filter(|v| v.is_finite());
    let (lat, lon) = (num(lat)?, num(lon)?);
    ((-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon)).then_some([lat, lon])
}

/// The places of a search answer (Nominatim's `jsonv2` with `addressdetails`), in its order;
/// ones without a readable position are left out. Each is named short: "37 Promenade des
/// Anglais, Nice", "Mont Vinaigrier, Nice", not down to the country; a name already given (the
/// pieces of one street) comes once.
pub fn read_search(json: &str) -> Result<Vec<Place>, String> {
    #[derive(serde::Deserialize)]
    struct Hit {
        lat: String,
        lon: String,
        #[serde(default)]
        name: String,
        display_name: String,
        #[serde(default)]
        address: std::collections::HashMap<String, String>,
    }
    let hits: Vec<Hit> = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let mut seen = std::collections::HashSet::new();
    Ok(hits
        .into_iter()
        .filter_map(|h| {
            let at = [h.lat.parse().ok()?, h.lon.parse().ok()?];
            let a = |k: &str| h.address.get(k).map(|v| v.trim()).filter(|v| !v.is_empty());
            let street = a("road").or(a("pedestrian")).or(a("footway")).map(|r| match a("house_number") {
                Some(n) => format!("{n} {r}"),
                None => r.to_string(),
            });
            let what = Some(h.name.trim()).filter(|n| !n.is_empty()).map(str::to_string).or(street);
            let town = a("city").or(a("town")).or(a("village")).or(a("municipality"));
            let name = match (what, town) {
                (Some(w), Some(t)) if w != t => format!("{w}, {t}"),
                (Some(w), _) => w,
                (None, Some(t)) => t.to_string(),
                (None, None) => h.display_name,
            };
            Some(Place { at, name })
        })
        .filter(|p| seen.insert(p.name.clone()))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_coordinates_as_typed() {
        for s in ["43.6950, 7.2575", "43.6950 7.2575", "  43.6950,7.2575  ", "43,6950 7,2575", "43,6950; 7,2575", "43.6950;7.2575"] {
            assert_eq!(coords(s), Some([43.695, 7.2575]), "{s}");
        }
        assert_eq!(coords("-33.9, 151.2"), Some([-33.9, 151.2]));
        assert_eq!(coords("43 7"), Some([43.0, 7.0]));
        // An address, one number, three, out of range, or not numbers.
        for s in ["", "37 promenade des Anglais", "43.69", "43.6 7.2 3", "91, 7", "43, 181", "NaN, 7", "inf 7", "nord, est", "43,6950,7,2575"] {
            assert_eq!(coords(s), None, "{s}");
        }
    }

    #[test]
    fn reads_a_search_answer() {
        let json = r#"[
            {"place_id": 1, "lat": "43.6944974", "lon": "7.2584024", "name": "", "type": "house",
             "display_name": "37, Promenade des Anglais, Carré d'Or, La Buffa, Nice, Alpes-Maritimes, Provence-Alpes-Côte d'Azur, France métropolitaine, 06000, France",
             "address": {"house_number": "37", "road": "Promenade des Anglais", "quarter": "Carré d'Or", "city": "Nice", "postcode": "06000", "country": "France"}},
            {"lat": "x", "lon": "7.2", "display_name": "Bad"},
            {"lat": "43.71", "lon": "7.31", "name": "Mont Vinaigrier", "display_name": "Mont Vinaigrier, Nice, …", "address": {"peak": "Mont Vinaigrier", "city": "Nice"}},
            {"lat": "43.7", "lon": "7.26", "name": "Nice", "display_name": "Nice, Alpes-Maritimes, …", "address": {"city": "Nice"}},
            {"lat": "44.1", "lon": "6.9", "name": "", "display_name": "Somewhere « odd », 04000", "address": {"village": "Saint-Paul"}},
            {"lat": "44.2", "lon": "6.8", "display_name": "Only the long name"},
            {"lat": "43.6951", "lon": "7.2576", "name": "", "display_name": "37, Promenade des Anglais, …", "address": {"house_number": "37", "road": "Promenade des Anglais", "city": "Nice"}}
        ]"#;
        let got = read_search(json).unwrap();
        let names: Vec<&str> = got.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["37 Promenade des Anglais, Nice", "Mont Vinaigrier, Nice", "Nice", "Saint-Paul", "Only the long name"]);
        assert_eq!(got[0].at, [43.6944974, 7.2584024]);
        assert_eq!(read_search("[]").unwrap(), vec![]);
        assert!(read_search("<html>").is_err());
    }
}
