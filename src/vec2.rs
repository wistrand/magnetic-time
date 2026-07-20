//! Minimal 2-vectors for simulation code (egui's Vec2 is f32 and UI-flavored).
//! Two scalar widths: `Vec2` (f64) for the "world" — magnet field math, clock
//! time, pixel mapping, where precision and dynamic range matter — and `Vec2f`
//! (f32) for per-particle state, halving the memory the neighbor pass gathers
//! (the sim is bandwidth-bound; see agent_docs/gotchas.md). Convert at the
//! boundary with `to_f32` / `to_f64`.

use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub};

macro_rules! vec2_type {
    ($name:ident, $t:ty) => {
        #[derive(Clone, Copy, Debug, Default, PartialEq)]
        pub struct $name {
            pub x: $t,
            pub y: $t,
        }

        impl $name {
            pub const ZERO: $name = $name { x: 0.0, y: 0.0 };

            pub fn new(x: $t, y: $t) -> Self {
                Self { x, y }
            }

            pub fn dot(self, o: $name) -> $t {
                self.x * o.x + self.y * o.y
            }

            pub fn len_sq(self) -> $t {
                self.dot(self)
            }

            pub fn len(self) -> $t {
                self.len_sq().sqrt()
            }

            /// Zero vector stays zero.
            pub fn normalized(self) -> $name {
                let l = self.len();
                if l > 1e-12 {
                    self / l
                } else {
                    $name::ZERO
                }
            }
        }

        impl Add for $name {
            type Output = $name;
            fn add(self, o: $name) -> $name {
                $name::new(self.x + o.x, self.y + o.y)
            }
        }

        impl AddAssign for $name {
            fn add_assign(&mut self, o: $name) {
                *self = *self + o;
            }
        }

        impl Sub for $name {
            type Output = $name;
            fn sub(self, o: $name) -> $name {
                $name::new(self.x - o.x, self.y - o.y)
            }
        }

        impl Mul<$t> for $name {
            type Output = $name;
            fn mul(self, s: $t) -> $name {
                $name::new(self.x * s, self.y * s)
            }
        }

        impl Div<$t> for $name {
            type Output = $name;
            fn div(self, s: $t) -> $name {
                $name::new(self.x / s, self.y / s)
            }
        }

        impl Neg for $name {
            type Output = $name;
            fn neg(self) -> $name {
                $name::new(-self.x, -self.y)
            }
        }
    };
}

vec2_type!(Vec2, f64);
vec2_type!(Vec2f, f32);

impl Vec2 {
    /// Narrow to f32 particle space.
    pub fn to_f32(self) -> Vec2f {
        Vec2f::new(self.x as f32, self.y as f32)
    }
}

impl Vec2f {
    /// Widen to f64 world space (for field queries and pixel mapping).
    pub fn to_f64(self) -> Vec2 {
        Vec2::new(self.x as f64, self.y as f64)
    }
}
