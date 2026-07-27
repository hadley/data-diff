use arrow_array::{
    Array, BooleanArray, Float32Array, Float64Array, Int8Array, Int16Array, Int32Array, Int64Array,
    LargeStringArray, StringArray, UInt8Array, UInt16Array, UInt32Array, UInt64Array,
};
use arrow_cast::cast;
use arrow_schema::DataType;
use xxhash_rust::xxh3::xxh3_128_with_seed;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Boolean,
    Int,
    Double,
    String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Domain {
    Boolean,
    Int,
    Double,
    String,
    IntDouble,
    StringBoolean,
    StringInt,
    StringDouble,
}

/// A type-pair-specific equality and hashing plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ComparisonPlan {
    old: Kind,
    new: Kind,
    domain: Domain,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CanonicalValue {
    Null,
    Boolean(bool),
    Int(i64),
    Double(u64),
    String(Vec<u8>),
    UnparsedString(Vec<u8>),
}

impl CanonicalValue {
    pub(crate) fn invalid_key(&self) -> bool {
        matches!(self, Self::Null)
            || matches!(self, Self::Double(bits) if f64::from_bits(*bits).is_nan())
    }
}

impl ComparisonPlan {
    pub(crate) fn new(old: &DataType, new: &DataType) -> Option<Self> {
        let old = kind(old)?;
        let new = kind(new)?;
        let domain = match (old, new) {
            (Kind::Boolean, Kind::Boolean) => Domain::Boolean,
            (Kind::Int, Kind::Int) => Domain::Int,
            (Kind::Double, Kind::Double) => Domain::Double,
            (Kind::String, Kind::String) => Domain::String,
            (Kind::Int, Kind::Double) | (Kind::Double, Kind::Int) => Domain::IntDouble,
            (Kind::String, Kind::Boolean) | (Kind::Boolean, Kind::String) => Domain::StringBoolean,
            (Kind::String, Kind::Int) | (Kind::Int, Kind::String) => Domain::StringInt,
            (Kind::String, Kind::Double) | (Kind::Double, Kind::String) => Domain::StringDouble,
            _ => return None,
        };
        Some(Self { old, new, domain })
    }

    pub(crate) fn canonicalize_old(&self, values: &dyn Array) -> Vec<CanonicalValue> {
        self.canonicalize(values, self.old)
    }

    pub(crate) fn canonicalize_new(&self, values: &dyn Array) -> Vec<CanonicalValue> {
        self.canonicalize(values, self.new)
    }

    fn canonicalize(&self, values: &dyn Array, kind: Kind) -> Vec<CanonicalValue> {
        raw_values(values, kind)
            .into_iter()
            .map(|value| canonicalize(value, self.domain))
            .collect()
    }
}

#[derive(Clone, Debug)]
enum RawValue {
    Null,
    Boolean(bool),
    Int(i64),
    Double(f64),
    String(Vec<u8>),
}

fn kind(data_type: &DataType) -> Option<Kind> {
    match data_type {
        DataType::Boolean => Some(Kind::Boolean),
        DataType::Int8
        | DataType::Int16
        | DataType::Int32
        | DataType::Int64
        | DataType::UInt8
        | DataType::UInt16
        | DataType::UInt32
        | DataType::UInt64 => Some(Kind::Int),
        DataType::Float32 | DataType::Float64 => Some(Kind::Double),
        DataType::Utf8 | DataType::LargeUtf8 => Some(Kind::String),
        DataType::Dictionary(_, value)
            if matches!(value.as_ref(), DataType::Utf8 | DataType::LargeUtf8) =>
        {
            Some(Kind::String)
        }
        _ => None,
    }
}

fn raw_values(values: &dyn Array, kind: Kind) -> Vec<RawValue> {
    match kind {
        Kind::Boolean => primitive_values::<BooleanArray, _>(values, |array, row| {
            RawValue::Boolean(array.value(row))
        }),
        Kind::Int => integer_values(values),
        Kind::Double => double_values(values),
        Kind::String => string_values(values),
    }
}

fn primitive_values<A, F>(values: &dyn Array, value: F) -> Vec<RawValue>
where
    A: Array + 'static,
    F: Fn(&A, usize) -> RawValue,
{
    let array = values.as_any().downcast_ref::<A>().unwrap();
    (0..array.len())
        .map(|row| {
            if array.is_null(row) {
                RawValue::Null
            } else {
                value(array, row)
            }
        })
        .collect()
}

macro_rules! integers {
    ($values:expr, $array:ty) => {
        primitive_values::<$array, _>($values, |array, row| RawValue::Int(array.value(row) as i64))
    };
}

fn integer_values(values: &dyn Array) -> Vec<RawValue> {
    match values.data_type() {
        DataType::Int8 => integers!(values, Int8Array),
        DataType::Int16 => integers!(values, Int16Array),
        DataType::Int32 => integers!(values, Int32Array),
        DataType::Int64 => integers!(values, Int64Array),
        DataType::UInt8 => integers!(values, UInt8Array),
        DataType::UInt16 => integers!(values, UInt16Array),
        DataType::UInt32 => integers!(values, UInt32Array),
        DataType::UInt64 => integers!(values, UInt64Array),
        _ => unreachable!("validated integer type"),
    }
}

fn double_values(values: &dyn Array) -> Vec<RawValue> {
    match values.data_type() {
        DataType::Float32 => primitive_values::<Float32Array, _>(values, |array, row| {
            RawValue::Double(f64::from(array.value(row)))
        }),
        DataType::Float64 => primitive_values::<Float64Array, _>(values, |array, row| {
            RawValue::Double(array.value(row))
        }),
        _ => unreachable!("validated double type"),
    }
}

fn string_values(values: &dyn Array) -> Vec<RawValue> {
    let owned;
    let values = if matches!(values.data_type(), DataType::Dictionary(_, _)) {
        owned = cast(values, &DataType::LargeUtf8).expect("validated string dictionary");
        owned.as_ref()
    } else {
        values
    };
    match values.data_type() {
        DataType::Utf8 => primitive_values::<StringArray, _>(values, |array, row| {
            RawValue::String(array.value(row).as_bytes().to_vec())
        }),
        DataType::LargeUtf8 => primitive_values::<LargeStringArray, _>(values, |array, row| {
            RawValue::String(array.value(row).as_bytes().to_vec())
        }),
        _ => unreachable!("validated string type"),
    }
}

fn canonicalize(value: RawValue, domain: Domain) -> CanonicalValue {
    match value {
        RawValue::Null => CanonicalValue::Null,
        RawValue::Boolean(value) => CanonicalValue::Boolean(value),
        RawValue::Int(value) => match domain {
            Domain::Double | Domain::StringDouble => canonical_double(value as f64),
            _ => CanonicalValue::Int(value),
        },
        RawValue::Double(value) => match domain {
            Domain::IntDouble => exact_double_to_i64(value)
                .map(CanonicalValue::Int)
                .unwrap_or_else(|| canonical_double(value)),
            _ => canonical_double(value),
        },
        RawValue::String(value) => match domain {
            Domain::String => CanonicalValue::String(value),
            Domain::StringBoolean => parse_bytes(&value, |value| {
                value.parse::<bool>().ok().map(CanonicalValue::Boolean)
            }),
            Domain::StringInt => parse_bytes(&value, |value| {
                parse_exact_i64(value).map(CanonicalValue::Int)
            }),
            Domain::StringDouble => parse_bytes(&value, |value| {
                value.parse::<f64>().ok().map(canonical_double)
            }),
            _ => unreachable!("string value in non-string domain"),
        },
    }
}

fn parse_bytes(bytes: &[u8], parse: impl FnOnce(&str) -> Option<CanonicalValue>) -> CanonicalValue {
    std::str::from_utf8(bytes)
        .ok()
        .and_then(parse)
        .unwrap_or_else(|| CanonicalValue::UnparsedString(bytes.to_vec()))
}

fn canonical_double(value: f64) -> CanonicalValue {
    let bits = if value.is_nan() {
        f64::NAN.to_bits()
    } else if value == 0.0 {
        0
    } else {
        value.to_bits()
    };
    CanonicalValue::Double(bits)
}

fn exact_double_to_i64(value: f64) -> Option<i64> {
    const I64_UPPER_EXCLUSIVE: f64 = 9_223_372_036_854_775_808.0;
    if value.is_finite()
        && value.fract() == 0.0
        && value >= i64::MIN as f64
        && value < I64_UPPER_EXCLUSIVE
    {
        let integer = value as i64;
        (integer as f64 == value).then_some(integer)
    } else {
        None
    }
}

fn parse_exact_i64(value: &str) -> Option<i64> {
    let bytes = value.as_bytes();
    let (negative, rest) = match bytes.first()? {
        b'+' => (false, &bytes[1..]),
        b'-' => (true, &bytes[1..]),
        _ => (false, bytes),
    };
    if rest.is_empty() {
        return None;
    }

    let exponent_at = rest.iter().position(|byte| matches!(byte, b'e' | b'E'));
    let (mantissa, exponent) = match exponent_at {
        Some(index) => {
            let exponent = parse_exponent(&rest[index + 1..])?;
            (&rest[..index], exponent)
        }
        None => (rest, 0),
    };
    let dot_at = mantissa.iter().position(|byte| *byte == b'.');
    if dot_at.is_some_and(|index| mantissa[index + 1..].contains(&b'.')) {
        return None;
    }
    let (whole, fraction) = match dot_at {
        Some(index) => (&mantissa[..index], &mantissa[index + 1..]),
        None => (mantissa, &[][..]),
    };
    if whole.is_empty() || !whole.iter().chain(fraction).all(u8::is_ascii_digit) {
        return None;
    }

    let mut digits = Vec::with_capacity(whole.len() + fraction.len());
    digits.extend_from_slice(whole);
    digits.extend_from_slice(fraction);
    if digits.iter().all(|digit| *digit == b'0') {
        return Some(0);
    }
    let scale = exponent.checked_sub(i64::try_from(fraction.len()).ok()?)?;
    if scale < 0 {
        let remove = usize::try_from(scale.unsigned_abs()).ok()?;
        if remove > digits.len() || !digits[digits.len() - remove..].iter().all(|d| *d == b'0') {
            return None;
        }
        digits.truncate(digits.len() - remove);
    }

    let limit = if negative {
        i64::MAX as u64 + 1
    } else {
        i64::MAX as u64
    };
    let mut magnitude = 0_u64;
    for digit in digits {
        magnitude = magnitude
            .checked_mul(10)?
            .checked_add(u64::from(digit - b'0'))?;
        if magnitude > limit {
            return None;
        }
    }
    for _ in 0..usize::try_from(scale.max(0)).ok()? {
        magnitude = magnitude.checked_mul(10)?;
        if magnitude > limit {
            return None;
        }
    }

    if negative {
        if magnitude == i64::MAX as u64 + 1 {
            Some(i64::MIN)
        } else {
            Some(-(magnitude as i64))
        }
    } else {
        Some(magnitude as i64)
    }
}

fn parse_exponent(bytes: &[u8]) -> Option<i64> {
    let (negative, digits) = match bytes.first()? {
        b'+' => (false, &bytes[1..]),
        b'-' => (true, &bytes[1..]),
        _ => (false, bytes),
    };
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let mut value = 0_i64;
    for digit in digits {
        value = value
            .checked_mul(10)?
            .checked_add(i64::from(digit - b'0'))?;
    }
    if negative {
        value.checked_neg()
    } else {
        Some(value)
    }
}

pub(crate) trait StableHasher {
    fn hash(&self, bytes: &[u8]) -> u128;
}

pub(crate) struct Xxh3;

impl StableHasher for Xxh3 {
    fn hash(&self, bytes: &[u8]) -> u128 {
        xxh3_128_with_seed(bytes, 0)
    }
}

pub(crate) fn stable_hash(value: &CanonicalValue) -> u128 {
    hash_with(value, &Xxh3)
}

fn hash_with(value: &CanonicalValue, hasher: &impl StableHasher) -> u128 {
    let mut bytes = Vec::new();
    match value {
        CanonicalValue::Null => bytes.push(0),
        CanonicalValue::Boolean(value) => {
            bytes.push(1);
            bytes.push(u8::from(*value));
        }
        CanonicalValue::Int(value) => {
            bytes.push(2);
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        CanonicalValue::Double(value) => {
            bytes.push(3);
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        CanonicalValue::String(value) => encode_bytes(4, value, &mut bytes),
        CanonicalValue::UnparsedString(value) => encode_bytes(5, value, &mut bytes),
    }
    hasher.hash(&bytes)
}

fn encode_bytes(tag: u8, value: &[u8], output: &mut Vec<u8>) {
    output.push(tag);
    output.extend_from_slice(&(value.len() as u64).to_le_bytes());
    output.extend_from_slice(value);
}

#[cfg(test)]
pub(crate) fn equal_after_hash(
    old: &CanonicalValue,
    new: &CanonicalValue,
    hasher: &impl StableHasher,
) -> bool {
    hash_with(old, hasher) == hash_with(new, hasher) && old == new
}

#[cfg(test)]
mod tests {
    use arrow_array::types::Int8Type;
    use arrow_array::{ArrayRef, DictionaryArray, Int8Array, StringArray};
    use test_support::column;

    use super::{
        CanonicalValue, ComparisonPlan, StableHasher, equal_after_hash, parse_exact_i64,
        stable_hash,
    };

    fn values(array: ArrayRef, other: ArrayRef) -> (Vec<CanonicalValue>, Vec<CanonicalValue>) {
        let plan = ComparisonPlan::new(array.data_type(), other.data_type()).unwrap();
        (
            plan.canonicalize_old(array.as_ref()),
            plan.canonicalize_new(other.as_ref()),
        )
    }

    #[test]
    fn comparison_matrix_accepts_only_compatible_pairs() {
        use arrow_schema::DataType::{Boolean, Float64, Int64, Utf8};

        for (old, new) in [
            (Boolean, Boolean),
            (Int64, Int64),
            (Float64, Float64),
            (Utf8, Utf8),
            (Int64, Float64),
            (Utf8, Boolean),
            (Utf8, Int64),
            (Utf8, Float64),
        ] {
            assert!(ComparisonPlan::new(&old, &new).is_some(), "{old:?}/{new:?}");
            assert!(ComparisonPlan::new(&new, &old).is_some(), "{new:?}/{old:?}");
        }
        assert!(ComparisonPlan::new(&Boolean, &Int64).is_none());
        assert!(ComparisonPlan::new(&Boolean, &Float64).is_none());
    }

    #[test]
    fn exact_numeric_rules_canonicalize_equally() {
        let (old, new) = values(
            column!([i64::MIN, 0, 1, 9_007_199_254_740_993]),
            column!([i64::MIN as f64, -0.0, 1.0, 9_007_199_254_740_992.0]),
        );
        assert_eq!(old[0], new[0]);
        assert_eq!(old[1], new[1]);
        assert_eq!(old[2], new[2]);
        assert_ne!(old[3], new[3]);
    }

    #[test]
    fn doubles_canonicalize_nan_and_signed_zero() {
        let (old, new) = values(
            column!([f64::NAN, -0.0]),
            column!([f64::from_bits(0x7ff8_0000_0000_0001), 0.0]),
        );
        assert_eq!(old, new);
    }

    #[test]
    fn string_parsing_is_exact_and_does_not_trim() {
        let (old, new) = values(
            column!(["1", "1.0", "1e0", "1.5", " 1", "9223372036854775808"]),
            column!([1, 1, 1, 1, 1, i64::MAX]),
        );
        assert_eq!(old[0], new[0]);
        assert_eq!(old[1], new[1]);
        assert_eq!(old[2], new[2]);
        assert_ne!(old[3], new[3]);
        assert_ne!(old[4], new[4]);
        assert_ne!(old[5], new[5]);
    }

    #[test]
    fn strings_parse_to_boolean_and_double_domains() {
        let (old, new) = values(
            column!(["true", "True", "NaN", "inf"]),
            column!([true, true, true, true]),
        );
        assert_eq!(old[0], new[0]);
        assert_ne!(old[1], new[1]);

        let (old, new) = values(column!(["NaN", "inf"]), column!([f64::NAN, f64::INFINITY]));
        assert_eq!(old, new);
    }

    #[test]
    fn nulls_agree_across_compatible_types() {
        let (old, new) = values(column!([None::<&str>]), column!([None::<i64>]));
        assert_eq!(old, new);
        assert_eq!(old, [CanonicalValue::Null]);
    }

    #[test]
    fn dictionary_strings_use_logical_values() {
        // Written out rather than built by the fixture helper: the keys run
        // against the dictionary's own order, so canonicalization has to follow
        // them rather than read the values positionally.
        let dictionary = DictionaryArray::<Int8Type>::try_new(
            Int8Array::from(vec![1, 0]),
            std::sync::Arc::new(StringArray::from(vec!["a", "b"])),
        )
        .unwrap();
        let (old, new) = values(std::sync::Arc::new(dictionary), column!(["b", "a"]));
        assert_eq!(old, new);
    }

    #[test]
    fn exact_integer_parser_handles_bounds_and_exponents() {
        for (input, expected) in [
            ("+1", Some(1)),
            ("-1.00e2", Some(-100)),
            ("1e-0", Some(1)),
            ("0.000e999", Some(0)),
            ("-9223372036854775808", Some(i64::MIN)),
            ("9223372036854775807", Some(i64::MAX)),
            ("1e-1", None),
            ("1.2", None),
            ("", None),
            (".0", None),
            ("1e999999999999999999999", None),
        ] {
            assert_eq!(parse_exact_i64(input), expected, "{input}");
        }
    }

    #[test]
    fn equal_values_have_equal_stable_hashes() {
        let (old, new) = values(column!(["1.0"]), column!([1]));
        assert_eq!(old, new);
        assert_eq!(stable_hash(&old[0]), stable_hash(&new[0]));
    }

    #[test]
    fn a_hash_collision_cannot_create_equality() {
        struct Constant;
        impl StableHasher for Constant {
            fn hash(&self, _bytes: &[u8]) -> u128 {
                0
            }
        }

        assert!(!equal_after_hash(
            &CanonicalValue::Int(1),
            &CanonicalValue::Int(2),
            &Constant
        ));
    }
}
