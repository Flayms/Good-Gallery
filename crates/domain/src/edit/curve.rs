//! Tone curves and their channels (REQ-021-04).

use super::*;

/// One tone curve: points from black to white, sorted by input (REQ-021-04).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Curve {
    pub points: Vec<[f32; 2]>,
}

impl Default for Curve {
    fn default() -> Self {
        Self {
            points: vec![[0.0, 0.0], [1.0, 1.0]],
        }
    }
}

impl Curve {
    /// A straight line from black to white.
    pub fn is_identity(&self) -> bool {
        self.points.iter().all(|[x, y]| (x - y).abs() < 1e-6)
    }

    /// Endpoints at 0 and 1, values in 0–1, sorted, at most [`MAX_CURVE_POINTS`] (recipes from files may be hostile).
    pub(super) fn sanitize(&mut self) {
        let mut points: Vec<[f32; 2]> = self
            .points
            .iter()
            .filter(|[x, y]| x.is_finite() && y.is_finite())
            .map(|[x, y]| [x.clamp(0.0, 1.0), y.clamp(0.0, 1.0)])
            .take(MAX_CURVE_POINTS)
            .collect();
        points.sort_by(|a, b| a[0].total_cmp(&b[0]));
        points.dedup_by(|a, b| (a[0] - b[0]).abs() < 1e-4);
        if points.len() < 2 {
            points = Self::default().points;
        }
        self.points = points;
    }

    /// The curve at `x`: a monotone cubic through the points (Fritsch–Carlson), so it never overshoots them; flat
    /// beyond the endpoints.
    pub fn eval(&self, x: f32) -> f32 {
        let p = &self.points;
        let n = p.len();
        if n == 0 {
            return x;
        }
        if x <= p[0][0] {
            return p[0][1];
        }
        if x >= p[n - 1][0] {
            return p[n - 1][1];
        }
        let slopes = self.tangents();
        let i = p.windows(2).position(|w| x < w[1][0]).unwrap_or(n - 2);
        let ([x0, y0], [x1, y1]) = (p[i], p[i + 1]);
        let h = x1 - x0;
        let t = (x - x0) / h;
        let (t2, t3) = (t * t, t * t * t);
        let y = (2.0 * t3 - 3.0 * t2 + 1.0) * y0
            + (t3 - 2.0 * t2 + t) * h * slopes[i]
            + (-2.0 * t3 + 3.0 * t2) * y1
            + (t3 - t2) * h * slopes[i + 1];
        y.clamp(0.0, 1.0)
    }

    fn tangents(&self) -> Vec<f32> {
        let p = &self.points;
        let n = p.len();
        let secants: Vec<f32> = p
            .windows(2)
            .map(|w| (w[1][1] - w[0][1]) / (w[1][0] - w[0][0]))
            .collect();
        let mut m = vec![0.0; n];
        m[0] = secants[0];
        m[n - 1] = secants[n - 2];
        for i in 1..n - 1 {
            m[i] = if secants[i - 1] * secants[i] <= 0.0 {
                0.0
            } else {
                (secants[i - 1] + secants[i]) / 2.0
            };
        }
        for i in 0..n - 1 {
            if secants[i] == 0.0 {
                m[i] = 0.0;
                m[i + 1] = 0.0;
                continue;
            }
            let (a, b) = (m[i] / secants[i], m[i + 1] / secants[i]);
            let s = a * a + b * b;
            if s > 9.0 {
                let tau = 3.0 / s.sqrt();
                m[i] = tau * a * secants[i];
                m[i + 1] = tau * b * secants[i];
            }
        }
        m
    }

    /// Adds a point at `x` (if there is room and none is that close) and returns its index.
    pub fn add_point(&mut self, x: f32, y: f32) -> Option<usize> {
        let (x, y) = (x.clamp(0.0, 1.0), y.clamp(0.0, 1.0));
        if self.points.len() >= MAX_CURVE_POINTS || self.points.iter().any(|p| (p[0] - x).abs() < 0.02) {
            return None;
        }
        let at = self.points.iter().position(|p| p[0] > x).unwrap_or(self.points.len());
        self.points.insert(at, [x, y]);
        Some(at)
    }

    /// Moves point `i` to `(x, y)`, kept between its neighbours and inside 0–1.
    pub fn move_point(&mut self, i: usize, x: f32, y: f32) {
        let n = self.points.len();
        if i >= n {
            return;
        }
        let low = if i == 0 { 0.0 } else { self.points[i - 1][0] + 0.01 };
        let high = if i + 1 == n { 1.0 } else { self.points[i + 1][0] - 0.01 };
        self.points[i] = [x.clamp(low, high.max(low)), y.clamp(0.0, 1.0)];
    }

    /// Removes point `i`, unless it is one of the endpoints.
    pub fn remove_point(&mut self, i: usize) -> bool {
        if i == 0 || i + 1 >= self.points.len() {
            return false;
        }
        self.points.remove(i);
        true
    }
}

/// Which tone curve: all channels together, or one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    All,
    Red,
    Green,
    Blue,
}

impl Channel {
    pub const ALL: [Channel; 4] = [Self::All, Self::Red, Self::Green, Self::Blue];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "RGB",
            Self::Red => "Red",
            Self::Green => "Green",
            Self::Blue => "Blue",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Curves {
    pub all: Curve,
    pub red: Curve,
    pub green: Curve,
    pub blue: Curve,
}

impl Curves {
    pub fn get(&self, channel: Channel) -> &Curve {
        match channel {
            Channel::All => &self.all,
            Channel::Red => &self.red,
            Channel::Green => &self.green,
            Channel::Blue => &self.blue,
        }
    }

    pub fn get_mut(&mut self, channel: Channel) -> &mut Curve {
        match channel {
            Channel::All => &mut self.all,
            Channel::Red => &mut self.red,
            Channel::Green => &mut self.green,
            Channel::Blue => &mut self.blue,
        }
    }

    pub fn is_identity(&self) -> bool {
        Channel::ALL.iter().all(|&channel| self.get(channel).is_identity())
    }
}
