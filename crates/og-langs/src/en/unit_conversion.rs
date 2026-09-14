//! Rust port of LanguageTool's `UnitConversionRule`
//! (`AbstractUnitConversionRule`, ids `METRIC_UNITS_EN_GENERAL` /
//! `METRIC_UNITS_EN_IMPERIAL` / `METRIC_UNITS_EN_US`).
//!
//! Subset ported: the SUGGESTION path — a number followed by a non-metric
//! unit gets a suggestion appending the metric equivalent, e.g.
//! "10 miles (16.09 km)". Java's CHECK path (verifying conversions the user
//! already wrote, e.g. "10 miles (16 km)") is NOT ported; documented in
//! docs/plans/compatibility-matrix.md. Java tags these rules picky-only.

use og_core::{
    AnalyzedSentence, Category, IssueType, RuleMatch, RuleMatchContext, RuleMatchRule,
    SuggestedReplacement, rule::Rule,
};
use regex::Regex;
use std::sync::LazyLock;

/// A unit recognized in text, with its metric conversion factor.
struct UnitDef {
    /// Suffix appended to the matched unit text to build the number+unit
    /// regex (Java `addUnit` pattern).
    pattern: &'static str,
    /// Human name of the metric unit for the suggestion.
    metric_name: &'static str,
    /// Multiply the non-metric value by this to get metric.
    factor: f64,
}

/// Java `UnitConversionRule` (general) unit table — non-metric units only
/// (metric units never produce a SUGGESTION).
const GENERAL_UNITS: &[UnitDef] = &[
    UnitDef {
        pattern: "miles per hour",
        metric_name: "km/h",
        factor: 1.609344,
    },
    UnitDef {
        pattern: "pounds?",
        metric_name: "kg",
        factor: 0.45359237,
    },
    UnitDef {
        pattern: "ounces?",
        metric_name: "g",
        factor: 28.349523125,
    },
    UnitDef {
        pattern: "feet",
        metric_name: "m",
        factor: 0.3048,
    },
    UnitDef {
        pattern: "miles?",
        metric_name: "km",
        factor: 1.609344,
    },
    UnitDef {
        pattern: "yards?",
        metric_name: "m",
        factor: 0.9144,
    },
    UnitDef {
        pattern: "inch(es)?",
        metric_name: "cm",
        factor: 2.54,
    },
];

/// Java `UnitConversionRuleImperial`/`UnitConversionRuleUS` additions
/// (imperial/US volume units).
const VARIANT_UNITS: &[UnitDef] = &[
    UnitDef {
        pattern: "quarts?",
        metric_name: "l",
        factor: 0.946352946,
    },
    UnitDef {
        pattern: "gallons?",
        metric_name: "l",
        factor: 3.785411784,
    },
    UnitDef {
        pattern: "pints?",
        metric_name: "l",
        factor: 0.473176473,
    },
];

/// Fahrenheit needs a non-linear conversion (Java specialPatterns).
fn fahrenheit_to_celsius(f: f64) -> f64 {
    (f - 32.0) * 5.0 / 9.0
}

fn build_regex(units: &[UnitDef], with_fahrenheit: bool) -> Regex {
    let mut alternation: Vec<String> = units.iter().map(|u| format!("(?:{})", u.pattern)).collect();
    if with_fahrenheit {
        alternation.push("(?:degrees? )?Fahrenheit".to_string());
    }
    // Java: NUMBER_REGEX + " " + unit pattern
    Regex::new(&format!(
        r"(-?\b[0-9]{{1,32}}[0-9,.]{{0,32}})\s+(?:{})\b",
        alternation.join("|")
    ))
    .expect("valid unit regex")
}

/// Parse a number like "10", "1,000.5", "1.000,5" (Java uses locale-aware
/// NumberFormat; here a heuristic: strip thousands separators).
/// True for "1,000" / "1,000,000" style thousands grouping.
fn thousands_grouped(s: &str) -> bool {
    let parts: Vec<&str> = s.split(',').collect();
    parts.len() >= 2
        && !parts[0].is_empty()
        && parts[0].len() <= 3
        && parts[0].chars().all(|c| c.is_ascii_digit())
        && parts[1..]
            .iter()
            .all(|p| p.len() == 3 && p.chars().all(|c| c.is_ascii_digit()))
}

fn parse_number(raw: &str) -> Option<f64> {
    let cleaned: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
    let last_comma = cleaned.rfind(',');
    let last_dot = cleaned.rfind('.');
    let normalized = match (last_comma, last_dot) {
        (Some(c), Some(d)) => {
            // whichever separator comes last is the decimal separator
            if c > d {
                cleaned.replace('.', "").replace(',', ".")
            } else {
                cleaned.replace(',', "")
            }
        }
        (Some(_), None)
            if cleaned.matches(',').count() == 1
                && !cleaned.ends_with(',')
                && !thousands_grouped(&cleaned) =>
        {
            // "1,5" — decimal comma
            cleaned.replace(',', ".")
        }
        _ => cleaned.replace(',', ""),
    };
    normalized.parse::<f64>().ok()
}

fn format_number(value: f64) -> String {
    // Java: NumberFormat with max 2 fraction digits, HALF_UP rounding
    format!("{:.2}", value)
}

pub struct UnitConversionRule {
    id: String,
    description: String,
    regex: Regex,
    /// General table variant (matches long unit words); variant tables add
    /// volume units. Each variant gets its own regex set.
    volume_regex: Option<Regex>,
}

impl UnitConversionRule {
    /// Java `UnitConversionRule` (id `METRIC_UNITS_EN_GENERAL`).
    pub fn general() -> Self {
        Self {
            id: "METRIC_UNITS_EN_GENERAL".to_string(),
            description: "Suggests or checks conversion of units to their metric equivalents."
                .to_string(),
            regex: build_regex(GENERAL_UNITS, true),
            volume_regex: None,
        }
    }

    /// Java `UnitConversionRuleUS` (id `METRIC_UNITS_EN_US`).
    pub fn us() -> Self {
        Self {
            id: "METRIC_UNITS_EN_US".to_string(),
            description: "Suggests or checks conversion of units to their metric equivalents."
                .to_string(),
            regex: build_regex(GENERAL_UNITS, true),
            volume_regex: Some(build_regex(VARIANT_UNITS, false)),
        }
    }

    /// Java `UnitConversionRuleImperial` (id `METRIC_UNITS_EN_IMPERIAL`).
    /// Imperial units differ from US only in the conversion factors.
    pub fn imperial() -> Self {
        Self {
            id: "METRIC_UNITS_EN_IMPERIAL".to_string(),
            description: "Suggests or checks conversion of units to their metric equivalents."
                .to_string(),
            regex: build_regex(GENERAL_UNITS, true),
            volume_regex: Some(build_regex(VARIANT_UNITS, false)),
        }
    }

    fn convert(raw_number: &str, unit_text: &str, imperial: bool) -> Option<(f64, String)> {
        let number = parse_number(raw_number)?;
        let unit = unit_text.to_lowercase();
        // Fahrenheit is matched by the general regex
        if unit.ends_with("fahrenheit") {
            return Some((fahrenheit_to_celsius(number), "°C".to_string()));
        }
        let tables: &[&[UnitDef]] = if imperial {
            &[GENERAL_UNITS, IMPERIAL_VOLUME_UNITS]
        } else {
            &[GENERAL_UNITS, VARIANT_UNITS]
        };
        for table in tables {
            for def in *table {
                let re = Regex::new(&format!("^(?:{})$", def.pattern)).ok()?;
                if re.is_match(&unit) {
                    return Some((number * def.factor, def.metric_name.to_string()));
                }
            }
        }
        None
    }

    fn match_with(
        &self,
        sentence: &AnalyzedSentence,
        re: &Regex,
        imperial: bool,
    ) -> Vec<RuleMatch> {
        let mut matches = Vec::new();
        for caps in re.captures_iter(sentence.text()) {
            let whole = caps.get(0).expect("group 0 present");
            let number = caps.get(1).expect("group 1 present");
            let unit_text = sentence.text()[number.end() + 1..whole.end()].trim();
            if let Some((metric, unit_name)) = Self::convert(number.as_str(), unit_text, imperial) {
                // Skip conversions the user already wrote: "10 miles (16 km)"
                let after = &sentence.text()[whole.end()..];
                let close_paren = after.find(')');
                let open_paren = after.find('(');
                if matches!((open_paren, close_paren), (Some(op), Some(cp)) if op < cp) {
                    continue; // existing conversion after the unit
                }
                let original = whole.as_str();
                let converted = format!("{} {}", format_number(metric), unit_name);
                let message =
                    "Writing for an international audience? Consider adding the metric equivalent.";
                let rm_rule =
                    RuleMatchRule::new(&self.id, &self.description).with_category(self.category());
                let ctx_start = whole.start().saturating_sub(40);
                let ctx_end = (whole.end() + 40).min(sentence.text().len());
                matches.push(
                    RuleMatch::new(
                        message,
                        whole.start(),
                        whole.len(),
                        rm_rule,
                        RuleMatchContext::new(
                            sentence.text()[ctx_start..ctx_end].to_string(),
                            ctx_start,
                            ctx_end - ctx_start,
                        ),
                    )
                    .with_short_message("Add metric equivalent?")
                    .with_replacements(vec![SuggestedReplacement::new(format!(
                        "{original} ({converted})"
                    ))])
                    .with_sentence(sentence.text().to_string()),
                );
            }
        }
        matches
    }
}

/// Imperial volume factors (Java IMP_* units).
const IMPERIAL_VOLUME_UNITS: &[UnitDef] = &[
    UnitDef {
        pattern: "quarts?",
        metric_name: "l",
        factor: 1.1365225,
    },
    UnitDef {
        pattern: "gallons?",
        metric_name: "l",
        factor: 4.54609,
    },
    UnitDef {
        pattern: "pints?",
        metric_name: "l",
        factor: 0.56826125,
    },
];

static ANTI_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        // "3-5 pounds", "1/4 mile", "2:1", thousands separator 100'000
        Regex::new(r"\d+[-‐–]\d+").unwrap(),
        Regex::new(r"\d+/\d+").unwrap(),
        Regex::new(r"\d+:\d+").unwrap(),
    ]
});

impl Rule for UnitConversionRule {
    fn id(&self) -> &str {
        &self.id
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn is_default_on(&self) -> bool {
        // Java: setTags(Arrays.asList(Tag.picky)) — picky-only.
        false
    }

    fn category(&self) -> Category {
        Category::new("STYLE", "Style")
    }

    fn issue_type(&self) -> IssueType {
        IssueType::Other("style".to_string())
    }

    fn match_sentence(&self, sentence: &AnalyzedSentence) -> Vec<RuleMatch> {
        // Java anti-patterns: skip number ranges/fractions/ratios
        if ANTI_PATTERNS.iter().any(|re| re.is_match(sentence.text())) {
            return Vec::new();
        }
        let imperial = self.id.ends_with("IMPERIAL");
        let mut matches = self.match_with(sentence, &self.regex, imperial);
        if let Some(volume_re) = &self.volume_regex {
            matches.extend(self.match_with(sentence, volume_re, imperial));
        }
        matches
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use og_core::{AnalyzedSentence, AnalyzedToken, AnalyzedTokenReadings};

    fn sentence(text: &str) -> AnalyzedSentence {
        let mut s = AnalyzedSentence::new(text.to_string(), 0, text.len());
        // unit rules are text-based; a single word token keeps the pipeline
        // shape valid
        s.set_tokens(vec![AnalyzedTokenReadings::new(AnalyzedToken::new(
            text,
            0,
            text.len(),
        ))]);
        s
    }

    #[test]
    fn suggests_metric_for_miles() {
        let rule = UnitConversionRule::general();
        let s = sentence("We walked 10 miles yesterday.");
        let matches = rule.match_sentence(&s);
        assert_eq!(matches.len(), 1);
        assert!(matches[0].replacements()[0].value().contains("16.09 km"));
    }

    #[test]
    fn fahrenheit_conversion() {
        let rule = UnitConversionRule::us();
        let s = sentence("It was 100 degrees Fahrenheit outside.");
        let matches = rule.match_sentence(&s);
        assert_eq!(matches.len(), 1);
        assert!(matches[0].replacements()[0].value().contains("37.78 °C"));
    }

    #[test]
    fn existing_conversion_not_flagged() {
        let rule = UnitConversionRule::general();
        let s = sentence("We walked 10 miles (16.09 km) yesterday.");
        assert!(rule.match_sentence(&s).is_empty());
    }

    #[test]
    fn ranges_and_fractions_skipped() {
        let rule = UnitConversionRule::general();
        assert!(
            rule.match_sentence(&sentence("It weighs 3-5 pounds."))
                .is_empty()
        );
        assert!(
            rule.match_sentence(&sentence("A quarter of a mile: 1/4 mile."))
                .is_empty()
        );
    }

    #[test]
    fn picky_only_default_off() {
        let rule = UnitConversionRule::general();
        assert!(!rule.is_default_on());
        assert_eq!(rule.id(), "METRIC_UNITS_EN_GENERAL");
    }

    #[test]
    fn number_parsing_heuristics() {
        assert_eq!(parse_number("10"), Some(10.0));
        assert_eq!(parse_number("1,000"), Some(1000.0));
        assert_eq!(parse_number("1.5"), Some(1.5));
        assert_eq!(parse_number("1,5"), Some(1.5));
    }
}
