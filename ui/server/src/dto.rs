//! Serde mirrors of the library's model types.
//!
//! The library stays free of serde by decision (2026-08-10); everything the
//! frontend renders crosses the HTTP boundary as one of these DTOs. Integers
//! and decimals serialize as strings, JSON numbers being `f64` and unable to
//! hold every `i64` exactly.

use data_diff::Value;
use serde::Serialize;

/// A value as the frontend renders it: a kind for styling (null, NaN, and
/// the empty string staying three distinct things) and an honest text form.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ValueDto {
    pub kind: String,
    pub text: String,
}

pub fn value(value: &Value) -> ValueDto {
    let (kind, text) = match value {
        Value::Null => ("null", "null".to_owned()),
        Value::Boolean(value) => ("boolean", value.to_string()),
        Value::Int64(value) => ("int64", value.to_string()),
        Value::Double(value) => ("double", format_double(*value)),
        Value::String(value) => ("string", value.clone()),
        Value::Timestamp {
            value,
            unit,
            timezone,
        } => {
            let zone = timezone
                .as_ref()
                .map(|zone| format!(" {zone}"))
                .unwrap_or_default();
            ("timestamp", format!("{value} {unit:?}{zone}"))
        }
        Value::Date32(days) => ("date", format!("{days}d")),
        Value::Date64(millis) => ("date", format!("{millis}ms")),
        Value::Decimal128 {
            value,
            precision: _,
            scale,
        } => ("decimal", format_decimal(&value.to_string(), *scale)),
        Value::Decimal256 {
            value,
            precision: _,
            scale,
        } => ("decimal", format_decimal(&value.to_string(), *scale)),
        Value::Opaque(bytes) => ("opaque", hex(bytes)),
    };
    ValueDto {
        kind: kind.to_owned(),
        text,
    }
}

/// Render a double at a fixed two decimal places, so numeric columns align
/// and the binary tail a full rendering would show (`40.50194903904803`)
/// stays out of view. Magnitudes where two decimals say nothing — vanishing
/// or vast — go scientific rather than rendering as `0.00` or spilling.
fn format_double(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value.is_infinite() {
        return if value > 0.0 { "inf" } else { "-inf" }.to_owned();
    }
    let magnitude = value.abs();
    if magnitude != 0.0 && !(1e-2..1e15).contains(&magnitude) {
        return format!("{value:.2e}");
    }
    format!("{value:.2}")
}

/// The difference of two numeric values, for the cell view's delta column;
/// `None` for non-numeric pairs and for results no number names.
pub fn delta(old: &Value, new: &Value) -> Option<ValueDto> {
    let (old, new) = match (old, new) {
        (Value::Int64(old), Value::Int64(new)) => (*old as f64, *new as f64),
        (Value::Double(old), Value::Double(new)) => (*old, *new),
        (Value::Int64(old), Value::Double(new)) => (*old as f64, *new),
        (Value::Double(old), Value::Int64(new)) => (*old, *new as f64),
        _ => return None,
    };
    let delta = new - old;
    if delta.is_finite() {
        Some(value(&Value::Double(delta)))
    } else {
        None
    }
}

/// Render a decimal mantissa at its scale, `150` at scale `2` being `1.50`.
fn format_decimal(mantissa: &str, scale: i8) -> String {
    let (sign, digits) = match mantissa.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", mantissa),
    };
    if scale <= 0 {
        return format!("{sign}{digits}{}", "0".repeat(-scale as usize));
    }
    let scale = scale as usize;
    let padded = format!("{:0>width$}", digits, width = scale + 1);
    let point = padded.len() - scale;
    format!("{sign}{}.{}", &padded[..point], &padded[point..])
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// One row of the schema panel: an identity, an addition, or a drop.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SchemaRowDto {
    /// `identity`, `added`, or `dropped`.
    pub status: String,
    pub key: bool,
    /// One-based; always present for identities and drops.
    pub old_pos: Option<u32>,
    pub old_name: Option<String>,
    /// One-based; always present for identities and adds. `moved` says
    /// whether it differs from the old position, the panel showing it only
    /// then.
    pub new_pos: Option<u32>,
    pub new_name: Option<String>,
    pub moved: bool,
    /// The identity basis word (`exact`, `hinted`, …) when the two names
    /// differ.
    pub basis: Option<String>,
    /// `old -> new` source types, when they differ.
    pub type_change: Option<(String, String)>,
    /// The column's own source type — the new side's for identities and
    /// adds, the old side's for drops — so the panel can show every
    /// column's type, not only changed ones.
    pub source_type: Option<String>,
}

/// What opening a session returns: enough to pick the opening view and
/// render the schema panel without a second round trip.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SessionSummaryDto {
    pub old_path: String,
    pub new_path: String,
    /// Every count the opening-view rule and the expandos read.
    pub cells: usize,
    pub optimal: bool,
    /// Columns and rows with changed cells — the expando counts.
    pub edited_columns: usize,
    pub edited_rows: usize,
    /// The minimum cover's events — what the opening-view rule reads, a
    /// cover dominated by column edits opening the column view and so on.
    pub cover_columns: usize,
    pub cover_rows: usize,
    pub added_rows: usize,
    pub dropped_rows: usize,
    pub moved_rows: usize,
    pub fanout_groups: usize,
    /// The key columns' new-side names, so the views label their frozen
    /// columns with them rather than a literal "key". A positional key
    /// labels its one column "row".
    pub key_columns: Vec<String>,
    pub schema: Vec<SchemaRowDto>,
}

/// One page of a paginated list.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PageDto<T> {
    pub items: Vec<T>,
    pub total: usize,
    pub page: usize,
    pub page_size: usize,
}

pub fn page<T: Serialize>(
    items: Vec<T>,
    total: usize,
    page: usize,
    page_size: usize,
) -> PageDto<T> {
    PageDto {
        items,
        total,
        page,
        page_size,
    }
}

/// One changed cell in the cell view: `key | column | old | new`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CellRowDto {
    pub key: Vec<ValueDto>,
    /// The identity's new-side name, the design's display rule.
    pub column: String,
    pub old: ValueDto,
    pub new: ValueDto,
    /// `new - old` for numeric pairs, rendered like any other double.
    pub delta: Option<ValueDto>,
}

/// A column header in the column view.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ColumnHeaderDto {
    pub name: String,
    /// `pair` spans `old`/`new` sub-columns; `single` joins unchanged,
    /// added, or dropped columns.
    pub span: String,
    /// For singles: which side the values come from.
    pub side: Option<String>,
    /// What the column is doing here: `edited` (a pair), `context` (an
    /// unchanged identity), `added`, or `dropped`.
    pub origin: String,
}

/// One cell of a column-view row.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ColumnCellDto {
    pub old: Option<ValueDto>,
    pub new: Option<ValueDto>,
    pub changed: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ColumnRowDto {
    pub key: Vec<ValueDto>,
    pub cells: Vec<ColumnCellDto>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ColumnViewDto {
    pub columns: Vec<ColumnHeaderDto>,
    pub rows: PageDto<ColumnRowDto>,
}

/// One rendered line of a row-view section: an old row, a new row, or one
/// leg of a fanout, with per-column changed flags.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RowLineDto {
    /// `old`, `new`, `new 1`, … — the label the design's tables show.
    pub label: String,
    pub key: Vec<ValueDto>,
    pub values: Vec<ValueDto>,
    pub changed: Vec<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FanoutGroupDto {
    pub old_row: u32,
    pub new_rows: Vec<u32>,
    pub lines: Vec<RowLineDto>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RowViewDto {
    pub columns: Vec<String>,
    /// Paged lines for `edited`, `added`, `dropped`, and `moved`.
    pub rows: Option<PageDto<RowLineDto>>,
    /// Paged groups for `fanout`.
    pub groups: Option<PageDto<FanoutGroupDto>>,
}

#[cfg(test)]
mod tests {
    use super::format_double;

    #[test]
    fn doubles_render_at_a_fixed_two_decimals() {
        assert_eq!(format_double(40.50194903904803), "40.50");
        assert_eq!(format_double(9.99), "9.99");
        assert_eq!(format_double(16.0), "16.00");
        assert_eq!(format_double(0.0), "0.00");
        assert_eq!(format_double(-1234.5678), "-1234.57");
        assert_eq!(format_double(f64::NAN), "NaN");
        assert_eq!(format_double(f64::INFINITY), "inf");
        // Two decimals would say nothing at these magnitudes.
        assert_eq!(format_double(1.5e-7), "1.50e-7");
        assert_eq!(format_double(1.5e18), "1.50e18");
    }

    #[test]
    fn deltas_cover_numeric_pairs_and_skip_the_rest() {
        use super::{delta, Value};
        use crate::dto::value;

        assert_eq!(
            delta(&Value::Int64(14), &Value::Double(16.5)),
            Some(value(&Value::Double(2.5)))
        );
        assert_eq!(
            delta(&Value::Double(20.0), &Value::Double(21.03)),
            Some(value(&Value::Double(1.03)))
        );
        assert_eq!(delta(&Value::Null, &Value::Int64(1)), None);
        assert_eq!(
            delta(
                &Value::String("a".to_owned()),
                &Value::String("b".to_owned())
            ),
            None
        );
        // NaN - NaN names nothing.
        assert_eq!(delta(&Value::Double(f64::NAN), &Value::Double(1.0)), None);
    }
}
