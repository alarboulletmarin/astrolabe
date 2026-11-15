//! OpenStreetMap extracts (`.osm.pbf`): the few protobuf messages a walking graph needs, read by
//! hand (fileformat.proto and osmformat.proto), and which ways one can run on. Pure: the caller
//! hands over the bytes of each blob.
//!
//! A file is a run of frames: a big-endian `u32` length, a `BlobHeader` of that length, then the
//! `Blob` it announces. The first blob is an `OSMHeader`, the others `OSMData`.

use std::io::Read;

/// The largest blob the format allows, compressed or not (the spec says 32 MiB).
const MAX_BLOB: usize = 32 * 1024 * 1024;
/// A `BlobHeader` is a few bytes; the spec caps it at 64 KiB.
pub const MAX_HEADER: usize = 64 * 1024;

/// A protobuf reader over a slice: varints, keys, length-delimited fields. Every read is
/// checked, so a damaged file gives an error, never a panic.
struct Pb<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Pb<'a> {
    fn new(b: &'a [u8]) -> Self {
        Pb { b, i: 0 }
    }

    fn more(&self) -> bool {
        self.i < self.b.len()
    }

    fn varint(&mut self) -> Result<u64, String> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = *self.b.get(self.i).ok_or("PBF: truncated varint")?;
            self.i += 1;
            v |= ((byte & 0x7f) as u64) << shift;
            if byte & 0x80 == 0 {
                return Ok(v);
            }
        }
        Err("PBF: varint too long".into())
    }

    /// The next field's number and wire type.
    fn key(&mut self) -> Result<(u64, u8), String> {
        let k = self.varint()?;
        Ok((k >> 3, (k & 7) as u8))
    }

    fn bytes(&mut self) -> Result<&'a [u8], String> {
        let n = usize::try_from(self.varint()?).map_err(|_| "PBF: length too large")?;
        let end = self.i.checked_add(n).filter(|e| *e <= self.b.len()).ok_or("PBF: truncated field")?;
        let s = &self.b[self.i..end];
        self.i = end;
        Ok(s)
    }

    fn skip(&mut self, wire: u8) -> Result<(), String> {
        match wire {
            0 => self.varint().map(drop),
            1 => self.advance(8),
            2 => self.bytes().map(drop),
            5 => self.advance(4),
            w => Err(format!("PBF: unknown wire type {w}")),
        }
    }

    fn advance(&mut self, n: usize) -> Result<(), String> {
        if self.i + n > self.b.len() {
            return Err("PBF: truncated field".into());
        }
        self.i += n;
        Ok(())
    }

    /// A repeated integer field, packed (wire 2) or not (wire 0): each value to `f`.
    fn repeated(&mut self, wire: u8, mut f: impl FnMut(u64)) -> Result<(), String> {
        match wire {
            0 => f(self.varint()?),
            2 => {
                let mut p = Pb::new(self.bytes()?);
                while p.more() {
                    f(p.varint()?);
                }
            }
            w => return Err(format!("PBF: wire type {w} for a repeated integer")),
        }
        Ok(())
    }
}

fn zigzag(v: u64) -> i64 {
    ((v >> 1) as i64) ^ -((v & 1) as i64)
}

/// What a `BlobHeader` announces: the blob's type and size.
pub fn blob_header(b: &[u8]) -> Result<(String, usize), String> {
    let (mut kind, mut size) = (None, None);
    let mut p = Pb::new(b);
    while p.more() {
        match p.key()? {
            (1, 2) => kind = Some(String::from_utf8_lossy(p.bytes()?).into_owned()),
            (3, 0) => size = Some(p.varint()? as usize),
            (_, w) => p.skip(w)?,
        }
    }
    let size = size.ok_or("PBF: blob without a size")?;
    if size > MAX_BLOB {
        return Err("PBF: blob too large".into());
    }
    Ok((kind.ok_or("PBF: blob without a type")?, size))
}

/// The bytes inside a `Blob`, inflated.
pub fn blob_data(b: &[u8]) -> Result<Vec<u8>, String> {
    let (mut raw, mut zlib, mut size) = (None, None, 0usize);
    let mut p = Pb::new(b);
    while p.more() {
        match p.key()? {
            (1, 2) => raw = Some(p.bytes()?),
            (2, 0) => size = p.varint()? as usize,
            (3, 2) => zlib = Some(p.bytes()?),
            (4 | 5 | 6 | 7, _) => return Err("PBF: compression not supported (only zlib)".into()),
            (_, w) => p.skip(w)?,
        }
    }
    if let Some(raw) = raw {
        return Ok(raw.to_vec());
    }
    let zlib = zlib.ok_or("PBF: empty blob")?;
    if size > MAX_BLOB {
        return Err("PBF: blob too large".into());
    }
    let mut out = Vec::with_capacity(size);
    // One byte more than announced: a blob that inflates beyond its size is caught, not trusted.
    flate2::read::ZlibDecoder::new(zlib).take(size as u64 + 1).read_to_end(&mut out).map_err(|e| format!("PBF: {e}"))?;
    if out.len() != size {
        return Err("PBF: blob size does not match".into());
    }
    Ok(out)
}

/// The file's header: what a reader must understand, and when the extract was cut.
#[derive(Debug, Default, PartialEq)]
pub struct Header {
    /// Seconds since 1970 (`osmosis_replication_timestamp`); 0 when the file doesn't say.
    pub timestamp: i64,
}

pub fn header(b: &[u8]) -> Result<Header, String> {
    let mut h = Header::default();
    let mut p = Pb::new(b);
    while p.more() {
        match p.key()? {
            (4, 2) => {
                let feature = String::from_utf8_lossy(p.bytes()?).into_owned();
                if !matches!(feature.as_str(), "OsmSchema-V0.6" | "DenseNodes") {
                    return Err(format!("PBF: required feature not supported: {feature}"));
                }
            }
            (32, 0) => h.timestamp = p.varint()? as i64,
            (_, w) => p.skip(w)?,
        }
    }
    Ok(h)
}

/// A way one can run on: its id, what kind it is (`Class`), and its nodes.
#[derive(Clone, Debug, PartialEq)]
pub struct Way {
    pub id: i64,
    pub class: Class,
    pub refs: Vec<i64>,
}

/// What a data block holds that the graph needs: positions (id, lat, lon in 1e-7 degrees) and
/// the ways one can run on. Only what was asked for is read.
#[derive(Debug, Default)]
pub struct Block {
    pub nodes: Vec<(i64, i32, i32)>,
    pub ways: Vec<Way>,
    /// The node refs of closed ways tagged as an industrial area (see `is_industrial_area`).
    // ponytail: multipolygon relations are not read, so an area drawn as one is missed.
    pub areas: Vec<Vec<i64>>,
    /// The node refs of closed ways tagged as a building, with its height in metres (see
    /// `building_height`); read only when asked for (`block_of`).
    // ponytail: multipolygon relations are not read, so a building with a courtyard drawn as
    // one is missed; `building:part` is not read either: one height per footprint.
    pub buildings: Vec<(Vec<i64>, f32)>,
}

/// Reads an `OSMData` block (inflated). Positions only if `nodes`, ways only if `ways`.
pub fn block(b: &[u8], nodes: bool, ways: bool) -> Result<Block, String> {
    block_of(b, nodes, ways, false)
}

/// As `block`, and the buildings if `buildings`.
pub fn block_of(b: &[u8], nodes: bool, ways: bool, buildings: bool) -> Result<Block, String> {
    let mut strings: Vec<&[u8]> = Vec::new();
    let mut groups = Vec::new();
    let (mut gran, mut lat_off, mut lon_off) = (100i64, 0i64, 0i64);
    let mut p = Pb::new(b);
    while p.more() {
        match p.key()? {
            (1, 2) => {
                let mut s = Pb::new(p.bytes()?);
                while s.more() {
                    match s.key()? {
                        (1, 2) => strings.push(s.bytes()?),
                        (_, w) => s.skip(w)?,
                    }
                }
            }
            (2, 2) => groups.push(p.bytes()?),
            (17, 0) => gran = p.varint()? as i64,
            (19, 0) => lat_off = p.varint()? as i64,
            (20, 0) => lon_off = p.varint()? as i64,
            (_, w) => p.skip(w)?,
        }
    }
    // Nanodegrees to 1e-7 degrees.
    let coord = |off: i64, v: i64| ((off + gran * v) / 100) as i32;
    let mut out = Block::default();
    for g in groups {
        let mut p = Pb::new(g);
        while p.more() {
            match p.key()? {
                (1, 2) if nodes => {
                    let (mut id, mut lat, mut lon) = (0, 0, 0);
                    let mut n = Pb::new(p.bytes()?);
                    while n.more() {
                        match n.key()? {
                            (1, 0) => id = zigzag(n.varint()?),
                            (8, 0) => lat = zigzag(n.varint()?),
                            (9, 0) => lon = zigzag(n.varint()?),
                            (_, w) => n.skip(w)?,
                        }
                    }
                    out.nodes.push((id, coord(lat_off, lat), coord(lon_off, lon)));
                }
                (2, 2) if nodes => {
                    let (mut ids, mut lats, mut lons) = (Vec::new(), Vec::new(), Vec::new());
                    let mut d = Pb::new(p.bytes()?);
                    while d.more() {
                        match d.key()? {
                            (1, w) => d.repeated(w, |v| ids.push(zigzag(v)))?,
                            (8, w) => d.repeated(w, |v| lats.push(zigzag(v)))?,
                            (9, w) => d.repeated(w, |v| lons.push(zigzag(v)))?,
                            (_, w) => d.skip(w)?,
                        }
                    }
                    if ids.len() != lats.len() || ids.len() != lons.len() {
                        return Err("PBF: dense nodes of different lengths".into());
                    }
                    let (mut id, mut lat, mut lon) = (0i64, 0i64, 0i64);
                    out.nodes.reserve(ids.len());
                    for k in 0..ids.len() {
                        id = id.wrapping_add(ids[k]);
                        lat = lat.wrapping_add(lats[k]);
                        lon = lon.wrapping_add(lons[k]);
                        out.nodes.push((id, coord(lat_off, lat), coord(lon_off, lon)));
                    }
                }
                (3, 2) if ways || buildings => {
                    let (mut id, mut keys, mut vals, mut refs) = (0i64, Vec::new(), Vec::new(), Vec::new());
                    let mut w = Pb::new(p.bytes()?);
                    while w.more() {
                        match w.key()? {
                            (1, 0) => id = w.varint()? as i64,
                            (2, wire) => w.repeated(wire, |v| keys.push(v as usize))?,
                            (3, wire) => w.repeated(wire, |v| vals.push(v as usize))?,
                            (8, wire) => {
                                let mut r = 0i64;
                                w.repeated(wire, |v| {
                                    r = r.wrapping_add(zigzag(v));
                                    refs.push(r);
                                })?
                            }
                            (_, wire) => w.skip(wire)?,
                        }
                    }
                    if keys.len() != vals.len() {
                        return Err("PBF: way tags of different lengths".into());
                    }
                    let text = |i: usize| strings.get(i).copied().ok_or("PBF: string index out of range");
                    let mut tags = Vec::with_capacity(keys.len());
                    for (k, v) in keys.iter().zip(&vals) {
                        tags.push((text(*k)?, text(*v)?));
                    }
                    let closed = refs.len() >= 4 && refs.first() == refs.last();
                    if buildings
                        && closed
                        && let Some(h) = building_height(&tags)
                    {
                        out.buildings.push((refs.clone(), h));
                    }
                    if ways && closed && is_industrial_area(&tags) {
                        out.areas.push(refs.clone());
                    }
                    if ways
                        && refs.len() >= 2
                        && let Some(class) = classify(&tags)
                    {
                        out.ways.push(Way { id, class, refs });
                    }
                }
                (_, w) => p.skip(w)?,
            }
        }
    }
    Ok(out)
}

/// The kinds of way kept, in the order of the bits that store them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Highway {
    Path,
    Footway,
    Track,
    Bridleway,
    Steps,
    Pedestrian,
    LivingStreet,
    Residential,
    Unclassified,
    Service,
    Tertiary,
    Cycleway,
    Secondary,
    Primary,
}

const HIGHWAYS: [(&[u8], Highway); 14] = [
    (b"path", Highway::Path),
    (b"footway", Highway::Footway),
    (b"track", Highway::Track),
    (b"bridleway", Highway::Bridleway),
    (b"steps", Highway::Steps),
    (b"pedestrian", Highway::Pedestrian),
    (b"living_street", Highway::LivingStreet),
    (b"residential", Highway::Residential),
    (b"unclassified", Highway::Unclassified),
    (b"service", Highway::Service),
    (b"tertiary", Highway::Tertiary),
    (b"cycleway", Highway::Cycleway),
    (b"secondary", Highway::Secondary),
    (b"primary", Highway::Primary),
];

/// The surface, in four kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Surface {
    Unknown,
    Paved,
    Gravel,
    Natural,
}

/// A way's attributes, packed in a `u32`: highway (4 bits), surface (2), sac_scale (3: 0 none,
/// 1 hiking … 6 difficult_alpine_hiking), trail_visibility (3: 0 unknown, 1 excellent … 6 no),
/// tracktype (3: 0 unknown, 1 grade1 … 5 grade5), lit (2: 0 unknown, 1 lit, 2 unlit), and
/// industrial (1: inside an industrial area, set by the graph build, not read from the way).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Class(pub u32);

impl Class {
    pub fn new(h: Highway, s: Surface, sac: u8, visibility: u8, tracktype: u8) -> Class {
        Class(h as u32 | (s as u32) << 4 | (sac.min(7) as u32) << 6 | (visibility.min(7) as u32) << 9 | (tracktype.min(7) as u32) << 12)
    }

    /// The same class with `lit` (0 unknown, 1 lit, 2 unlit).
    pub fn with_lit(self, lit: u8) -> Class {
        Class(self.0 & !(3 << 15) | (lit.min(2) as u32) << 15)
    }

    pub fn with_industrial(self) -> Class {
        Class(self.0 | 1 << 17)
    }

    #[allow(dead_code)] // read by the route planner, not wired yet
    pub fn lit(self) -> u8 {
        (self.0 >> 15 & 3) as u8
    }

    #[allow(dead_code)] // read by the route planner, not wired yet
    pub fn industrial(self) -> bool {
        self.0 >> 17 & 1 == 1
    }

    pub fn highway(self) -> Highway {
        HIGHWAYS.get((self.0 & 15) as usize).map_or(Highway::Path, |h| h.1)
    }

    pub fn surface(self) -> Surface {
        [Surface::Unknown, Surface::Paved, Surface::Gravel, Surface::Natural][(self.0 >> 4 & 3) as usize]
    }

    pub fn sac_scale(self) -> u8 {
        (self.0 >> 6 & 7) as u8
    }

    pub fn visibility(self) -> u8 {
        (self.0 >> 9 & 7) as u8
    }

    pub fn tracktype(self) -> u8 {
        (self.0 >> 12 & 7) as u8
    }

    /// A way that is a trail rather than a street: a path, track or bridleway that isn't paved,
    /// or a footway or steps known to be unpaved.
    pub fn is_trail(self) -> bool {
        let unpaved = matches!(self.surface(), Surface::Gravel | Surface::Natural);
        match self.highway() {
            Highway::Path | Highway::Bridleway => self.surface() != Surface::Paved,
            Highway::Track => self.surface() != Surface::Paved && self.tracktype() != 1,
            Highway::Footway | Highway::Steps => unpaved,
            _ => false,
        }
    }

    /// A way that is asphalt (or concrete, or paving stones): said so, or a street that says
    /// nothing about it.
    pub fn is_paved(self) -> bool {
        match self.surface() {
            Surface::Paved => true,
            Surface::Unknown => matches!(
                self.highway(),
                Highway::Pedestrian
                    | Highway::LivingStreet
                    | Highway::Residential
                    | Highway::Unclassified
                    | Highway::Service
                    | Highway::Tertiary
                    | Highway::Cycleway
                    | Highway::Secondary
                    | Highway::Primary
            ) || (self.highway() == Highway::Track && self.tracktype() == 1),
            _ => false,
        }
    }
}

/// Whether a way can be run on, and its class: the highways above, unless access is private or
/// forbidden (and not reopened to walkers) or walkers are excluded.
pub fn classify(tags: &[(&[u8], &[u8])]) -> Option<Class> {
    let tag = |k: &[u8]| tags.iter().find(|(key, _)| *key == k).map(|(_, v)| *v);
    let highway = HIGHWAYS.iter().find(|(name, _)| Some(*name) == tag(b"highway"))?.1;
    let foot = tag(b"foot");
    if matches!(foot, Some(b"no" | b"private")) {
        return None;
    }
    let reopened = matches!(foot, Some(b"yes" | b"designated" | b"permissive"));
    if matches!(tag(b"access"), Some(b"private" | b"no")) && !reopened {
        return None;
    }
    let surface = match tag(b"surface").unwrap_or_default() {
        b"" => Surface::Unknown,
        b"paved" | b"asphalt" | b"concrete" | b"concrete:plates" | b"concrete:lanes" | b"paving_stones" | b"sett" | b"cobblestone"
        | b"unhewn_cobblestone" | b"metal" | b"wood" | b"chipseal" | b"bricks" => Surface::Paved,
        b"gravel" | b"fine_gravel" | b"compacted" | b"unpaved" | b"pebblestone" => Surface::Gravel,
        _ => Surface::Natural,
    };
    let sac = match tag(b"sac_scale").unwrap_or_default() {
        b"hiking" => 1,
        b"mountain_hiking" => 2,
        b"demanding_mountain_hiking" => 3,
        b"alpine_hiking" => 4,
        b"demanding_alpine_hiking" => 5,
        b"difficult_alpine_hiking" => 6,
        _ => 0,
    };
    let visibility = match tag(b"trail_visibility").unwrap_or_default() {
        b"excellent" => 1,
        b"good" => 2,
        b"intermediate" => 3,
        b"bad" => 4,
        b"horrible" => 5,
        b"no" => 6,
        _ => 0,
    };
    let tracktype = match tag(b"tracktype").unwrap_or_default() {
        b"grade1" => 1,
        b"grade2" => 2,
        b"grade3" => 3,
        b"grade4" => 4,
        b"grade5" => 5,
        _ => 0,
    };
    let lit = match tag(b"lit").unwrap_or_default() {
        b"" => 0,
        b"no" | b"disused" => 2,
        b"yes" | b"24/7" | b"automatic" | b"limited" | b"interval" | b"sunset-sunrise" => 1,
        _ => 0,
    };
    Some(Class::new(highway, surface, sac, visibility, tracktype).with_lit(lit))
}

/// A closed way that is an industrial, railway or quarry area.
fn is_industrial_area(tags: &[(&[u8], &[u8])]) -> bool {
    tags.iter().any(|(k, v)| *k == b"landuse" && matches!(*v, b"industrial" | b"railway" | b"quarry"))
}

/// The height in metres of a way tagged as a building (`None` when it is not one): its `height`,
/// else 3 m for each of its `building:levels`, else 8 m; between 2 and 400 m.
pub fn building_height(tags: &[(&[u8], &[u8])]) -> Option<f32> {
    let tag = |k: &[u8]| tags.iter().find(|(key, _)| *key == k).map(|(_, v)| *v);
    if matches!(tag(b"building"), None | Some(b"no")) {
        return None;
    }
    // "12", "12.5", "12 m", "12,5m": the number at the start.
    let number = |v: &[u8]| {
        let s = std::str::from_utf8(v).ok()?.trim().replace(',', ".");
        let end = s.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(s.len());
        s[..end].parse::<f32>().ok().filter(|n| n.is_finite() && *n > 0.0)
    };
    let h = tag(b"height").and_then(number).or_else(|| tag(b"building:levels").and_then(number).map(|l| l * 3.0)).unwrap_or(8.0);
    Some(h.clamp(2.0, 400.0))
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn buildings_have_a_height() {
        let h = |tags: &[(&str, &str)]| building_height(&tags.iter().map(|(k, v)| (k.as_bytes(), v.as_bytes())).collect::<Vec<_>>());
        assert_eq!(h(&[("building", "yes")]), Some(8.0));
        assert_eq!(h(&[("building", "house"), ("height", "12.5")]), Some(12.5));
        assert_eq!(h(&[("building", "yes"), ("height", " 21 m")]), Some(21.0));
        assert_eq!(h(&[("building", "yes"), ("height", "7,5m")]), Some(7.5));
        assert_eq!(h(&[("building", "yes"), ("building:levels", "5")]), Some(15.0));
        // The height wins over the levels; nonsense falls back, extremes are held.
        assert_eq!(h(&[("building", "yes"), ("building:levels", "5"), ("height", "20")]), Some(20.0));
        assert_eq!(h(&[("building", "yes"), ("height", "tall"), ("building:levels", "x")]), Some(8.0));
        assert_eq!(h(&[("building", "yes"), ("height", "0")]), Some(8.0));
        assert_eq!(h(&[("building", "yes"), ("height", "-4")]), Some(8.0));
        assert_eq!(h(&[("building", "yes"), ("height", "9999")]), Some(400.0));
        assert_eq!(h(&[("building", "yes"), ("height", "0.5")]), Some(2.0));
        assert_eq!(h(&[("building", "no")]), None);
        assert_eq!(h(&[("highway", "path"), ("height", "3")]), None);
        // Read from a block only when asked for, and only closed ways.
        let b = block_with(&[], &[(1, &[("building", "yes"), ("building:levels", "2")], &[1, 2, 3, 1]), (2, &[("building", "yes")], &[1, 2, 3]), (3, &[("highway", "path")], &[1, 2])]);
        assert!(block(&b, false, true).unwrap().buildings.is_empty());
        let got = block_of(&b, false, false, true).unwrap();
        assert_eq!(got.buildings, vec![(vec![1, 2, 3, 1], 6.0)]);
        assert!(got.ways.is_empty(), "the ways were not asked for");
        assert_eq!(block_of(&b, false, true, true).unwrap().ways.len(), 1);
    }

    /// A minimal protobuf writer, to make blocks the tests can read.
    #[derive(Default)]
    pub struct W(pub Vec<u8>);

    impl W {
        pub fn varint(&mut self, mut v: u64) -> &mut Self {
            while v >= 0x80 {
                self.0.push(v as u8 | 0x80);
                v >>= 7;
            }
            self.0.push(v as u8);
            self
        }
        pub fn key(&mut self, field: u64, wire: u64) -> &mut Self {
            self.varint(field << 3 | wire)
        }
        pub fn uint(&mut self, field: u64, v: u64) -> &mut Self {
            self.key(field, 0).varint(v)
        }
        pub fn bytes(&mut self, field: u64, b: &[u8]) -> &mut Self {
            self.key(field, 2).varint(b.len() as u64);
            self.0.extend_from_slice(b);
            self
        }
        pub fn packed(&mut self, field: u64, vs: impl IntoIterator<Item = u64>) -> &mut Self {
            let mut inner = W::default();
            for v in vs {
                inner.varint(v);
            }
            self.bytes(field, &inner.0)
        }
    }

    pub fn zz(v: i64) -> u64 {
        ((v << 1) ^ (v >> 63)) as u64
    }

    fn block_with(nodes: &[(i64, f64, f64)], ways: &[(i64, &[(&str, &str)], &[i64])]) -> Vec<u8> {
        let mut strings = vec![String::new()];
        let mut index = |s: &str| {
            if let Some(i) = strings.iter().position(|x| x == s) {
                return i as u64;
            }
            strings.push(s.into());
            (strings.len() - 1) as u64
        };
        let mut group = W::default();
        let delta = |v: &[i64]| v.iter().scan(0i64, |prev, x| { let d = x - *prev; *prev = *x; Some(zz(d)) }).collect::<Vec<_>>();
        let ids: Vec<i64> = nodes.iter().map(|n| n.0).collect();
        let lats: Vec<i64> = nodes.iter().map(|n| (n.1 * 1e7).round() as i64).collect();
        let lons: Vec<i64> = nodes.iter().map(|n| (n.2 * 1e7).round() as i64).collect();
        let mut dense = W::default();
        dense.packed(1, delta(&ids)).packed(8, delta(&lats)).packed(9, delta(&lons));
        group.bytes(2, &dense.0);
        for (id, tags, refs) in ways {
            let mut w = W::default();
            w.uint(1, *id as u64);
            let keys: Vec<u64> = tags.iter().map(|(k, _)| index(k)).collect();
            let vals: Vec<u64> = tags.iter().map(|(_, v)| index(v)).collect();
            w.packed(2, keys).packed(3, vals).packed(8, delta(refs));
            group.bytes(3, &w.0);
        }
        let mut table = W::default();
        for s in &strings {
            table.bytes(1, s.as_bytes());
        }
        let mut b = W::default();
        // Granularity 100 nanodegrees = 1e-7 degrees.
        b.bytes(1, &table.0).bytes(2, &group.0).uint(17, 100);
        b.0
    }

    #[test]
    fn reads_dense_nodes_and_ways() {
        let b = block_with(
            &[(10, 43.7, 7.25), (11, 43.701, 7.251), (12, 43.702, 7.252)],
            &[
                (100, &[("highway", "path"), ("surface", "ground"), ("sac_scale", "mountain_hiking")], &[10, 11, 12]),
                (101, &[("highway", "motorway")], &[10, 12]),
                (102, &[("highway", "track"), ("access", "private")], &[10, 12]),
                (103, &[("highway", "track"), ("access", "no"), ("foot", "yes"), ("tracktype", "grade3")], &[12, 10]),
                (104, &[("highway", "footway"), ("foot", "no")], &[10, 12]),
                (105, &[("highway", "path")], &[10]),
            ],
        );
        let all = block(&b, true, true).unwrap();
        assert_eq!(all.nodes, [(10, 437_000_000, 72_500_000), (11, 437_010_000, 72_510_000), (12, 437_020_000, 72_520_000)]);
        assert_eq!(all.ways.iter().map(|w| w.id).collect::<Vec<_>>(), [100, 103]);
        assert_eq!(all.ways[0].refs, [10, 11, 12]);
        let c = all.ways[0].class;
        assert_eq!((c.highway(), c.surface(), c.sac_scale()), (Highway::Path, Surface::Natural, 2));
        assert!(c.is_trail() && !c.is_paved());
        assert_eq!(all.ways[1].class.tracktype(), 3);
        // Only what is asked for is read.
        let ways = block(&b, false, true).unwrap();
        assert!(ways.nodes.is_empty() && ways.ways.len() == 2);
        assert!(block(&b, true, false).unwrap().ways.is_empty());
    }

    #[test]
    fn classes_say_trail_or_asphalt() {
        let c = |tags: &[(&str, &str)]| {
            let t: Vec<(&[u8], &[u8])> = tags.iter().map(|(k, v)| (k.as_bytes(), v.as_bytes())).collect();
            classify(&t)
        };
        let residential = c(&[("highway", "residential")]).unwrap();
        assert!(residential.is_paved() && !residential.is_trail());
        let gravel_road = c(&[("highway", "unclassified"), ("surface", "gravel")]).unwrap();
        assert!(!gravel_road.is_paved() && !gravel_road.is_trail());
        assert!(c(&[("highway", "track"), ("tracktype", "grade1")]).unwrap().is_paved());
        assert!(c(&[("highway", "footway")]).is_some_and(|c| !c.is_trail()));
        assert!(c(&[("highway", "footway"), ("surface", "dirt")]).unwrap().is_trail());
        assert_eq!(c(&[("highway", "steps"), ("trail_visibility", "bad")]).unwrap().visibility(), 4);
        // Main roads and cycleways link the rest, at a cost; motorways and trunks never.
        assert!(c(&[("highway", "cycleway")]).unwrap().is_paved());
        assert_eq!(c(&[("highway", "cycleway"), ("foot", "no")]), None);
        assert_eq!(c(&[("highway", "primary")]).unwrap().highway(), Highway::Primary);
        assert_eq!(c(&[("highway", "trunk")]), None);
        assert_eq!(c(&[("building", "yes")]), None);
        // Lighting: said yes in several ways, said no, or silent or odd.
        let lit = |v: &str| c(&[("highway", "residential"), ("lit", v)]).unwrap().lit();
        assert_eq!([lit("yes"), lit("24/7"), lit("automatic"), lit("limited"), lit("sunset-sunrise")], [1; 5]);
        assert_eq!([lit("no"), lit("disused")], [2; 2]);
        assert_eq!([lit("maybe"), lit("")], [0; 2]);
        assert_eq!(residential.lit(), 0);
        // The new bits leave the others alone.
        let both = gravel_road.with_lit(2).with_industrial();
        assert!(both.lit() == 2 && both.industrial() && !gravel_road.industrial());
        assert_eq!((both.highway(), both.surface()), (Highway::Unclassified, Surface::Gravel));
        assert_eq!(both.with_lit(1).lit(), 1);
    }

    #[test]
    fn reads_closed_industrial_areas() {
        let ring = &[10, 11, 12, 10];
        let b = block_with(
            &[(10, 43.7, 7.25), (11, 43.701, 7.251), (12, 43.702, 7.252)],
            &[
                (200, &[("landuse", "industrial")], ring),
                (201, &[("landuse", "quarry")], ring),
                (202, &[("landuse", "railway")], ring),
                (203, &[("landuse", "forest")], ring),
                (204, &[("landuse", "industrial")], &[10, 11, 12]),
            ],
        );
        let r = block(&b, false, true).unwrap();
        assert_eq!(r.areas.len(), 3);
        assert!(r.ways.is_empty());
    }

    #[test]
    fn blobs_inflate_and_damage_is_an_error() {
        use std::io::Write;
        let data = b"hello blocks".repeat(10);
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(&data).unwrap();
        let z = z.finish().unwrap();
        let mut blob = W::default();
        blob.uint(2, data.len() as u64).bytes(3, &z);
        assert_eq!(blob_data(&blob.0).unwrap(), data);
        // Announced larger than it is, smaller than it is, or not zlib.
        let mut wrong = W::default();
        wrong.uint(2, data.len() as u64 + 5).bytes(3, &z);
        assert!(blob_data(&wrong.0).is_err());
        let mut wrong = W::default();
        wrong.uint(2, 3).bytes(3, &z);
        assert!(blob_data(&wrong.0).is_err());
        let mut lzma = W::default();
        lzma.uint(2, 3).bytes(4, b"xyz");
        assert!(blob_data(&lzma.0).unwrap_err().contains("compression"));
        // Header: type and size.
        let mut h = W::default();
        h.bytes(1, b"OSMData").uint(3, 1234);
        assert_eq!(blob_header(&h.0).unwrap(), ("OSMData".into(), 1234));
        let mut h = W::default();
        h.bytes(1, b"OSMData").uint(3, 1 << 30);
        assert!(blob_header(&h.0).is_err());
        let mut hb = W::default();
        hb.bytes(4, b"OsmSchema-V0.6").bytes(4, b"DenseNodes").uint(32, 1_700_000_000);
        assert_eq!(header(&hb.0).unwrap().timestamp, 1_700_000_000);
        let mut hb = W::default();
        hb.bytes(4, b"HistoricalInformation");
        assert!(header(&hb.0).is_err());
    }

    #[test]
    fn garbage_never_panics() {
        let b = block_with(&[(1, 1.0, 1.0), (2, 1.1, 1.1)], &[(5, &[("highway", "path")], &[1, 2])]);
        // Every truncation and a few flipped bytes: an error or a block, never a panic.
        for n in 0..b.len() {
            let _ = block(&b[..n], true, true);
        }
        let mut seed = 7u64;
        for _ in 0..2000 {
            let mut c = b.clone();
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let i = (seed >> 33) as usize % c.len();
            c[i] ^= (seed >> 13) as u8 | 1;
            let _ = block(&c, true, true);
            let _ = blob_header(&c);
            let _ = blob_data(&c);
        }
    }
}
