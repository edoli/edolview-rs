use std::ops::Range;

use eframe::egui::Pos2;

use crate::util::math_ext::{vec2i, Vec2i};

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct Recti {
    pub min: Vec2i,
    pub max: Vec2i,
}

impl Recti {
    pub const ZERO: Self = Self {
        min: Vec2i::ZERO,
        max: Vec2i::ZERO,
    };

    #[inline(always)]
    pub const fn from_min_max(min: Vec2i, max: Vec2i) -> Self {
        Self { min, max }
    }

    /// left-top corner plus a size (stretching right-down).
    #[inline(always)]
    pub fn from_min_size(min: Vec2i, size: Vec2i) -> Self {
        Self { min, max: min + size }
    }

    #[inline(always)]
    pub fn from_x_y_ranges(x_range: Range<i32>, y_range: Range<i32>) -> Self {
        Self {
            min: vec2i(x_range.start, y_range.start),
            max: vec2i(x_range.end, y_range.end),
        }
    }

    /// Returns the bounding rectangle of the two points.
    #[inline]
    pub fn from_two_pos(a: Vec2i, b: Vec2i) -> Self {
        Self {
            min: vec2i(a.x.min(b.x), a.y.min(b.y)),
            max: vec2i(a.x.max(b.x), a.y.max(b.y)),
        }
    }

    /// Returns the rectangle of minimum size that contains both positions.
    #[inline]
    pub fn bound_two_pos(a: Pos2, b: Pos2) -> Self {
        Self {
            min: vec2i(a.x.min(b.x).floor() as i32, a.y.min(b.y).floor() as i32),
            max: vec2i(a.x.max(b.x).ceil() as i32, a.y.max(b.y).ceil() as i32),
        }
    }

    /// A zero-sized rect at a specific point.
    #[inline]
    pub fn from_pos(point: Vec2i) -> Self {
        Self { min: point, max: point }
    }

    #[must_use]
    #[inline]
    pub fn intersects(self, other: Self) -> bool {
        self.min.x <= other.max.x && other.min.x <= self.max.x && self.min.y <= other.max.y && other.min.y <= self.max.y
    }

        #[inline(always)]
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        Self {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }

    #[inline]
    #[must_use]
    pub fn intersect(self, other: Self) -> Self {
        Self {
            min: self.min.max(other.min),
            max: self.max.min(other.max),
        }
    }

        #[inline(always)]
    pub fn size(&self) -> Vec2i {
        self.max - self.min
    }

    #[inline(always)]
    pub fn width(&self) -> i32 {
        self.max.x - self.min.x
    }

    #[inline(always)]
    pub fn height(&self) -> i32 {
        self.max.y - self.min.y
    }
    
    #[inline(always)]
    pub fn area(&self) -> i32 {
        self.width() * self.height()
    }

    pub fn aspect_ratio(&self) -> f32 {
        self.width() as f32 / self.height() as f32
    }

    pub fn to_rect(&self) -> eframe::egui::Rect {
        eframe::egui::Rect {
            min: Pos2::new(self.min.x as f32, self.min.y as f32),
            max: Pos2::new(self.max.x as f32, self.max.y as f32),
        }
    }

    pub fn to_cv_rect(&self) -> opencv::core::Rect {
        opencv::core::Rect {
            x: self.min.x,
            y: self.min.y,
            width: self.width(),
            height: self.height(),
        }
    }
}

impl std::str::FromStr for Recti {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();

        // Helper to parse i32 with trimming
        fn parse_i32(t: &str) -> Result<i32, ()> {
            t.trim().parse::<i32>().map_err(|_| ())
        }

        // Format 1: [y_min:y_max, x_min:x_max]
        if s.contains(':') {
            // Be permissive about brackets/spaces
            let inner = s.trim_matches(|c: char| c == '[' || c == ']' || c.is_whitespace());
            let mut parts = inner.split(',');
            let y_part = parts.next().ok_or(())?.trim();
            let x_part = parts.next().ok_or(())?.trim();
            // Ensure there are not extra parts
            if parts.next().is_some() {
                return Err(());
            }

            let mut y_vals = y_part.split(':');
            let y0 = parse_i32(y_vals.next().ok_or(())?)?;
            let y1 = parse_i32(y_vals.next().ok_or(())?)?;
            if y_vals.next().is_some() {
                return Err(());
            }

            let mut x_vals = x_part.split(':');
            let x0 = parse_i32(x_vals.next().ok_or(())?)?;
            let x1 = parse_i32(x_vals.next().ok_or(())?)?;
            if x_vals.next().is_some() {
                return Err(());
            }

            let min = vec2i(x0.min(x1), y0.min(y1));
            let max = vec2i(x0.max(x1), y0.max(y1));
            return Ok(Recti::from_min_max(min, max));
        }

        // Format 2: (x, y, width, height) — parentheses optional
    let inner = s.trim_matches(|c: char| c == '(' || c == ')' || c == '[' || c == ']' || c.is_whitespace());
        let parts: Vec<&str> = inner
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|t| !t.is_empty())
            .collect();

        if parts.len() != 4 {
            return Err(());
        }

        let x = parse_i32(parts[0])?;
        let y = parse_i32(parts[1])?;
        let w = parse_i32(parts[2])?;
        let h = parse_i32(parts[3])?;

        let x0 = x;
        let y0 = y;
        let x1 = x.saturating_add(w);
        let y1 = y.saturating_add(h);

        let min = vec2i(x0.min(x1), y0.min(y1));
        let max = vec2i(x0.max(x1), y0.max(y1));
        Ok(Recti::from_min_max(min, max))
    }
}

impl std::fmt::Display for Recti {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let x = self.min.x;
        let y = self.min.y;
        let width = self.max.x - self.min.x;
        let height = self.max.y - self.min.y;
        write!(f, "({}, {}, {}, {})", x, y, width, height)
    }
}
