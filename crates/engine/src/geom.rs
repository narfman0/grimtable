//! Flat 2D geometry in inches. The board origin is the top-left corner, x
//! runs along the long edge, y along the short edge.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const fn new(x: f32, y: f32) -> Self {
        Vec2 { x, y }
    }

    pub fn dist(self, other: Vec2) -> f32 {
        ((self.x - other.x).powi(2) + (self.y - other.y).powi(2)).sqrt()
    }

    pub fn sub(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x - o.x, self.y - o.y)
    }

    fn cross(self, o: Vec2) -> f32 {
        self.x * o.y - self.y * o.x
    }
}

/// Millimetres to a base radius in inches.
pub fn base_radius_in(mm: u32) -> f32 {
    mm as f32 / 25.4 / 2.0
}

/// Edge-to-edge distance between two round bases. Never negative.
pub fn base_distance(a: Vec2, ra: f32, b: Vec2, rb: f32) -> f32 {
    (a.dist(b) - ra - rb).max(0.0)
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Polygon {
    pub points: Vec<Vec2>,
}

impl Polygon {
    pub fn rect(x: f32, y: f32, w: f32, h: f32) -> Self {
        Polygon { points: vec![Vec2::new(x, y), Vec2::new(x + w, y), Vec2::new(x + w, y + h), Vec2::new(x, y + h)] }
    }

    /// Even-odd point-in-polygon.
    pub fn contains(&self, p: Vec2) -> bool {
        let n = self.points.len();
        if n < 3 {
            return false;
        }
        let mut inside = false;
        let mut j = n - 1;
        for i in 0..n {
            let (a, b) = (self.points[i], self.points[j]);
            if (a.y > p.y) != (b.y > p.y) {
                let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
                if p.x < x {
                    inside = !inside;
                }
            }
            j = i;
        }
        inside
    }

    pub fn edges(&self) -> impl Iterator<Item = (Vec2, Vec2)> + '_ {
        let n = self.points.len();
        (0..n).map(move |i| (self.points[i], self.points[(i + 1) % n]))
    }

    /// Does the segment a-b cross any edge of this polygon?
    pub fn crosses(&self, a: Vec2, b: Vec2) -> bool {
        self.edges().any(|(c, d)| segments_intersect(a, b, c, d))
    }
}

/// Proper or touching intersection of segments ab and cd.
pub fn segments_intersect(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> bool {
    let d1 = d.sub(c).cross(a.sub(c));
    let d2 = d.sub(c).cross(b.sub(c));
    let d3 = b.sub(a).cross(c.sub(a));
    let d4 = b.sub(a).cross(d.sub(a));
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0)) && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0)) {
        return true;
    }
    let on = |p: Vec2, q: Vec2, r: Vec2| -> bool {
        (q.sub(p).cross(r.sub(p))).abs() < 1e-6
            && r.x >= p.x.min(q.x) - 1e-6
            && r.x <= p.x.max(q.x) + 1e-6
            && r.y >= p.y.min(q.y) - 1e-6
            && r.y <= p.y.max(q.y) + 1e-6
    };
    on(a, b, c) || on(a, b, d) || on(c, d, a) || on(c, d, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_in_rect() {
        let r = Polygon::rect(10.0, 10.0, 5.0, 5.0);
        assert!(r.contains(Vec2::new(12.0, 12.0)));
        assert!(!r.contains(Vec2::new(9.0, 12.0)));
        assert!(!r.contains(Vec2::new(12.0, 16.0)));
    }

    #[test]
    fn segment_crossing() {
        let r = Polygon::rect(10.0, 10.0, 5.0, 5.0);
        assert!(r.crosses(Vec2::new(0.0, 12.0), Vec2::new(20.0, 12.0)));
        assert!(!r.crosses(Vec2::new(0.0, 20.0), Vec2::new(20.0, 20.0)));
    }

    #[test]
    fn base_math() {
        assert!((base_radius_in(25) - 0.492).abs() < 0.01);
        let d = base_distance(Vec2::new(0.0, 0.0), 0.5, Vec2::new(3.0, 0.0), 0.5);
        assert!((d - 2.0).abs() < 1e-6);
    }
}
