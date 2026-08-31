//! Colors as they appear in recipes: `#rgb` or `#rrggbb` strings.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Parse `#rgb` or `#rrggbb` (the leading `#` is optional).
    pub fn parse(s: &str) -> Result<Self, ColorError> {
        let hex = s.strip_prefix('#').unwrap_or(s);
        let digits: Vec<u32> = hex
            .chars()
            .map(|c| c.to_digit(16).ok_or(ColorError::NotHex))
            .collect::<Result<_, _>>()?;
        match digits.len() {
            3 => Ok(Self::new(
                (digits[0] * 17) as u8,
                (digits[1] * 17) as u8,
                (digits[2] * 17) as u8,
            )),
            6 => Ok(Self::new(
                (digits[0] * 16 + digits[1]) as u8,
                (digits[2] * 16 + digits[3]) as u8,
                (digits[4] * 16 + digits[5]) as u8,
            )),
            _ => Err(ColorError::BadLength),
        }
    }

    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    /// Linear interpolation, `t` clamped to 0.0..=1.0.
    pub fn lerp(self, other: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        Self::new(
            mix(self.r, other.r),
            mix(self.g, other.g),
            mix(self.b, other.b),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorError {
    NotHex,
    BadLength,
}

impl std::fmt::Display for ColorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ColorError::NotHex => write!(f, "color contains a non-hex digit"),
            ColorError::BadLength => write!(f, "color must have 3 or 6 hex digits"),
        }
    }
}

impl std::error::Error for ColorError {}

impl Serialize for Rgb {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Rgb {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Rgb::parse(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_both_forms() {
        assert_eq!(Rgb::parse("#f80").unwrap(), Rgb::new(0xff, 0x88, 0x00));
        assert_eq!(Rgb::parse("ff8800").unwrap(), Rgb::new(0xff, 0x88, 0x00));
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(Rgb::parse("#12345"), Err(ColorError::BadLength));
        assert_eq!(Rgb::parse("#gggggg"), Err(ColorError::NotHex));
    }

    #[test]
    fn hex_round_trips() {
        let c = Rgb::new(1, 2, 3);
        assert_eq!(Rgb::parse(&c.to_hex()).unwrap(), c);
    }

    #[test]
    fn lerp_endpoints_and_clamp() {
        let a = Rgb::new(0, 0, 0);
        let b = Rgb::new(255, 255, 255);
        assert_eq!(a.lerp(b, 0.0), a);
        assert_eq!(a.lerp(b, 1.0), b);
        assert_eq!(a.lerp(b, 2.0), b);
        assert_eq!(a.lerp(b, 0.5), Rgb::new(128, 128, 128));
    }
}
