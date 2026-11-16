//! Sums typed into the search field: `12 * (3 + 4)`, `2^10`, `sqrt 2`, `15% * 80`.
//! A small recursive descent parser; nothing is evaluated that isn't arithmetic.

pub struct Answer {
    /// The number as it's shown, with thousands separated: `1,048,576`.
    pub shown: String,
    /// The number as it's copied: `1048576`.
    pub plain: String,
}

/// The answer to `input`, if it's a sum or a conversion. A lone number isn't one, so typing `42`
/// still searches.
pub fn evaluate(input: &str) -> Option<Answer> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    if let Some(answer) = convert(input) {
        return Some(answer);
    }
    if !looks_like_sum(input) {
        return None;
    }
    let value = arithmetic(input)?;
    Some(Answer { shown: format(value, true), plain: format(value, false) })
}

fn arithmetic(input: &str) -> Option<f64> {
    let mut parser = Parser { src: input.as_bytes(), pos: 0 };
    let value = parser.expr()?;
    parser.space();
    (parser.pos == parser.src.len() && value.is_finite()).then_some(value)
}

#[derive(Clone, Copy, PartialEq)]
enum Measure {
    Length,
    Mass,
    Volume,
    Area,
    Time,
    Data,
    Speed,
    Temperature,
}

/// A unit: the names it's typed as (the first is how it's shown), what it measures, and how many
/// of the measure's base unit one of it is. Temperature's base is kelvin, with an offset.
struct Unit {
    names: &'static [&'static str],
    measure: Measure,
    factor: f64,
    offset: f64,
}

const fn unit(names: &'static [&'static str], measure: Measure, factor: f64) -> Unit {
    Unit { names, measure, factor, offset: 0.0 }
}

use Measure::*;
const UNITS: &[Unit] = &[
    unit(&["mm", "millimeter", "millimeters", "millimetre", "millimetres"], Length, 0.001),
    unit(&["cm", "centimeter", "centimeters", "centimetre", "centimetres"], Length, 0.01),
    unit(&["m", "meter", "meters", "metre", "metres"], Length, 1.0),
    unit(&["km", "kilometer", "kilometers", "kilometre", "kilometres"], Length, 1000.0),
    unit(&["in", "inch", "inches", "\""], Length, 0.0254),
    unit(&["ft", "foot", "feet", "'"], Length, 0.3048),
    unit(&["yd", "yard", "yards"], Length, 0.9144),
    unit(&["mi", "mile", "miles"], Length, 1609.344),
    unit(&["nmi", "nautical mile", "nautical miles"], Length, 1852.0),
    unit(&["mg", "milligram", "milligrams"], Mass, 1e-6),
    unit(&["g", "gram", "grams"], Mass, 0.001),
    unit(&["kg", "kilogram", "kilograms", "kilo", "kilos"], Mass, 1.0),
    unit(&["t", "tonne", "tonnes"], Mass, 1000.0),
    unit(&["oz", "ounce", "ounces"], Mass, 0.028349523125),
    unit(&["lb", "lbs", "pound", "pounds"], Mass, 0.45359237),
    unit(&["st", "stone", "stones"], Mass, 6.35029318),
    unit(&["ml", "milliliter", "milliliters", "millilitre", "millilitres"], Volume, 0.001),
    unit(&["cl", "centiliter", "centiliters", "centilitre", "centilitres"], Volume, 0.01),
    unit(&["l", "liter", "liters", "litre", "litres"], Volume, 1.0),
    unit(&["tsp", "teaspoon", "teaspoons"], Volume, 0.00492892159375),
    unit(&["tbsp", "tablespoon", "tablespoons"], Volume, 0.01478676478125),
    unit(&["floz", "fl oz", "fluid ounce", "fluid ounces"], Volume, 0.0295735295625),
    unit(&["cup", "cups"], Volume, 0.2365882365),
    unit(&["pt", "pint", "pints"], Volume, 0.473176473),
    unit(&["gal", "gallon", "gallons"], Volume, 3.785411784),
    unit(&["m2", "m²", "square meter", "square meters", "square metre", "square metres", "sqm"], Area, 1.0),
    unit(&["km2", "km²", "square kilometer", "square kilometers", "square kilometre", "square kilometres"], Area, 1e6),
    unit(&["ft2", "ft²", "square foot", "square feet", "sqft"], Area, 0.09290304),
    unit(&["ha", "hectare", "hectares"], Area, 10_000.0),
    unit(&["acre", "acres"], Area, 4046.8564224),
    unit(&["ms", "millisecond", "milliseconds"], Time, 0.001),
    unit(&["s", "sec", "secs", "second", "seconds"], Time, 1.0),
    unit(&["min", "mins", "minute", "minutes"], Time, 60.0),
    unit(&["h", "hr", "hrs", "hour", "hours"], Time, 3600.0),
    unit(&["d", "day", "days"], Time, 86_400.0),
    unit(&["wk", "week", "weeks"], Time, 604_800.0),
    unit(&["yr", "year", "years"], Time, 31_557_600.0),
    unit(&["bit", "bits"], Data, 0.125),
    unit(&["B", "byte", "bytes"], Data, 1.0),
    unit(&["KB", "kb", "kilobyte", "kilobytes"], Data, 1e3),
    unit(&["MB", "mb", "megabyte", "megabytes"], Data, 1e6),
    unit(&["GB", "gb", "gigabyte", "gigabytes"], Data, 1e9),
    unit(&["TB", "tb", "terabyte", "terabytes"], Data, 1e12),
    unit(&["KiB", "kib", "kibibyte", "kibibytes"], Data, 1024.0),
    unit(&["MiB", "mib", "mebibyte", "mebibytes"], Data, 1_048_576.0),
    unit(&["GiB", "gib", "gibibyte", "gibibytes"], Data, 1_073_741_824.0),
    unit(&["TiB", "tib", "tebibyte", "tebibytes"], Data, 1_099_511_627_776.0),
    unit(&["m/s", "mps"], Speed, 1.0),
    unit(&["km/h", "kmh", "kph"], Speed, 1.0 / 3.6),
    unit(&["mph"], Speed, 0.44704),
    unit(&["kn", "knot", "knots"], Speed, 0.514444),
    Unit { names: &["°C", "c", "celsius", "°c"], measure: Temperature, factor: 1.0, offset: 273.15 },
    Unit { names: &["°F", "f", "fahrenheit", "°f"], measure: Temperature, factor: 5.0 / 9.0, offset: 459.67 },
    Unit { names: &["K", "k", "kelvin"], measure: Temperature, factor: 1.0, offset: 0.0 },
];

fn find_unit(name: &str) -> Option<&'static Unit> {
    let name = name.trim().trim_start_matches("degrees ").trim_start_matches("degree ");
    // Exact first, so "MB" and "mb" both work and "B" isn't "b".
    UNITS.iter().find(|u| u.names.contains(&name)).or_else(|| UNITS.iter().find(|u| u.names.iter().any(|n| n.eq_ignore_ascii_case(name))))
}

/// `5 km in miles`, `72 f to c`, `2 gb as mb`, `(3+4) hours -> min`: the amount in the other unit.
fn convert(input: &str) -> Option<Answer> {
    let (left, target) =
        [" in ", " to ", " as ", " -> ", "->"].iter().filter_map(|sep| input.rsplit_once(sep)).max_by_key(|(l, _)| l.len())?;
    let to = find_unit(target)?;
    // The amount, then the unit: the unit is what follows the last digit or bracket.
    let left = left.trim();
    let cut = left.rfind(|c: char| c.is_ascii_digit() || c == ')' || c == '.')? + 1;
    let (amount, from) = (left[..cut].trim(), left[cut..].trim());
    let from = find_unit(from)?;
    if from.measure != to.measure {
        return None;
    }
    let amount = if amount.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '_') {
        amount.replace('_', "").parse().ok()?
    } else {
        arithmetic(amount)?
    };
    let base = (amount + from.offset) * from.factor;
    let value = base / to.factor - to.offset;
    if !value.is_finite() {
        return None;
    }
    // Six decimals is plenty for a conversion; more is noise.
    let value = (value * 1e6).round() / 1e6;
    let name = to.names[0];
    let sep = if name.starts_with('°') { "" } else { " " };
    Some(Answer { shown: format!("{}{sep}{name}", format(value, true)), plain: format(value, false) })
}

fn looks_like_sum(s: &str) -> bool {
    let has_op = s.chars().skip(1).any(|c| "+-*/^%×÷(".contains(c)) || s.starts_with('(') || s.contains(" mod ");
    let has_fn = FUNCTIONS.iter().any(|(name, _)| s.starts_with(name));
    let starts_well = s.starts_with(|c: char| c.is_ascii_digit() || "(.-".contains(c));
    (has_op && starts_well) || has_fn || s == "pi"
}

type Function = (&'static str, fn(f64) -> f64);

const FUNCTIONS: &[Function] = &[
    ("sqrt", f64::sqrt),
    ("cbrt", f64::cbrt),
    ("sin", f64::sin),
    ("cos", f64::cos),
    ("tan", f64::tan),
    ("asin", f64::asin),
    ("acos", f64::acos),
    ("atan", f64::atan),
    ("ln", f64::ln),
    ("log", f64::log10),
    ("abs", f64::abs),
    ("round", f64::round),
    ("floor", f64::floor),
    ("ceil", f64::ceil),
];

struct Parser<'a> {
    src: &'a [u8],
    pos: usize,
}

impl Parser<'_> {
    fn space(&mut self) {
        while self.src.get(self.pos).is_some_and(u8::is_ascii_whitespace) {
            self.pos += 1;
        }
    }

    fn eat(&mut self, token: &str) -> bool {
        self.space();
        if self.src[self.pos..].starts_with(token.as_bytes()) {
            self.pos += token.len();
            true
        } else {
            false
        }
    }

    // expr := term (('+' | '-') term)*
    fn expr(&mut self) -> Option<f64> {
        let mut value = self.term()?;
        loop {
            if self.eat("+") {
                value += self.term()?;
            } else if self.eat("-") {
                value -= self.term()?;
            } else {
                return Some(value);
            }
        }
    }

    // term := power (('*' | '/' | 'mod') power)*
    fn term(&mut self) -> Option<f64> {
        let mut value = self.power()?;
        loop {
            if self.eat("*") || self.eat("×") || self.eat("x ") {
                value *= self.power()?;
            } else if self.eat("/") || self.eat("÷") {
                value /= self.power()?;
            } else if self.eat("mod") {
                value %= self.power()?;
            } else {
                return Some(value);
            }
        }
    }

    // power := unary ('^' power)?   (right associative: 2^3^2 is 2^9)
    fn power(&mut self) -> Option<f64> {
        let base = self.unary()?;
        if self.eat("^") || self.eat("**") {
            return Some(base.powf(self.power()?));
        }
        Some(base)
    }

    fn unary(&mut self) -> Option<f64> {
        if self.eat("-") {
            return Some(-self.unary()?);
        }
        if self.eat("+") {
            return self.unary();
        }
        let value = self.atom()?;
        // A percent sign after a number is that many hundredths.
        if self.eat("%") { Some(value / 100.0) } else { Some(value) }
    }

    fn atom(&mut self) -> Option<f64> {
        self.space();
        if self.eat("(") {
            let value = self.expr()?;
            // Forgive a missing closing bracket at the very end, as you're still typing it.
            if !self.eat(")") && self.pos < self.src.len() {
                return None;
            }
            return Some(value);
        }
        for (name, f) in FUNCTIONS {
            if self.eat(name) {
                return Some(f(self.unary()?));
            }
        }
        if self.eat("pi") || self.eat("π") {
            return Some(std::f64::consts::PI);
        }
        if self.eat("e") {
            return Some(std::f64::consts::E);
        }
        self.number()
    }

    fn number(&mut self) -> Option<f64> {
        self.space();
        let start = self.pos;
        while self.src.get(self.pos).is_some_and(|c| c.is_ascii_digit() || *c == b'.' || *c == b'_') {
            self.pos += 1;
        }
        let text: String = std::str::from_utf8(&self.src[start..self.pos]).ok()?.replace('_', "");
        text.parse().ok()
    }
}

fn format(value: f64, grouped: bool) -> String {
    if value == 0.0 {
        return "0".into();
    }
    if value.abs() >= 1e15 || value.abs() < 1e-9 {
        return format!("{value:e}");
    }
    let text = format!("{:.10}", value);
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if !grouped {
        return text.to_string();
    }
    let (sign, rest) = text.strip_prefix('-').map_or(("", text), |r| ("-", r));
    let (whole, fraction) = rest.split_once('.').map_or((rest, None), |(w, f)| (w, Some(f)));
    let mut out = String::with_capacity(text.len() + whole.len() / 3);
    for (i, digit) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(digit);
    }
    match fraction {
        Some(f) => format!("{sign}{out}.{f}"),
        None => format!("{sign}{out}"),
    }
}

#[cfg(test)]
mod tests {
    use super::evaluate;

    fn shown(s: &str) -> Option<String> {
        evaluate(s).map(|a| a.shown)
    }

    #[test]
    fn sums() {
        assert_eq!(shown("1+2*3").as_deref(), Some("7"));
        assert_eq!(shown("(1+2)*3").as_deref(), Some("9"));
        assert_eq!(shown("2^3^2").as_deref(), Some("512"));
        assert_eq!(shown("10/4").as_deref(), Some("2.5"));
        assert_eq!(shown("-3 + 5").as_deref(), Some("2"));
        assert_eq!(shown("1024*1024").as_deref(), Some("1,048,576"));
        assert_eq!(shown("15% * 80").as_deref(), Some("12"));
        assert_eq!(shown("7 mod 3").as_deref(), Some("1"));
        assert_eq!(shown("sqrt 16").as_deref(), Some("4"));
        assert_eq!(shown("2 × 3").as_deref(), Some("6"));
        assert_eq!(shown("(2+3").as_deref(), Some("5"));
        assert_eq!(evaluate("0.1+0.2").map(|a| a.plain).as_deref(), Some("0.3"));
    }

    #[test]
    fn conversions() {
        assert_eq!(shown("5 km in miles").as_deref(), Some("3.106856 mi"));
        assert_eq!(shown("72 f to c").as_deref(), Some("22.222222°C"));
        assert_eq!(shown("100 c to f").as_deref(), Some("212°F"));
        assert_eq!(shown("0 k in c").as_deref(), Some("-273.15°C"));
        assert_eq!(shown("2 GB in MB").as_deref(), Some("2,000 MB"));
        assert_eq!(shown("1 GiB to MiB").as_deref(), Some("1,024 MiB"));
        assert_eq!(shown("90 min in hours").as_deref(), Some("1.5 h"));
        assert_eq!(shown("6 ft to cm").as_deref(), Some("182.88 cm"));
        assert_eq!(shown("(2+3)kg in lb").as_deref(), Some("11.023113 lb"));
        assert_eq!(shown("100 km/h to mph").as_deref(), Some("62.137119 mph"));
        assert_eq!(evaluate("5 km in miles").map(|a| a.plain).as_deref(), Some("3.106856"));
        assert!(evaluate("5 km in kg").is_none(), "different measures");
        assert!(evaluate("log in to github").is_none());
        assert!(evaluate("photos in 2024").is_none());
    }

    #[test]
    fn not_sums() {
        assert!(evaluate("42").is_none());
        assert!(evaluate("firefox").is_none());
        assert!(evaluate("1/0").is_none());
        assert!(evaluate("3 +").is_none());
        assert!(evaluate("e-mail").is_none());
        assert!(evaluate("7-zip").is_none());
    }
}
