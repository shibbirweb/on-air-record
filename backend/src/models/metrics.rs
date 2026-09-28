//! The Prometheus text exposition format, written out by hand.
//!
//! The format is a handful of line shapes (version 0.0.4 of the text format, which every Prometheus
//! release still reads), so a dependency would add more code than it saves. Kept free of the application
//! so it can be tested against the format alone: what the service chooses to report lives in
//! `dto::metrics_dto`, and this only decides how it is spelled.

use std::fmt::Write;

/// The `Content-Type` a scraper expects for this format.
pub const CONTENT_TYPE: &str = "text/plain; version=0.0.4; charset=utf-8";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricKind {
    /// Only ever goes up, apart from resetting to zero, which Prometheus detects and handles.
    Counter,
    /// Goes up and down.
    Gauge,
}

impl MetricKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Counter => "counter",
            Self::Gauge => "gauge",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    pub labels: Vec<(&'static str, String)>,
    pub value: f64,
}

/// One metric name with its help text, its type and every series reported under it.
#[derive(Debug, Clone, PartialEq)]
pub struct MetricFamily {
    pub name: &'static str,
    pub help: &'static str,
    pub kind: MetricKind,
    pub samples: Vec<Sample>,
}

impl MetricFamily {
    pub fn gauge(name: &'static str, help: &'static str) -> Self {
        Self::new(name, help, MetricKind::Gauge)
    }

    pub fn counter(name: &'static str, help: &'static str) -> Self {
        Self::new(name, help, MetricKind::Counter)
    }

    fn new(name: &'static str, help: &'static str, kind: MetricKind) -> Self {
        Self {
            name,
            help,
            kind,
            samples: Vec::new(),
        }
    }

    /// The single series of a metric without labels.
    pub fn value(mut self, value: f64) -> Self {
        self.samples.push(Sample {
            labels: Vec::new(),
            value,
        });
        self
    }

    /// One series of a labelled metric.
    pub fn labelled(mut self, labels: &[(&'static str, &str)], value: f64) -> Self {
        self.samples.push(Sample {
            labels: labels
                .iter()
                .map(|(name, text)| (*name, text.to_string()))
                .collect(),
            value,
        });
        self
    }
}

/// The whole scrape body, in the order given.
///
/// A family with no series is left out whole. Writing to a `String` cannot fail, so the `fmt::Result`s are
/// dropped rather than threaded through.
pub fn render(families: &[MetricFamily]) -> String {
    let mut out = String::new();
    for family in families.iter().filter(|family| !family.samples.is_empty()) {
        let _ = writeln!(out, "# HELP {} {}", family.name, escape_help(family.help));
        let _ = writeln!(out, "# TYPE {} {}", family.name, family.kind.as_str());
        for sample in &family.samples {
            out.push_str(family.name);
            if !sample.labels.is_empty() {
                out.push('{');
                for (index, (label, text)) in sample.labels.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    let _ = write!(out, "{label}=\"{}\"", escape_label_value(text));
                }
                out.push('}');
            }
            let _ = writeln!(out, " {}", format_value(sample.value));
        }
    }
    out
}

/// Help text may hold anything but a raw backslash or line break.
fn escape_help(text: &str) -> String {
    text.replace('\\', "\\\\").replace('\n', "\\n")
}

/// A label value is quoted, so a quote needs escaping too.
fn escape_label_value(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

/// Rust's shortest round trip spelling, which never uses an exponent and drops the point from whole
/// numbers, with the format's own names for the values that are not numbers. Negative zero becomes zero,
/// since `-0` reads oddly on a dashboard and means the same.
fn format_value(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "+Inf" } else { "-Inf" }.to_string();
    }
    if value == 0.0 {
        return "0".to_string();
    }
    value.to_string()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn a_gauge_is_announced_then_reported() {
        let body = render(&[MetricFamily::gauge("oar_level", "How loud it is.").value(0.25)]);
        assert_eq!(
            body,
            "# HELP oar_level How loud it is.\n# TYPE oar_level gauge\noar_level 0.25\n"
        );
    }

    #[test]
    fn a_counter_says_it_is_one() {
        let body = render(&[MetricFamily::counter("oar_dropped_total", "Frames lost.").value(3.0)]);
        assert_eq!(
            body,
            "# HELP oar_dropped_total Frames lost.\n# TYPE oar_dropped_total counter\noar_dropped_total 3\n"
        );
    }

    #[test]
    fn labels_are_written_in_the_order_given() {
        let body = render(&[MetricFamily::gauge("oar_build_info", "Build.")
            .labelled(&[("version", "0.9.0"), ("channel", "beta")], 1.0)]);
        assert!(
            body.ends_with("oar_build_info{version=\"0.9.0\",channel=\"beta\"} 1\n"),
            "{body}"
        );
    }

    #[test]
    fn every_series_of_a_family_shares_one_header() {
        let body = render(&[MetricFamily::gauge("oar_state", "State.")
            .labelled(&[("state", "idle")], 1.0)
            .labelled(&[("state", "recording")], 0.0)]);
        assert_eq!(body.matches("# HELP").count(), 1);
        assert_eq!(body.matches("# TYPE").count(), 1);
        assert!(body.contains("oar_state{state=\"idle\"} 1\noar_state{state=\"recording\"} 0\n"));
    }

    #[test]
    fn families_follow_one_another_in_the_order_given() {
        let body = render(&[
            MetricFamily::gauge("oar_b", "B.").value(1.0),
            MetricFamily::gauge("oar_a", "A.").value(2.0),
        ]);
        let b = body.find("oar_b 1").expect("b");
        let a = body.find("oar_a 2").expect("a");
        assert!(b < a, "{body}");
    }

    /// A value that does not exist, such as the oldest recording before anything is recorded, is left out
    /// rather than reported as zero, which an alert would read as 1970. A family left with no series is
    /// dropped whole, header and all.
    #[test]
    fn a_family_with_nothing_to_report_is_left_out() {
        let body = render(&[
            MetricFamily::gauge("oar_oldest_seconds", "Oldest."),
            MetricFamily::gauge("oar_level", "Level.").value(0.5),
        ]);
        assert!(!body.contains("oar_oldest_seconds"), "{body}");
        assert!(body.contains("oar_level 0.5"));
    }

    #[test]
    fn nothing_at_all_renders_as_an_empty_body() {
        assert_eq!(render(&[]), "");
    }

    #[test]
    fn label_values_escape_the_three_characters_the_format_reserves() {
        let body = render(&[MetricFamily::gauge("oar_device", "Device.")
            .labelled(&[("name", "a \"quoted\" back\\slash\nnew line")], 1.0)]);
        assert!(
            body.contains(r#"oar_device{name="a \"quoted\" back\\slash\nnew line"} 1"#),
            "{body}"
        );
    }

    #[test]
    fn help_text_escapes_backslashes_and_new_lines_but_not_quotes() {
        let body = render(&[MetricFamily::gauge("oar_x", "Say \"hi\"\\\nthen stop.").value(1.0)]);
        assert!(
            body.starts_with("# HELP oar_x Say \"hi\"\\\\\\nthen stop.\n"),
            "{body}"
        );
    }

    #[test]
    fn whole_numbers_are_written_without_a_decimal_point_or_exponent() {
        let body = render(&[
            MetricFamily::gauge("oar_a", "A.").value(0.0),
            MetricFamily::gauge("oar_b", "B.").value(48_000.0),
            MetricFamily::gauge("oar_c", "C.").value(1_790_000_000.0),
            MetricFamily::gauge("oar_d", "D.").value(-2.0),
        ]);
        assert!(body.contains("oar_a 0\n"), "{body}");
        assert!(body.contains("oar_b 48000\n"), "{body}");
        assert!(body.contains("oar_c 1790000000\n"), "{body}");
        assert!(body.contains("oar_d -2\n"), "{body}");
    }

    #[test]
    fn fractions_keep_their_digits() {
        let body = render(&[MetricFamily::gauge("oar_level", "Level.").value(0.125)]);
        assert!(body.contains("oar_level 0.125\n"), "{body}");
    }

    #[test]
    fn values_outside_the_real_numbers_use_the_formats_own_spellings() {
        let body = render(&[
            MetricFamily::gauge("oar_nan", "N.").value(f64::NAN),
            MetricFamily::gauge("oar_up", "U.").value(f64::INFINITY),
            MetricFamily::gauge("oar_down", "D.").value(f64::NEG_INFINITY),
        ]);
        assert!(body.contains("oar_nan NaN\n"), "{body}");
        assert!(body.contains("oar_up +Inf\n"), "{body}");
        assert!(body.contains("oar_down -Inf\n"), "{body}");
    }

    #[test]
    fn negative_zero_is_plain_zero() {
        let body = render(&[MetricFamily::gauge("oar_z", "Z.").value(-0.0)]);
        assert!(body.contains("oar_z 0\n"), "{body}");
    }

    #[test]
    fn the_content_type_names_the_format_version() {
        assert_eq!(CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8");
    }

    /// Reads one sample line back the way a scraper does, for the property below: name, then each label
    /// value unescaped, then the value.
    /// A sample line read back: the name, each label with its unescaped value, and the value as written.
    pub(crate) type ParsedSample = (String, Vec<(String, String)>, String);

    pub(crate) fn parse_sample(line: &str) -> Option<ParsedSample> {
        let (name_end, rest) = match line.find('{') {
            Some(at) => (at, &line[at..]),
            None => {
                let at = line.find(' ')?;
                (at, &line[at..])
            }
        };
        let name = line[..name_end].to_string();
        let mut labels = Vec::new();
        let mut chars = rest.chars().peekable();
        if chars.peek() == Some(&'{') {
            chars.next();
            loop {
                let mut label = String::new();
                for c in chars.by_ref() {
                    if c == '=' {
                        break;
                    }
                    label.push(c);
                }
                if chars.next() != Some('"') {
                    return None;
                }
                let mut value = String::new();
                loop {
                    match chars.next()? {
                        '\\' => match chars.next()? {
                            '\\' => value.push('\\'),
                            '"' => value.push('"'),
                            'n' => value.push('\n'),
                            _ => return None,
                        },
                        '"' => break,
                        '\n' => return None,
                        c => value.push(c),
                    }
                }
                labels.push((label, value));
                match chars.next()? {
                    ',' => continue,
                    '}' => break,
                    _ => return None,
                }
            }
        }
        if chars.next() != Some(' ') {
            return None;
        }
        let value: String = chars.collect();
        Some((name, labels, value))
    }

    #[test]
    fn the_test_parser_reads_back_what_it_should() {
        let parsed = parse_sample(r#"oar_x{a="1\"2",b="\\"} 5"#).expect("parse");
        assert_eq!(parsed.0, "oar_x");
        assert_eq!(
            parsed.1,
            vec![
                ("a".to_string(), "1\"2".to_string()),
                ("b".to_string(), "\\".to_string())
            ]
        );
        assert_eq!(parsed.2, "5");
        assert_eq!(
            parse_sample("oar_y 0.5"),
            Some(("oar_y".to_string(), vec![], "0.5".to_string()))
        );
    }

    mod properties {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            /// A device name or anything else a label carries comes back exactly as it went in, whatever
            /// it holds, and never breaks the one line a series must fit on.
            #[test]
            fn any_label_value_reads_back_exactly(text in any::<String>()) {
                let body = render(&[MetricFamily::gauge("oar_device", "Device.")
                    .labelled(&[("name", text.as_str())], 1.0)]);
                let lines: Vec<&str> = body.lines().collect();
                prop_assert_eq!(lines.len(), 3, "{}", body);
                let parsed = parse_sample(lines[2]);
                prop_assert!(parsed.is_some(), "{}", body);
                let (name, labels, value) = parsed.unwrap_or_default();
                prop_assert_eq!(name, "oar_device");
                prop_assert_eq!(labels, vec![("name".to_string(), text.clone())]);
                prop_assert_eq!(value, "1");
            }

            /// Every finite value reads back as the number it was, so rounding in the formatter never
            /// moves a reading.
            #[test]
            fn any_finite_value_reads_back_as_the_same_number(value in any::<f64>().prop_filter("finite", |v| v.is_finite())) {
                let body = render(&[MetricFamily::gauge("oar_v", "V.").value(value)]);
                let line = body.lines().nth(2).unwrap_or_default();
                let text = line.strip_prefix("oar_v ").unwrap_or_default();
                let back: f64 = text.parse().unwrap_or(f64::NAN);
                prop_assert_eq!(back, value);
            }
        }
    }
}
