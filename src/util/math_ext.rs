#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Vec2i {
    pub x: i32,
    pub y: i32,
}

impl Vec2i {
    pub fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    pub fn zero() -> Self {
        Self { x: 0, y: 0 }
    }

    pub fn one() -> Self {
        Self { x: 1, y: 1 }
    }

    pub fn dot(self, other: Self) -> i32 {
        self.x * other.x + self.y * other.y
    }

    pub fn length_squared(self) -> i32 {
        self.x * self.x + self.y * self.y
    }

    pub fn length(self) -> f32 {
        (self.length_squared() as f32).sqrt()
    }
}

#[inline(always)]
pub const fn vec2i(x: i32, y: i32) -> Vec2i {
    Vec2i { x, y }
}

impl std::ops::Add for Vec2i {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
        }
    }
}

impl std::ops::Sub for Vec2i {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
        }
    }
}

impl std::ops::Mul<i32> for Vec2i {
    type Output = Self;

    fn mul(self, rhs: i32) -> Self::Output {
        Self {
            x: self.x * rhs,
            y: self.y * rhs,
        }
    }
}

impl std::ops::Div<i32> for Vec2i {
    type Output = Self;

    fn div(self, rhs: i32) -> Self::Output {
        Self {
            x: self.x / rhs,
            y: self.y / rhs,
        }
    }
}

impl From<(i32, i32)> for Vec2i {
    fn from((x, y): (i32, i32)) -> Self {
        Self::new(x, y)
    }
}

impl Into<(i32, i32)> for Vec2i {
    fn into(self) -> (i32, i32) {
        (self.x, self.y)
    }
}