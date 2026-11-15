//! Sums typed into the search field: `12 * (3 + 4)`, `2^10`, `sqrt 2`, `15% * 80`.
//! A small recursive descent parser; nothing is evaluated that isn't arithmetic.

pub struct Answer {
    /// The number as it's shown, with thousands separated: `1,048,576`.
    pub shown: String,
    /// The number as it's copied: `1048576`.
    pub plain: String,
}

/// The answer to `input`, if it's a sum. A lone number isn't one, so typing `42` still searches.
pub fn evaluate(input: &str) -> Option<Answer> {
    let input = input.trim();
    if input.is_empty() || !looks_like_sum(input) {
        return None;
    }
    let mut parser = Parser { src: input.as_bytes(), pos: 0 };
    let value = parser.expr()?;
    parser.space();
    if parser.pos != parser.src.len() || !value.is_finite() {
        return None;
    }
    Some(Answer { shown: format(value, true), plain: format(value, false) })
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
    fn not_sums() {
        assert!(evaluate("42").is_none());
        assert!(evaluate("firefox").is_none());
        assert!(evaluate("1/0").is_none());
        assert!(evaluate("3 +").is_none());
        assert!(evaluate("e-mail").is_none());
        assert!(evaluate("7-zip").is_none());
    }
}
