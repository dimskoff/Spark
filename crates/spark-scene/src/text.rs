//! Text forms of numbers, strings and colors for the canonical writer, and the matching readers.
//!
//! Everything here is deterministic: the same value always prints as the same bytes.

/// Shortest text that reads back as the same `f32`, no trailing `.0`, `-0` printed as `0`, exponent form only for very
/// large or very small numbers (`1.5e20`, `1e-7`).
pub(crate) fn f32_compact(x: f32) -> String {
    if x == 0.0 {
        return "0".into();
    }
    if !x.is_finite() {
        return if x.is_nan() {
            "nan".into()
        } else if x > 0.0 {
            "inf".into()
        } else {
            "-inf".into()
        };
    }
    let a = x.abs();
    if !(1e-5..1e16).contains(&a) { format!("{x:e}") } else { format!("{x}") }
}

/// Like [`f32_compact`] but always a TOML float (`1.0`, `12.0`, `0.6`).
pub(crate) fn f32_scalar(x: f32) -> String {
    float_marker(f32_compact(x))
}

/// A float that is not known to come from an `f32` (free-form data such as script props): the `f32` form when the
/// value is exactly an `f32`, else the shortest `f64` text.
pub(crate) fn f64_scalar(x: f64) -> String {
    if (x as f32) as f64 == x {
        return f32_scalar(x as f32);
    }
    if !x.is_finite() {
        return f32_compact(x as f32);
    }
    let a = x.abs();
    float_marker(if !(1e-5..1e16).contains(&a) { format!("{x:e}") } else { format!("{x}") })
}

fn float_marker(mut s: String) -> String {
    if !s.contains(['.', 'e', 'n', 'i']) {
        s.push_str(".0");
    }
    s
}

/// The fewest-decimals degrees value that converts back to exactly `rad` (a spot angle of `50` degrees is stored as
/// 0.87266463 radians and must print as `50`, not `50.000004`).
pub(crate) fn radians_to_degrees(rad: f32) -> f32 {
    let d = rad.to_degrees();
    for decimals in 0..=9 {
        let k = 10f64.powi(decimals);
        let r = ((f64::from(d) * k).round() / k) as f32;
        if r.to_radians() == rad {
            return if r == 0.0 { 0.0 } else { r };
        }
    }
    d
}

/// A basic TOML string: `"text"` with the escapes TOML requires.
pub(crate) fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A key: bare when it only has `A-Za-z0-9_-`, quoted otherwise.
pub(crate) fn key(s: &str) -> String {
    if !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') { s.to_string() } else { quote(s) }
}

/// `"#rrggbb"` / `"#rrggbbaa"` when every channel is exactly a byte divided by 255 (what `Color::hex` and the reader
/// produce), else `[r, g, b, a]`.
pub(crate) fn color_text(c: [f32; 4]) -> String {
    let byte = |v: f32| -> Option<u8> {
        let b = (v * 255.0).round();
        ((0.0..=255.0).contains(&b) && (b as u8) as f32 / 255.0 == v).then_some(b as u8)
    };
    match (byte(c[0]), byte(c[1]), byte(c[2]), byte(c[3])) {
        (Some(r), Some(g), Some(b), Some(255)) => format!("\"#{r:02x}{g:02x}{b:02x}\""),
        (Some(r), Some(g), Some(b), Some(a)) => format!("\"#{r:02x}{g:02x}{b:02x}{a:02x}\""),
        _ => array(&c),
    }
}

/// `[a, b, c]` with compact numbers.
pub(crate) fn array(v: &[f32]) -> String {
    let items: Vec<String> = v.iter().map(|x| f32_compact(*x)).collect();
    format!("[{}]", items.join(", "))
}

/// `#rrggbb` or `#rrggbbaa` (any case) -> `[r, g, b, a]`, computed exactly like `Color::hex`.
pub(crate) fn parse_hex_color(s: &str) -> Option<[f32; 4]> {
    let hex = s.strip_prefix('#')?;
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) || !matches!(hex.len(), 6 | 8) {
        return None;
    }
    let v = u32::from_str_radix(hex, 16).ok()?;
    let (rgb, a) = if hex.len() == 8 { (v >> 8, (v & 0xff) as f32 / 255.0) } else { (v, 1.0) };
    let ch = |shift: u32| ((rgb >> shift) & 0xff) as f32 / 255.0;
    Some([ch(16), ch(8), ch(0), a])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers() {
        assert_eq!(f32_compact(3.0), "3");
        assert_eq!(f32_compact(-0.0), "0");
        assert_eq!(f32_compact(0.1), "0.1");
        assert_eq!(f32_compact(1e-7), "1e-7");
        assert_eq!(f32_compact(1.5e20), "1.5e20");
        assert_eq!(f32_scalar(12.0), "12.0");
        assert_eq!(f32_scalar(0.6), "0.6");
        assert_eq!(f32_scalar(1e20), "1e20");
        assert_eq!(f64_scalar(0.5), "0.5");
        assert_eq!(f64_scalar(0.1), "0.1");
        assert_eq!(f64_scalar(1.0 / 3.0), "0.3333333333333333");
    }

    #[test]
    fn angles() {
        for d in [1.0f32, 45.0, 50.0, 90.0, 120.5, 179.0] {
            let rad = d.to_radians();
            assert_eq!(radians_to_degrees(rad), d, "{d}");
        }
    }

    #[test]
    fn strings_and_keys() {
        assert_eq!(quote("a\"b\\c\n"), "\"a\\\"b\\\\c\\n\"");
        assert_eq!(quote("\u{1}"), "\"\\u0001\"");
        assert_eq!(key("open_angle"), "open_angle");
        assert_eq!(key("leaf/material.color"), "\"leaf/material.color\"");
        assert_eq!(key(""), "\"\"");
    }

    #[test]
    fn colors() {
        assert_eq!(color_text(parse_hex_color("#ffe9c4").unwrap()), "\"#ffe9c4\"");
        assert_eq!(color_text(parse_hex_color("#FFE9C480").unwrap()), "\"#ffe9c480\"");
        assert_eq!(color_text([0.1, 0.2, 0.3, 1.0]), "[0.1, 0.2, 0.3, 1]");
        assert_eq!(parse_hex_color("#fff"), None, "short forms are not written, so they are not read");
        assert_eq!(parse_hex_color("#+12345"), None);
    }
}
