use crate::{ErrorKind, MAX_ZOOM, MIN_ZOOM, RefscapeError};
use std::ops::Deref;

macro_rules! coordinate {
    ($name:ident) => {
        #[derive(Debug, Default, Clone, Copy, PartialEq)]
        pub struct $name(Point);
        impl $name {
            pub fn new(x: f32, y: f32) -> Result<Self, RefscapeError> {
                Self::try_from(Point::new(x, y))
            }
            pub const fn point(self) -> Point {
                self.0
            }
            pub const fn get(self) -> Point {
                self.0
            }
            pub fn translated(self, offset: Point) -> Result<Self, RefscapeError> {
                Self::new(self.x + offset.x, self.y + offset.y)
            }
        }
        impl Deref for $name {
            type Target = Point;
            fn deref(&self) -> &Point {
                &self.0
            }
        }
        impl From<$name> for Point {
            fn from(point: $name) -> Self {
                point.0
            }
        }
        impl TryFrom<Point> for $name {
            type Error = RefscapeError;
            fn try_from(point: Point) -> Result<Self, Self::Error> {
                if point.is_finite() {
                    Ok(Self(point))
                } else {
                    Err(RefscapeError::new(
                        ErrorKind::InvalidData,
                        "Coordinates must be finite",
                    ))
                }
            }
        }
        impl PartialEq<Point> for $name {
            fn eq(&self, other: &Point) -> bool {
                self.0 == *other
            }
        }
        impl PartialEq<$name> for Point {
            fn eq(&self, other: &$name) -> bool {
                *self == other.0
            }
        }
    };
}
coordinate!(WorldPoint);
coordinate!(ScreenPoint);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldSize {
    width: f32,
    height: f32,
}
impl WorldSize {
    pub fn new(width: f32, height: f32) -> Result<Self, RefscapeError> {
        if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
            return Err(RefscapeError::new(
                ErrorKind::InvalidData,
                "Dimensions must be finite and positive",
            ));
        }
        Ok(Self { width, height })
    }
    pub const fn width(self) -> f32 {
        self.width
    }
    pub const fn height(self) -> f32 {
        self.height
    }
    pub fn validate_at(self, position: WorldPoint) -> Result<(), RefscapeError> {
        let right = position.x + self.width;
        let bottom = position.y + self.height;
        if !right.is_finite() || !bottom.is_finite() || right <= position.x || bottom <= position.y
        {
            return Err(RefscapeError::new(
                ErrorKind::InvalidData,
                "Card edges must be finite and representable",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    pub offset: ScreenPoint,
    pub zoom: f32,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            offset: ScreenPoint::default(),
            zoom: 1.0,
        }
    }
}

impl Viewport {
    pub fn world_to_screen(self, point: impl Into<Point>) -> Point {
        let point = point.into();
        Point::new(
            point.x * self.zoom + self.offset.x,
            point.y * self.zoom + self.offset.y,
        )
    }

    pub fn screen_to_world(self, point: impl Into<Point>) -> Point {
        let point = point.into();
        Point::new(
            (point.x - self.offset.x) / self.zoom,
            (point.y - self.offset.y) / self.zoom,
        )
    }

    pub fn project_world(self, point: WorldPoint) -> Result<ScreenPoint, RefscapeError> {
        self.validate()
            .map_err(|error| RefscapeError::new(ErrorKind::InvalidData, error))?;
        self.world_to_screen(point).try_into()
    }

    pub fn unproject_screen(self, point: ScreenPoint) -> Result<WorldPoint, RefscapeError> {
        self.validate()
            .map_err(|error| RefscapeError::new(ErrorKind::InvalidData, error))?;
        self.screen_to_world(point).try_into()
    }

    pub fn validate(self) -> Result<(), String> {
        if !self.offset.is_finite()
            || !self.zoom.is_finite()
            || !(MIN_ZOOM..=MAX_ZOOM).contains(&self.zoom)
        {
            return Err("Viewport must have finite coordinates and zoom between 0.15 and 3".into());
        }
        Ok(())
    }
}
