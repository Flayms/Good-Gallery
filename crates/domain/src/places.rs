//! Places named from GPS positions (spec 042): the nearest town or city of 5,000 people or more, with its region and
//! country, from GeoNames (CC BY 4.0, <https://www.geonames.org>), bundled so nothing is looked up online.

use crate::media::Location;
use std::collections::HashMap;
use std::sync::OnceLock;

/// The bundled places, `cargo xtask places` writes them.
const DATA: &[u8] = include_bytes!("../data/places.deflate");
/// A position farther than this from every town names no place: at sea, in a desert, on a glacier.
pub const MAX_DISTANCE_KM: f64 = 100.0;

/// A place as Places shows it (REQ-042-01).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Place {
    pub country: &'static str,
    pub region: Option<&'static str>,
    pub city: &'static str,
}

impl Place {
    /// Its tag path under Places: `Places › Country › Region › City`, without the region where GeoNames has none.
    pub fn tag_path(&self) -> Vec<String> {
        ["Places", self.country]
            .into_iter()
            .chain(self.region)
            .chain([self.city])
            .map(String::from)
            .collect()
    }
}

/// The place nearest to `location`, if one lies within [`MAX_DISTANCE_KM`] (REQ-042-01).
pub fn place_at(location: Location) -> Option<Place> {
    let index = INDEX.get_or_init(Index::load);
    let (latitude, longitude) = (location.latitude, location.longitude);
    // Cells of one degree: those within reach of the position, wider in longitude towards the poles.
    let reach_lon = (MAX_DISTANCE_KM / (KM_PER_DEGREE * latitude.to_radians().cos().max(0.01))).ceil() as i32 + 1;
    let (cell_lat, cell_lon) = (latitude.floor() as i32, longitude.floor() as i32);
    let mut best: Option<(f64, &City)> = None;
    for d_lat in -2..=2 {
        for d_lon in -reach_lon.min(180)..=reach_lon.min(180) {
            let lon = (cell_lon + d_lon + 180).rem_euclid(360) - 180;
            let Some(cities) = index.grid.get(&(cell_lat + d_lat, lon)) else {
                continue;
            };
            for &city in cities {
                let city = &index.cities[city as usize];
                let km = distance_km(latitude, longitude, city.latitude, city.longitude);
                if km <= MAX_DISTANCE_KM && best.is_none_or(|(nearest, _)| km < nearest) {
                    best = Some((km, city));
                }
            }
        }
    }
    best.map(|(_, city)| Place {
        country: city.country,
        region: city.region,
        city: city.name,
    })
}

/// Kilometres a degree of latitude spans.
const KM_PER_DEGREE: f64 = 111.2;

/// The great-circle distance between two positions.
fn distance_km(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let (dp, dl) = ((lat2 - lat1).to_radians(), (lon2 - lon1).to_radians());
    let a = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    2.0 * 6371.0 * a.sqrt().min(1.0).asin()
}

struct City {
    latitude: f64,
    longitude: f64,
    country: &'static str,
    region: Option<&'static str>,
    name: &'static str,
}

/// The places by one-degree cell of their position.
struct Index {
    cities: Vec<City>,
    grid: HashMap<(i32, i32), Vec<u32>>,
}

static INDEX: OnceLock<Index> = OnceLock::new();

impl Index {
    /// Inflates and reads the bundled places, once; its text lives as long as the app.
    fn load() -> Self {
        let bytes = miniz_oxide::inflate::decompress_to_vec(DATA).unwrap_or_default();
        let text: &'static str = Box::leak(String::from_utf8(bytes).unwrap_or_default().into_boxed_str());
        let (mut countries, mut regions) = (HashMap::new(), HashMap::new());
        let mut index = Self {
            cities: Vec::new(),
            grid: HashMap::new(),
        };
        for line in text.lines() {
            let fields: Vec<&'static str> = line.split('\t').collect();
            match fields.as_slice() {
                ["C", code, name] => {
                    countries.insert(*code, *name);
                }
                ["R", code, name] => {
                    regions.insert(*code, *name);
                }
                ["P", latitude, longitude, country, region, name] => {
                    let (Ok(latitude), Ok(longitude)) = (latitude.parse::<i32>(), longitude.parse::<i32>()) else {
                        continue;
                    };
                    let Some(country) = countries.get(country) else {
                        continue;
                    };
                    let city = City {
                        latitude: f64::from(latitude) / 1e4,
                        longitude: f64::from(longitude) / 1e4,
                        country,
                        region: regions.get(region).copied(),
                        name,
                    };
                    let cell = (city.latitude.floor() as i32, city.longitude.floor() as i32);
                    index.grid.entry(cell).or_default().push(index.cities.len() as u32);
                    index.cities.push(city);
                }
                _ => {}
            }
        }
        index
    }
}

/// Every country's names in Unicode CLDR's fully covered languages, `cargo xtask countries` writes them.
const COUNTRIES: &[u8] = include_bytes!("../data/countries.deflate");

/// The country a name means in any of those languages, as its two-letter code; letter case, spacing and accents as
/// tags compare them. `None` for a name that isn't a country's, or that names different countries (REQ-042-06).
pub fn country_code(name: &str) -> Option<&'static str> {
    static NAMES: OnceLock<HashMap<String, Option<&'static str>>> = OnceLock::new();
    let names = NAMES.get_or_init(|| {
        let inflate = |data: &[u8]| -> &'static str {
            let bytes = miniz_oxide::inflate::decompress_to_vec(data).unwrap_or_default();
            Box::leak(String::from_utf8(bytes).unwrap_or_default().into_boxed_str())
        };
        let mut names: HashMap<String, Option<&'static str>> = HashMap::new();
        let mut add = |code: &'static str, name: &str| {
            names
                .entry(crate::tags::normalize_tag(name))
                .and_modify(|known| {
                    if *known != Some(code) {
                        *known = None;
                    }
                })
                .or_insert(Some(code));
        };
        for line in inflate(COUNTRIES).lines().filter(|line| !line.starts_with('#')) {
            let mut fields = line.split('\t');
            let code = fields.next().unwrap_or_default();
            fields.for_each(|name| add(code, name));
        }
        // GeoNames' names of the bundled places' countries, which CLDR may spell differently ("and" for "&").
        for line in inflate(DATA).lines() {
            if let ["C", code, name] = line.split('\t').collect::<Vec<_>>().as_slice() {
                add(code, name);
            }
        }
        names
    });
    names.get(&crate::tags::normalize_tag(name)).copied().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn req_042_06_knows_a_country_by_its_name_in_any_language() {
        for name in [
            "Germany",
            "Deutschland",
            "deutschland",
            "Allemagne",
            "Alemania",
            "Niemcy",
            "ドイツ",
            "Германия",
        ] {
            assert_eq!(country_code(name), Some("DE"), "{name}");
        }
        assert_eq!(country_code("USA"), Some("US"), "short names too");
        assert_eq!(country_code("Mexico"), Some("MX"));
        assert_eq!(country_code("México"), Some("MX"), "accents as tags compare them");
        assert_eq!(country_code("Berlin"), None);
        // The bundled places' countries are all known.
        let index = INDEX.get_or_init(Index::load);
        let unknown: std::collections::BTreeSet<&str> = index
            .cities
            .iter()
            .map(|city| city.country)
            .filter(|country| country_code(country).is_none())
            .collect();
        assert!(unknown.is_empty(), "{unknown:?}");
    }

    fn at(latitude: f64, longitude: f64) -> Option<Place> {
        place_at(Location::new(latitude, longitude).unwrap())
    }

    #[test]
    fn req_042_01_names_the_nearest_town_with_its_region_and_country() {
        let munich = at(48.1374, 11.5755).unwrap();
        assert_eq!(
            munich,
            Place {
                country: "Germany",
                region: Some("Bavaria"),
                city: "Munich"
            }
        );
        assert_eq!(munich.tag_path(), ["Places", "Germany", "Bavaria", "Munich"]);
        let atacama = at(-22.91, -68.20).unwrap();
        assert_eq!((atacama.country, atacama.city), ("Chile", "San Pedro de Atacama"));
        // Across the antimeridian: Fiji's Taveuni at 180° west and east alike.
        assert_eq!(at(-16.80, 179.99).map(|p| p.country), Some("Fiji"));
        assert_eq!(at(-16.80, -179.99).map(|p| p.country), Some("Fiji"));
    }

    #[test]
    fn req_042_01_far_from_every_town_names_no_place() {
        assert_eq!(at(0.0, -150.0), None, "the Pacific");
        assert_eq!(at(-75.0, 0.0), None, "Antarctica");
    }

    #[test]
    fn req_042_05_a_library_of_positions_is_named_quickly() {
        let started = std::time::Instant::now();
        place_at(Location::new(1.0, 1.0).unwrap());
        let loaded = started.elapsed();
        let started = std::time::Instant::now();
        for i in 0..20_000 {
            let latitude = -60.0 + f64::from(i % 1_200) * 0.1;
            let longitude = -180.0 + f64::from(i) * 0.018;
            std::hint::black_box(place_at(Location::new(latitude, longitude).unwrap()));
        }
        let looked_up = started.elapsed();
        assert!(loaded.as_secs_f64() < 2.0, "loading took {loaded:?}");
        assert!(looked_up.as_secs_f64() < 2.0, "20,000 positions took {looked_up:?}");
    }
}
