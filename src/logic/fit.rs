//! FIT files: the only place Garmin's Personal API gives the track, the samples and the laps.
//! Pure functions: bytes in, track out, then what the views derive from it.

use fitparser::{FitDataRecord, Value, profile::MesgNum};
use serde::{Deserialize, Serialize};
use std::io::{Cursor, Read};

/// One sample of the recording.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sample {
    /// Seconds since the first sample.
    pub t: f32,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    /// Metres since the start.
    pub dist: Option<f32>,
    pub alt: Option<f32>,
    pub hr: Option<f32>,
    /// m/s.
    pub speed: Option<f32>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Lap {
    pub time: f32,
    pub dist: f32,
    pub hr: Option<f32>,
    pub ascent: Option<f32>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Track {
    pub samples: Vec<Sample>,
    pub laps: Vec<Lap>,
}

/// Garmin serves the original file inside a ZIP; a bare FIT is accepted too.
pub fn unzip(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.get(8..12) == Some(b".FIT") {
        return Ok(bytes.to_vec());
    }
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| format!("ZIP illisible : {e}"))?;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| e.to_string())?;
        if f.name().is_ok_and(|n| n.to_ascii_lowercase().ends_with(".fit")) {
            let mut out = Vec::new();
            f.read_to_end(&mut out).map_err(|e| e.to_string())?;
            return Ok(out);
        }
    }
    Err("Aucun fichier FIT dans l'archive.".into())
}

fn num(v: &Value) -> Option<f64> {
    Some(match v {
        Value::Byte(x) | Value::UInt8(x) | Value::UInt8z(x) | Value::Enum(x) => *x as f64,
        Value::SInt8(x) => *x as f64,
        Value::SInt16(x) => *x as f64,
        Value::UInt16(x) | Value::UInt16z(x) => *x as f64,
        Value::SInt32(x) => *x as f64,
        Value::UInt32(x) | Value::UInt32z(x) => *x as f64,
        Value::SInt64(x) => *x as f64,
        Value::UInt64(x) | Value::UInt64z(x) => *x as f64,
        Value::Float32(x) => *x as f64,
        Value::Float64(x) => *x,
        _ => return None,
    })
}

fn field(r: &FitDataRecord, name: &str) -> Option<f64> {
    r.fields().iter().find(|f| f.name() == name).and_then(|f| num(f.value())).filter(|v| v.is_finite())
}

fn timestamp(r: &FitDataRecord) -> Option<f64> {
    r.fields().iter().find(|f| f.name() == "timestamp").and_then(|f| match f.value() {
        Value::Timestamp(t) => Some(t.timestamp_millis() as f64 / 1000.0),
        _ => None,
    })
}

const SEMICIRCLE: f64 = 180.0 / 2_147_483_648.0;

pub fn parse(fit: &[u8]) -> Result<Track, String> {
    if fit.len() < 12 {
        return Err("Fichier FIT vide.".into());
    }
    let records = fitparser::from_bytes(fit).map_err(|e| format!("FIT illisible : {e}"))?;
    let mut track = Track::default();
    let mut t0 = None;
    for r in &records {
        match r.kind() {
            MesgNum::Record => {
                let Some(ts) = timestamp(r) else { continue };
                let t0 = *t0.get_or_insert(ts);
                let lat = field(r, "position_lat").map(|v| v * SEMICIRCLE);
                let lon = field(r, "position_long").map(|v| v * SEMICIRCLE);
                // A position at (0, 0) or out of range is a missing fix.
                let (lat, lon) = match (lat, lon) {
                    (Some(a), Some(o)) if a.abs() <= 90.0 && o.abs() <= 180.0 && (a, o) != (0.0, 0.0) => (Some(a), Some(o)),
                    _ => (None, None),
                };
                track.samples.push(Sample {
                    t: (ts - t0) as f32,
                    lat,
                    lon,
                    dist: field(r, "distance").map(|v| v as f32),
                    alt: field(r, "enhanced_altitude").or_else(|| field(r, "altitude")).map(|v| v as f32),
                    hr: field(r, "heart_rate").filter(|h| *h > 0.0).map(|v| v as f32),
                    speed: field(r, "enhanced_speed").or_else(|| field(r, "speed")).map(|v| v as f32),
                });
            }
            MesgNum::Lap => track.laps.push(Lap {
                time: field(r, "total_timer_time").or_else(|| field(r, "total_elapsed_time")).unwrap_or(0.0) as f32,
                dist: field(r, "total_distance").unwrap_or(0.0) as f32,
                hr: field(r, "avg_heart_rate").map(|v| v as f32),
                ascent: field(r, "total_ascent").map(|v| v as f32),
            }),
            _ => {}
        }
    }
    // Multisport files restart nothing, but a device can repeat a timestamp: keep time monotonic.
    track.samples.sort_by(|a, b| a.t.total_cmp(&b.t));
    Ok(track)
}

/// Equirectangular distance in metres: plenty for consecutive GPS fixes.
pub fn metres(a: [f64; 2], b: [f64; 2]) -> f64 {
    let k = std::f64::consts::PI / 180.0;
    let x = (b[1] - a[1]) * k * ((a[0] + b[0]) * 0.5 * k).cos();
    let y = (b[0] - a[0]) * k;
    (x * x + y * y).sqrt() * 6_371_000.0
}

impl Track {
    pub fn positions(&self) -> impl Iterator<Item = [f64; 2]> + '_ {
        self.samples.iter().filter_map(|s| Some([s.lat?, s.lon?]))
    }

    /// (seconds, metres) pairs with a recorded distance, for efforts and splits.
    fn dist_time(&self) -> Vec<(f64, f64)> {
        let mut out: Vec<(f64, f64)> = Vec::new();
        for s in &self.samples {
            if let Some(d) = s.dist {
                // Distance never goes back; a glitch that does is dropped.
                if out.last().is_none_or(|l| d as f64 >= l.1) {
                    out.push((s.t as f64, d as f64));
                }
            }
        }
        out
    }

    /// Track simplified to a point every `step` metres, as `[lat, lon]`, for maps.
    pub fn line(&self, step: f64) -> Vec<[f32; 2]> {
        let mut out: Vec<[f64; 2]> = Vec::new();
        let all: Vec<_> = self.positions().collect();
        for (i, p) in all.iter().enumerate() {
            if out.last().is_none_or(|l| metres(*l, *p) >= step) || i == all.len() - 1 {
                out.push(*p);
            }
        }
        out.into_iter().map(|p| [p[0] as f32, p[1] as f32]).collect()
    }

    /// Fastest time over each distance, as Strava's best efforts: the shortest window of the
    /// recording covering it, scaled to the exact distance.
    pub fn best_efforts(&self, targets: &[f64]) -> Vec<Effort> {
        let pts = self.dist_time();
        let mut out = Vec::new();
        for &target in targets {
            let mut best: Option<f64> = None;
            let mut i = 0;
            for j in 0..pts.len() {
                while i + 1 < j && pts[j].1 - pts[i + 1].1 >= target {
                    i += 1;
                }
                let d = pts[j].1 - pts[i].1;
                if d >= target && d > 0.0 {
                    let t = (pts[j].0 - pts[i].0) * target / d;
                    if t > 0.0 && best.is_none_or(|b| t < b) {
                        best = Some(t);
                    }
                }
            }
            if let Some(time) = best {
                out.push(Effort { metres: target as f32, seconds: time as f32 });
            }
        }
        out
    }

    /// One split per full `every` metres, plus the remainder: (metres, seconds, avg HR, ascent).
    pub fn splits(&self, every: f64) -> Vec<Split> {
        let pts = self.dist_time();
        let Some(&(_, total)) = pts.last() else { return Vec::new() };
        let at = |d: f64| -> f64 {
            // Time when the distance `d` was reached, interpolated.
            let j = pts.partition_point(|p| p.1 < d).min(pts.len() - 1);
            if j == 0 {
                return pts[0].0;
            }
            let (a, b) = (pts[j - 1], pts[j]);
            if b.1 > a.1 { a.0 + (b.0 - a.0) * (d - a.1) / (b.1 - a.1) } else { b.0 }
        };
        let mut out = Vec::new();
        let mut from = 0.0;
        while from < total - 1.0 {
            let to = (from + every).min(total);
            let (t0, t1) = (at(from), at(to));
            let inside = self.samples.iter().filter(|s| (s.t as f64) >= t0 && (s.t as f64) <= t1);
            let (mut hr_sum, mut hr_n, mut up, mut last_alt) = (0.0, 0, 0.0, None::<f32>);
            for s in inside {
                if let Some(h) = s.hr {
                    hr_sum += h as f64;
                    hr_n += 1;
                }
                if let Some(a) = s.alt {
                    if let Some(l) = last_alt {
                        up += (a - l).max(0.0) as f64;
                    }
                    last_alt = Some(a);
                }
            }
            out.push(Split {
                metres: (to - from) as f32,
                seconds: (t1 - t0) as f32,
                hr: (hr_n > 0).then(|| (hr_sum / hr_n as f64) as f32),
                ascent: up as f32,
            });
            from = to;
        }
        out
    }

    /// The altitude at `n` points spread evenly along the outing, each the mean of its stretch (a
    /// stretch without a sample takes the one before). Along the distance when most samples have
    /// one, along the time otherwise; empty when fewer than two samples have an altitude.
    pub fn altitude(&self, n: usize) -> Vec<f32> {
        let by_distance = self.samples.iter().filter(|s| s.dist.is_some()).count() * 2 > self.samples.len();
        let mut pts: Vec<(f32, f32)> = Vec::new();
        for s in &self.samples {
            let (Some(alt), Some(x)) = (s.alt, if by_distance { s.dist } else { Some(s.t) }) else { continue };
            if alt.is_finite() && pts.last().is_none_or(|l| x >= l.0) {
                pts.push((x, alt));
            }
        }
        let (Some(first), Some(last)) = (pts.first(), pts.last()) else { return vec![] };
        let span = last.0 - first.0;
        if pts.len() < 2 || span <= 0.0 || n == 0 {
            return vec![];
        }
        let mut sums = vec![(0.0f64, 0u32); n];
        for (x, alt) in &pts {
            let k = (((x - first.0) / span * n as f32) as usize).min(n - 1);
            sums[k].0 += *alt as f64;
            sums[k].1 += 1;
        }
        let mut out: Vec<f32> = Vec::with_capacity(n);
        for (sum, count) in sums {
            let v = if count > 0 { (sum / count as f64) as f32 } else { out.last().copied().unwrap_or(first.1) };
            out.push(v);
        }
        out
    }

    /// Seconds per `BAND`-bpm band of heart rate. Each sample counts until the next one, at most
    /// 20 s: a pause is not time spent at the heart rate it started with.
    pub fn hr_bands(&self) -> Vec<f32> {
        let mut bands: Vec<f32> = Vec::new();
        for w in self.samples.windows(2) {
            let Some(hr) = w[0].hr.filter(|h| *h > 0.0 && *h < 300.0) else { continue };
            let i = (hr / BAND) as usize;
            if bands.len() <= i {
                bands.resize(i + 1, 0.0);
            }
            bands[i] += (w[1].t - w[0].t).clamp(0.0, 20.0);
        }
        bands
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Effort {
    pub metres: f32,
    pub seconds: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Split {
    pub metres: f32,
    pub seconds: f32,
    pub hr: Option<f32>,
    pub ascent: f32,
}

/// What every activity keeps once its file has been read: enough for the maps, the records and
/// the journal's small charts.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Derived {
    pub line: Vec<[f32; 2]>,
    pub efforts: Vec<Effort>,
    /// Altitude in metres at `PROFILE` points spread evenly over the distance (over the time when
    /// nothing recorded one), each the mean of its stretch; empty without altitude.
    #[serde(default)]
    pub profile: Vec<f32>,
    /// Seconds spent in each band of `BAND` bpm (band `i` starts at `i * BAND`); empty without
    /// heart rate.
    #[serde(default)]
    pub hr_bands: Vec<f32>,
    /// The `VERSION` that derived it: an older entry is derived again from its file.
    #[serde(default)]
    pub version: u32,
}

/// Raised when `derive` keeps something new.
pub const VERSION: u32 = 1;
pub const PROFILE: usize = 48;
pub const BAND: f32 = 5.0;

/// Distances of the records, in metres.
pub const EFFORTS: [f64; 13] =
    [400.0, 1000.0, 1609.344, 5000.0, 10_000.0, 15_000.0, 20_000.0, 21_097.5, 42_195.0, 50_000.0, 100_000.0, 1500.0, 40_000.0];

pub fn derive(track: &Track) -> Derived {
    Derived { line: track.line(15.0), efforts: track.best_efforts(&EFFORTS), profile: track.altitude(PROFILE), hr_bands: track.hr_bands(), version: VERSION }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
    }

    fn sample(t: f32, dist: f32) -> Sample {
        Sample { t, dist: Some(dist), ..Default::default() }
    }

    #[test]
    fn parses_a_real_bike_file() {
        let tr = parse(&fixture("garmin-fenix-5-bike.fit")).unwrap();
        assert_eq!(tr.samples.len(), 19);
        assert_eq!(tr.laps.len(), 1);
        assert!((tr.laps[0].dist - 459.52).abs() < 0.01);
        let first = tr.samples[0];
        assert_eq!(first.t, 0.0);
        // 446332520 semicircles ≈ 37.41°.
        assert!((first.lat.unwrap() - 37.411).abs() < 0.01, "{first:?}");
        assert!(tr.samples.windows(2).all(|w| w[0].t <= w[1].t));
    }

    #[test]
    fn parses_a_multisport_file_and_one_without_gps() {
        let tri = parse(&fixture("sample_mulitple_header.fit")).unwrap();
        assert!(tri.positions().count() > 1000);
        assert_eq!(tri.laps.len(), 5);
        let indoor = parse(&fixture("hrv-activity.fit")).unwrap();
        assert_eq!(indoor.positions().count(), 0);
        assert!(indoor.line(15.0).is_empty());
        assert!(indoor.samples.iter().any(|s| s.hr.is_some()));
        assert!(!derive(&indoor).efforts.is_empty(), "distance without GPS still gives efforts");
    }

    #[test]
    fn rejects_garbage_and_reads_zip() {
        assert!(parse(b"not a fit file").is_err());
        assert!(parse(&[]).is_err());
        assert!(unzip(b"nope").is_err());
        let raw = fixture("garmin-fenix-5-bike.fit");
        assert_eq!(unzip(&raw).unwrap(), raw);
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(Cursor::new(&mut buf));
            w.start_file("123_ACTIVITY.fit", zip::write::SimpleFileOptions::default()).unwrap();
            std::io::Write::write_all(&mut w, &raw).unwrap();
            w.finish().unwrap();
        }
        assert_eq!(unzip(&buf).unwrap(), raw);
    }

    #[test]
    fn best_efforts_find_the_fastest_window() {
        // 10 km at 5:00/km, except km 4-5 in 4:00.
        let mut s = Vec::new();
        let mut t = 0.0;
        for m in 0..=10_000 {
            s.push(sample(t, m as f32));
            t += if (4000..5000).contains(&m) { 0.24 } else { 0.30 };
        }
        let tr = Track { samples: s, laps: vec![] };
        let e = tr.best_efforts(&[1000.0, 5000.0, 10_000.0, 21_097.5]);
        assert_eq!(e.len(), 3, "no half marathon in 10 km");
        assert!((e[0].seconds - 240.0).abs() < 1.0, "{:?}", e[0]);
        assert!((e[1].seconds - 1440.0).abs() < 1.0, "{:?}", e[1]);
        assert!((e[2].seconds - 2940.0).abs() < 1.0, "{:?}", e[2]);
        assert!(Track::default().best_efforts(&[1000.0]).is_empty());
        // A distance that goes back is ignored, not counted twice.
        let glitch = Track { samples: vec![sample(0.0, 0.0), sample(10.0, 500.0), sample(11.0, 100.0), sample(300.0, 1000.0)], laps: vec![] };
        assert!((glitch.best_efforts(&[1000.0])[0].seconds - 300.0).abs() < 0.01);
    }

    #[test]
    fn splits_per_km_with_a_remainder() {
        let s: Vec<_> = (0..=2500).map(|m| Sample { hr: Some(150.0), alt: Some(m as f32 / 100.0), ..sample(m as f32 * 0.3, m as f32) }).collect();
        let sp = Track { samples: s, laps: vec![] }.splits(1000.0);
        assert_eq!(sp.len(), 3);
        assert!((sp[0].seconds - 300.0).abs() < 0.5);
        assert!((sp[2].metres - 500.0).abs() < 0.5);
        assert_eq!(sp[0].hr, Some(150.0));
        assert!((sp[0].ascent - 10.0).abs() < 0.1);
        assert!(Track::default().splits(1000.0).is_empty());
    }

    #[test]
    fn altitude_profile_and_heart_rate_bands() {
        // 1 km climbing from 100 m to 200 m, one sample per metre.
        let s: Vec<_> = (0..=1000).map(|m| Sample { alt: Some(100.0 + m as f32 / 10.0), ..sample(m as f32, m as f32) }).collect();
        let p = Track { samples: s.clone(), laps: vec![] }.altitude(10);
        assert_eq!(p.len(), 10);
        assert!((p[0] - 105.0).abs() < 0.1 && (p[9] - 195.0).abs() < 0.2, "{p:?}");
        assert!(p.windows(2).all(|w| w[1] > w[0]));
        // A gap in the samples: the empty stretch takes the one before, nothing drops to zero.
        let holes: Vec<_> = s.iter().copied().filter(|x| !(400.0..600.0).contains(&x.dist.unwrap())).collect();
        let p = Track { samples: holes, laps: vec![] }.altitude(10);
        assert_eq!(p[4], p[3]);
        assert!(p.iter().all(|v| *v >= 100.0));
        // No altitude, one sample, nothing at all, or no room: empty.
        assert!(Track { samples: (0..10).map(|i| sample(i as f32, i as f32)).collect(), laps: vec![] }.altitude(10).is_empty());
        assert!(Track { samples: s[..1].to_vec(), laps: vec![] }.altitude(10).is_empty());
        assert!(Track::default().altitude(10).is_empty());
        assert!(Track { samples: s.clone(), laps: vec![] }.altitude(0).is_empty());
        // Indoors (no distance): along the time.
        let indoor: Vec<_> = (0..100).map(|i| Sample { t: i as f32, alt: Some(i as f32), ..Default::default() }).collect();
        assert_eq!(Track { samples: indoor, laps: vec![] }.altitude(4).len(), 4);

        // 60 s at 142 bpm, then 30 samples at 171 with a 5 min pause among them, then a last one.
        let mut hr = vec![];
        for i in 0..60 {
            hr.push(Sample { t: i as f32, hr: Some(142.0), ..Default::default() });
        }
        for i in 0..30 {
            hr.push(Sample { t: 60.0 + i as f32 + if i >= 10 { 300.0 } else { 0.0 }, hr: Some(171.0), ..Default::default() });
        }
        hr.push(Sample { t: 400.0, hr: Some(90.0), ..Default::default() });
        let b = Track { samples: hr, laps: vec![] }.hr_bands();
        assert_eq!(b.len(), 35);
        assert!((b[28] - 60.0).abs() < 0.01, "{}", b[28]);
        // 9 + 19 one-second steps, the pause counted 20 s (not 300), and 11 s to the last sample.
        assert!((b[34] - (9.0 + 20.0 + 19.0 + 11.0)).abs() < 0.01, "{}", b[34]);
        assert_eq!(b[18], 0.0);
        assert!(Track::default().hr_bands().is_empty());
        let junk = Track { samples: vec![Sample { hr: Some(0.0), ..Default::default() }, Sample { t: 5.0, hr: Some(900.0), ..Default::default() }, Sample { t: 9.0, ..Default::default() }], laps: vec![] };
        assert!(junk.hr_bands().is_empty());
        // What is kept says which version kept it.
        assert_eq!(derive(&Track::default()).version, VERSION);
        assert_eq!(Derived::default().version, 0);
    }

    #[test]
    fn line_is_simplified() {
        let s: Vec<_> = (0..1000).map(|i| Sample { lat: Some(45.0 + i as f64 * 1e-5), lon: Some(4.8), ..Default::default() }).collect();
        let l = Track { samples: s, laps: vec![] }.line(15.0);
        // 1000 points 1.1 m apart ≈ 1.1 km: about one point per 15 m, ends kept.
        assert!((70..=80).contains(&l.len()), "{}", l.len());
        assert!((l.last().unwrap()[0] - (45.0 + 999.0 * 1e-5) as f32).abs() < 1e-6);
        assert!((metres([45.0, 4.8], [45.001, 4.8]) - 111.2).abs() < 0.5);
    }
}
