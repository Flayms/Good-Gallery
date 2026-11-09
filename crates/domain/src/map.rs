//! The world map (spec 044): Web Mercator, the part of the world a view shows, and photos grouped into clusters by
//! where they fall on screen.

/// The tile size the zoom levels are counted in: at zoom 0 the world is this many points wide.
pub const WORLD_POINTS: f64 = 256.0;
/// How far in and out the map zooms: the whole world, down to a town's streets.
pub const MIN_ZOOM: f64 = 1.0;
pub const MAX_ZOOM: f64 = 16.0;
/// Mercator stops short of the poles.
const MAX_LATITUDE: f64 = 85.051_128_78;

/// `(longitude, latitude)` in degrees on Web Mercator's square world, 0–1 from the west and the north.
pub fn project(longitude: f64, latitude: f64) -> [f64; 2] {
    let x = (longitude + 180.0) / 360.0;
    let sin = latitude.clamp(-MAX_LATITUDE, MAX_LATITUDE).to_radians().sin();
    let y = 0.5 - ((1.0 + sin) / (1.0 - sin)).ln() / (4.0 * std::f64::consts::PI);
    [x, y]
}

/// The longitude and latitude of a point on the world square.
pub fn unproject([x, y]: [f64; 2]) -> (f64, f64) {
    let longitude = x * 360.0 - 180.0;
    let latitude = (std::f64::consts::PI * (1.0 - 2.0 * y)).sinh().atan().to_degrees();
    (longitude, latitude)
}

/// What a map shows: the world point at its center and its zoom level.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapView {
    pub center: [f64; 2],
    pub zoom: f64,
}

impl Default for MapView {
    /// The whole world, Europe a little above the middle.
    fn default() -> Self {
        Self {
            center: project(10.0, 25.0),
            zoom: MIN_ZOOM,
        }
    }
}

impl MapView {
    /// Points a world square is wide at this zoom.
    pub fn scale(&self) -> f64 {
        WORLD_POINTS * self.zoom.exp2()
    }

    /// Where world point `p` lies, in points from the center of the view.
    pub fn to_screen(&self, p: [f64; 2]) -> [f64; 2] {
        let scale = self.scale();
        [(p[0] - self.center[0]) * scale, (p[1] - self.center[1]) * scale]
    }

    /// The world point `offset` points from the center of the view.
    pub fn to_world(&self, offset: [f64; 2]) -> [f64; 2] {
        let scale = self.scale();
        [self.center[0] + offset[0] / scale, self.center[1] + offset[1] / scale]
    }

    /// Moves the map by `delta` points, as dragging does.
    pub fn pan(&mut self, delta: [f64; 2]) {
        let scale = self.scale();
        self.center = [
            (self.center[0] - delta[0] / scale).clamp(0.0, 1.0),
            (self.center[1] - delta[1] / scale).clamp(0.0, 1.0),
        ];
    }

    /// Keeps a view `size` points large on the world: never smaller than the world is wide, never past its edges
    /// (REQ-044-02).
    pub fn fit(&mut self, size: [f64; 2]) {
        let fill = (size[0].max(1.0) / WORLD_POINTS).log2();
        self.zoom = self.zoom.clamp(fill.clamp(MIN_ZOOM, MAX_ZOOM), MAX_ZOOM);
        let scale = self.scale();
        for (c, side) in self.center.iter_mut().zip(size) {
            let half = (side / 2.0 / scale).min(0.5);
            *c = c.clamp(half, 1.0 - half);
        }
    }

    /// Zooms by `steps` levels around `at` (points from the center), so what lies under the pointer stays there.
    pub fn zoom_at(&mut self, steps: f64, at: [f64; 2]) {
        let under = self.to_world(at);
        self.zoom = (self.zoom + steps).clamp(MIN_ZOOM, MAX_ZOOM);
        let scale = self.scale();
        self.center = [under[0] - at[0] / scale, under[1] - at[1] / scale];
    }
}

/// Photos close together on screen, drawn as one (REQ-044-01).
#[derive(Debug, Clone, PartialEq)]
pub struct Cluster {
    /// The world point it is drawn at: the middle of its photos.
    pub center: [f64; 2],
    /// Its photos, newest first, as they came.
    pub ids: Vec<i64>,
    /// The longitude and latitude its photos span: west, south, east, north.
    pub bounds: [f64; 4],
}

/// The zoom level clusters are grouped at for a view: whole levels, so they keep their grouping while the map zooms
/// between two and only regroup on reaching the next (REQ-044-02).
pub fn cluster_level(view: &MapView) -> f64 {
    view.zoom.floor()
}

/// Groups `points` (id, longitude, latitude; newest first) into clusters at zoom `level`: what falls in the same
/// `spacing` × `spacing` points of its grid, then any two closer than `spacing` points to each other, merged until none
/// are. Clusters are drawn at most `spacing` wide, so they don't overlap at `level` and, as zooming in only moves them
/// apart, at any zoom of that level either (REQ-044-01, REQ-044-02).
pub fn clusters(points: &[(i64, f64, f64)], level: f64, spacing: f64) -> Vec<Cluster> {
    let size = WORLD_POINTS * level.exp2() / spacing;
    let cell_of = |p: [f64; 2]| ((p[0] * size).floor() as i64, (p[1] * size).floor() as i64);

    /// A cluster as it grows: the indexes of its points, the sum of their world points, its bounds.
    struct Group {
        members: Vec<usize>,
        sum: [f64; 2],
        bounds: [f64; 4],
    }
    impl Group {
        fn center(&self) -> [f64; 2] {
            let n = self.members.len() as f64;
            [self.sum[0] / n, self.sum[1] / n]
        }
        fn absorb(&mut self, other: Group) {
            self.members.extend(other.members);
            self.sum = [self.sum[0] + other.sum[0], self.sum[1] + other.sum[1]];
            let (a, b) = (&mut self.bounds, other.bounds);
            *a = [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])];
        }
    }

    // By grid cell first.
    let mut by_cell: std::collections::HashMap<(i64, i64), usize> = std::collections::HashMap::new();
    let mut groups: Vec<Group> = Vec::new();
    for (index, &(_, longitude, latitude)) in points.iter().enumerate() {
        let p = project(longitude, latitude);
        let at = *by_cell.entry(cell_of(p)).or_insert_with(|| {
            groups.push(Group {
                members: Vec::new(),
                sum: [0.0, 0.0],
                bounds: [longitude, latitude, longitude, latitude],
            });
            groups.len() - 1
        });
        groups[at].absorb(Group {
            members: vec![index],
            sum: p,
            bounds: [longitude, latitude, longitude, latitude],
        });
    }

    // Then neighbors that came out too close, until none are: a merged cluster's middle moves, which can bring it
    // close to another.
    let too_close = 1.0 / size;
    loop {
        let mut placed: std::collections::HashMap<(i64, i64), Vec<usize>> = std::collections::HashMap::new();
        let mut merged: Vec<Group> = Vec::with_capacity(groups.len());
        let mut changed = false;
        for group in groups {
            let center = group.center();
            let (x, y) = cell_of(center);
            let near = (x - 1..=x + 1)
                .flat_map(|cx| (y - 1..=y + 1).map(move |cy| (cx, cy)))
                .filter_map(|cell| placed.get(&cell))
                .flatten()
                .copied()
                .find(|&other| {
                    let c = merged[other].center();
                    (c[0] - center[0]).hypot(c[1] - center[1]) < too_close
                });
            match near {
                Some(other) => {
                    merged[other].absorb(group);
                    changed = true;
                }
                None => {
                    placed.entry((x, y)).or_default().push(merged.len());
                    merged.push(group);
                }
            }
        }
        groups = merged;
        if !changed {
            break;
        }
    }

    groups
        .into_iter()
        .map(|mut group| {
            group.members.sort_unstable();
            Cluster {
                center: group.center(),
                ids: group.members.iter().map(|&index| points[index].0).collect(),
                bounds: group.bounds,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn req_044_01_projects_onto_web_mercators_square() {
        assert_eq!(project(0.0, 0.0), [0.5, 0.5]);
        assert!(close(project(-180.0, 0.0)[0], 0.0) && close(project(180.0, 0.0)[0], 1.0));
        assert!(project(11.5, 48.1)[1] < 0.5, "north is up");
        let (lon, lat) = unproject(project(-70.66, -33.45));
        assert!(close(lon, -70.66) && close(lat, -33.45));
        assert!(close(project(0.0, 89.9)[1], 0.0), "clamped near the pole");
    }

    #[test]
    fn req_044_02_zooms_around_the_pointer_and_pans() {
        let mut view = MapView {
            center: [0.5, 0.5],
            zoom: 2.0,
        };
        assert_eq!(view.scale(), 1024.0);
        let munich = project(11.5, 48.1);
        let at = view.to_screen(munich);
        view.zoom_at(1.5, at);
        let after = view.to_screen(munich);
        assert!(
            close(after[0], at[0]) && close(after[1], at[1]),
            "Munich stays under the pointer"
        );
        assert!(close(view.zoom, 3.5));
        view.zoom_at(100.0, [0.0, 0.0]);
        assert_eq!(view.zoom, MAX_ZOOM);
        let before = view.center;
        view.pan([10.0, -20.0]);
        let moved = view.to_screen(before);
        assert!(
            close(moved[0], 10.0) && close(moved[1], -20.0),
            "the map follows the drag"
        );
        assert_eq!(
            view.to_world(view.to_screen(munich)).map(|c| (c * 1e9).round()),
            munich.map(|c| (c * 1e9).round())
        );
    }

    #[test]
    fn req_044_02_the_world_fills_the_view() {
        let mut view = MapView::default();
        view.fit([1024.0, 600.0]);
        assert!(close(view.zoom, 2.0), "1,024 points wide: the world at zoom 2");
        view.pan([10_000.0, 0.0]);
        view.fit([1024.0, 600.0]);
        assert!(
            close(view.to_screen([0.0, 0.5])[0], -512.0),
            "the west edge stays at the view's edge"
        );
        view.zoom_at(3.0, [0.0, 0.0]);
        view.fit([1024.0, 600.0]);
        assert!(close(view.zoom, 5.0), "zoomed in stays");
    }

    /// Photos spread over Europe in a fixed pseudo-random pattern, newest first.
    fn scattered(n: i64) -> Vec<(i64, f64, f64)> {
        let mut seed: u64 = 7;
        let mut next = move || {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        (0..n)
            .rev()
            .map(|id| (id, -10.0 + 40.0 * next(), 36.0 + 24.0 * next()))
            .collect()
    }

    /// REQ-044-02: while the map zooms between two whole levels, the clusters keep their photos; they only regroup on
    /// reaching the next level (#293).
    #[test]
    fn req_044_02_clusters_hold_still_while_zooming_within_a_level() {
        let points = scattered(400);
        let at = |zoom: f64| {
            let view = MapView {
                center: project(10.0, 48.0),
                zoom,
            };
            let mut groups: Vec<Vec<i64>> = clusters(&points, cluster_level(&view), 64.0)
                .into_iter()
                .map(|cluster| cluster.ids)
                .collect();
            groups.sort();
            groups
        };
        let level = at(4.0);
        for zoom in [4.1, 4.37, 4.5, 4.83, 4.999] {
            assert_eq!(at(zoom), level, "regrouped at zoom {zoom}");
        }
        assert!(at(5.0).len() > level.len(), "split on reaching the next level");
    }

    /// REQ-044-01, REQ-044-02: no two clusters come closer than their spacing on screen at any zoom, so they never
    /// overlap; each photo is in exactly one, and each lists its photos newest first (#296).
    #[test]
    fn req_044_01_req_044_02_clusters_never_overlap() {
        let points = scattered(600);
        for zoom in [1.0, 2.5, 3.0, 4.2, 5.7, 7.0, 9.9] {
            let view = MapView {
                center: project(10.0, 48.0),
                zoom,
            };
            let found = clusters(&points, cluster_level(&view), 64.0);
            let mut ids: Vec<i64> = found.iter().flat_map(|cluster| cluster.ids.clone()).collect();
            ids.sort_unstable();
            assert_eq!(ids, (0..600).collect::<Vec<_>>(), "zoom {zoom}");
            assert!(found.iter().all(|cluster| cluster.ids.windows(2).all(|w| w[0] > w[1])));
            let screen: Vec<[f64; 2]> = found.iter().map(|cluster| view.to_screen(cluster.center)).collect();
            for (i, a) in screen.iter().enumerate() {
                for b in &screen[i + 1..] {
                    let apart = (a[0] - b[0]).hypot(a[1] - b[1]);
                    assert!(apart >= 64.0 - 1e-6, "zoom {zoom}: two clusters {apart} points apart");
                }
            }
        }
    }

    #[test]
    fn req_044_01_req_044_02_clusters_split_as_the_map_zooms_in() {
        // Two photos in Munich, one in Berlin (newest first: Berlin, Munich, Munich).
        let points = [(3, 13.40, 52.52), (2, 11.58, 48.14), (1, 11.57, 48.13)];
        let world = MapView::default();
        let together = clusters(&points, cluster_level(&world), 64.0);
        assert_eq!(together.len(), 1, "one cluster for Germany seen from afar");
        assert_eq!(together[0].ids, [3, 2, 1], "newest first");
        let b = together[0].bounds;
        assert!(close(b[0], 11.57) && close(b[1], 48.13) && close(b[2], 13.40) && close(b[3], 52.52));

        let germany = MapView {
            center: project(12.0, 50.0),
            zoom: 6.0,
        };
        let mut apart = clusters(&points, cluster_level(&germany), 64.0);
        apart.sort_by_key(|cluster| cluster.ids.len());
        assert_eq!(apart.len(), 2);
        assert_eq!((apart[0].ids.clone(), apart[1].ids.clone()), (vec![3], vec![2, 1]));
        let streets = MapView {
            center: project(11.575, 48.135),
            zoom: 16.0,
        };
        assert_eq!(
            clusters(&points, cluster_level(&streets), 64.0).len(),
            3,
            "close up, each photo alone"
        );
    }
}
