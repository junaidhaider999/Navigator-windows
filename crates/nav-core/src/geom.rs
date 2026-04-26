//! Geometry primitives in **physical screen pixels** (see `03-modules.md`).

/// An axis-aligned rectangle: top-left `(x, y)`, size `(w, h)`.
///
/// All values are in physical pixels after DPI conversion at the UIA / render boundary.
///
/// # Example
///
/// ```
/// use nav_core::Rect;
/// let r = Rect { x: 10, y: 20, w: 100, h: 40 };
/// assert_eq!(r.center(), (60, 40));
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    /// Returns the integer center `(cx, cy)` of the rectangle.
    #[must_use]
    pub fn center(self) -> (i32, i32) {
        (self.x + self.w / 2, self.y + self.h / 2)
    }

    /// Manhattan distance between the centers of two rectangles.
    #[must_use]
    pub fn manhattan_center(self, other: Rect) -> i32 {
        let (ax, ay) = self.center();
        let (bx, by) = other.center();
        (ax - bx).abs() + (ay - by).abs()
    }
}
