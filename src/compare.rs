use arrow_array::{
    Array, BooleanArray, Float32Array, Float64Array, Int8Array, Int16Array, Int32Array, Int64Array,
    LargeStringArray, StringArray, UInt8Array, UInt16Array, UInt32Array, UInt64Array, make_array,
};
use arrow_cast::cast;
use arrow_row::{RowConverter, SortField};
use arrow_schema::DataType;
use xxhash_rust::xxh3::xxh3_128_with_seed;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Kind {
    Boolean,
    Int,
    Double,
    String,
    /// Any admitted type outside the matrix, compared only against itself.
    Opaque,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Domain {
    Boolean,
    Int,
    Double,
    String,
    IntDouble,
    BoolInt,
    BoolDouble,
    StringBoolean,
    StringInt,
    StringDouble,
    Opaque,
}

/// A type-pair-specific equality and hashing plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ComparisonPlan {
    old: Kind,
    new: Kind,
    domain: Domain,
}

/// `Hash` is derived rather than hand-written: `Eq` is derived too, and
/// doubles are held as bits, so the two agree by construction. It counts value
/// frequencies; [`stable_hash`] remains the hash that has to stay stable
/// across runs and versions.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub(crate) enum CanonicalValue {
    Null,
    Boolean(bool),
    Int(i64),
    Double(u64),
    String(Vec<u8>),
    UnparsedString(Vec<u8>),
    /// Canonical row-format bytes of a value outside the matrix.
    ///
    /// Equal exactly when the values are, for two arrays of one type, which is
    /// the only pairing the `Opaque` domain admits.
    Opaque(Vec<u8>),
}

impl CanonicalValue {
    pub(crate) fn invalid_key(&self) -> bool {
        matches!(self, Self::Null)
            || matches!(self, Self::Double(bits) if f64::from_bits(*bits).is_nan())
    }
}

impl ComparisonPlan {
    /// Plan the comparison for a pair of column types, where one exists.
    ///
    /// The four normalized types compare across the whole matrix. Outside it,
    /// identity of type is the whole of comparability: a pair of one admitted
    /// type gets the `Opaque` domain, and every other pair — opaque against a
    /// different opaque, opaque against a normalized type — has no plan and is
    /// declined wherever an identity would be claimed.
    pub(crate) fn new(old: &DataType, new: &DataType) -> Option<Self> {
        let old_kind = kind(old);
        let new_kind = kind(new);
        let domain = match (old_kind, new_kind) {
            (Kind::Boolean, Kind::Boolean) => Domain::Boolean,
            (Kind::Int, Kind::Int) => Domain::Int,
            (Kind::Double, Kind::Double) => Domain::Double,
            (Kind::String, Kind::String) => Domain::String,
            (Kind::Int, Kind::Double) | (Kind::Double, Kind::Int) => Domain::IntDouble,
            (Kind::Boolean, Kind::Int) | (Kind::Int, Kind::Boolean) => Domain::BoolInt,
            (Kind::Boolean, Kind::Double) | (Kind::Double, Kind::Boolean) => Domain::BoolDouble,
            (Kind::String, Kind::Boolean) | (Kind::Boolean, Kind::String) => Domain::StringBoolean,
            (Kind::String, Kind::Int) | (Kind::Int, Kind::String) => Domain::StringInt,
            (Kind::String, Kind::Double) | (Kind::Double, Kind::String) => Domain::StringDouble,
            (Kind::Opaque, Kind::Opaque) if old == new => Domain::Opaque,
            _ => return None,
        };
        Some(Self {
            old: old_kind,
            new: new_kind,
            domain,
        })
    }

    pub(crate) fn canonicalize_old(&self, values: &dyn Array) -> Vec<CanonicalValue> {
        self.canonicalize(values, self.old)
    }

    pub(crate) fn canonicalize_new(&self, values: &dyn Array) -> Vec<CanonicalValue> {
        self.canonicalize(values, self.new)
    }

    fn canonicalize(&self, values: &dyn Array, kind: Kind) -> Vec<CanonicalValue> {
        if kind == Kind::Opaque {
            return opaque_values(values);
        }
        raw_values(values, kind)
            .into_iter()
            .map(|value| canonicalize(value, self.domain))
            .collect()
    }
}

/// Encode a column outside the matrix as canonical row-format bytes.
///
/// The converter is built per side from the column's own type. The two sides'
/// types are identical whenever a plan exists, and the row encoding hydrates
/// dictionaries to their underlying values, so equal values arrive at equal
/// bytes with no state shared between the sides. Nulls are taken out first so
/// the null rules stay the matrix's own.
fn opaque_values(values: &dyn Array) -> Vec<CanonicalValue> {
    let converter = RowConverter::new(vec![SortField::new(values.data_type().clone())])
        .expect("validate_tables admits only encodable types");
    let rows = converter
        .convert_columns(&[make_array(values.to_data())])
        .expect("the converter was built for this column's own type");
    // Logical nulls rather than `is_null`, which for a dictionary reads only
    // the key buffer: a valid key pointing at a null value is a null of the
    // column, and it has to reach `CanonicalValue::Null` like every other so
    // the null rules stay uniform.
    let nulls = values.logical_nulls();
    (0..values.len())
        .map(|row| {
            if nulls.as_ref().is_some_and(|nulls| nulls.is_null(row)) {
                CanonicalValue::Null
            } else {
                CanonicalValue::Opaque(rows.row(row).as_ref().to_vec())
            }
        })
        .collect()
}

#[derive(Clone, Debug)]
enum RawValue {
    Null,
    Boolean(bool),
    Int(i64),
    Double(f64),
    String(Vec<u8>),
}

fn kind(data_type: &DataType) -> Kind {
    match data_type {
        DataType::Boolean => Kind::Boolean,
        DataType::Int8
        | DataType::Int16
        | DataType::Int32
        | DataType::Int64
        | DataType::UInt8
        | DataType::UInt16
        | DataType::UInt32
        | DataType::UInt64 => Kind::Int,
        DataType::Float32 | DataType::Float64 => Kind::Double,
        DataType::Utf8 | DataType::LargeUtf8 => Kind::String,
        DataType::Dictionary(_, value)
            if matches!(value.as_ref(), DataType::Utf8 | DataType::LargeUtf8) =>
        {
            Kind::String
        }
        _ => Kind::Opaque,
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
        Kind::Opaque => unreachable!("opaque columns canonicalize as row bytes"),
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
        // Encoding equality, not truthiness: `true` is `1` and `false` is `0`,
        // exactly, so every other number stays unequal to both.
        RawValue::Boolean(value) => match domain {
            Domain::BoolInt | Domain::BoolDouble => CanonicalValue::Int(i64::from(value)),
            _ => CanonicalValue::Boolean(value),
        },
        RawValue::Int(value) => match domain {
            Domain::Double | Domain::StringDouble => canonical_double(value as f64),
            _ => CanonicalValue::Int(value),
        },
        RawValue::Double(value) => match domain {
            Domain::IntDouble | Domain::BoolDouble => exact_double_to_i64(value)
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

/// Hash an ordered sequence of values.
///
/// Lengths are written before the values they describe, so no two sequences
/// share a byte encoding. Key tuples and whole columns are both sequences, and
/// hashing them the same way keeps row identity and column identity on one
/// definition of equality.
pub(crate) fn sequence_hash(values: &[CanonicalValue]) -> u128 {
    let mut bytes = Vec::with_capacity(8 + values.len() * 24);
    bytes.extend_from_slice(&(values.len() as u64).to_le_bytes());
    for value in values {
        let hash = stable_hash(value).to_le_bytes();
        bytes.extend_from_slice(&(hash.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&hash);
    }
    xxh3_128_with_seed(&bytes, 0)
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
        CanonicalValue::Opaque(value) => encode_bytes(6, value, &mut bytes),
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
    fn every_pair_of_normalized_types_is_comparable() {
        use arrow_schema::DataType::{Boolean, Float64, Int64, Utf8};

        for old in [Boolean, Int64, Float64, Utf8] {
            for new in [Boolean, Int64, Float64, Utf8] {
                assert!(ComparisonPlan::new(&old, &new).is_some(), "{old:?}/{new:?}");
            }
        }
    }

    #[test]
    fn outside_the_matrix_only_the_identical_type_has_a_plan() {
        use arrow_schema::{DataType, TimeUnit};

        let date = DataType::Date32;
        let aware = DataType::Timestamp(TimeUnit::Millisecond, Some("UTC".into()));
        let naive = DataType::Timestamp(TimeUnit::Millisecond, None);

        assert!(ComparisonPlan::new(&date, &date).is_some());
        assert!(ComparisonPlan::new(&aware, &aware).is_some());

        // A different opaque type, a different unit's worth of the same
        // family, and a normalized type are all incomparable: identity of type
        // is the whole of comparability out here.
        assert!(ComparisonPlan::new(&date, &aware).is_none());
        assert!(ComparisonPlan::new(&aware, &naive).is_none());
        assert!(ComparisonPlan::new(&date, &DataType::Int64).is_none());
        assert!(ComparisonPlan::new(&DataType::Int32, &date).is_none());
    }

    #[test]
    fn opaque_values_compare_and_hash_by_their_encoding() {
        let (old, new) = values(
            column!(date32[Some(1), Some(2), None, None]),
            column!(date32[Some(1), Some(3), None, Some(4)]),
        );
        assert_eq!(old[0], new[0]);
        assert_ne!(old[1], new[1]);
        // Nulls leave the encoding before it happens, so the null rules stay
        // the matrix's own: null agrees with null and disagrees with present.
        assert_eq!(old[2], new[2]);
        assert_eq!(old[2], CanonicalValue::Null);
        assert_ne!(old[3], new[3]);

        assert_eq!(stable_hash(&old[0]), stable_hash(&new[0]));
        assert_ne!(stable_hash(&old[1]), stable_hash(&new[1]));
    }

    #[test]
    fn a_valid_key_pointing_at_a_null_dictionary_value_is_null() {
        use arrow_array::Int64Array;

        // The null lives in the dictionary's values, behind a valid key, so
        // only the logical null mask can see it; reading the key buffer alone
        // would encode it as an opaque value and let it slip past the null
        // rules, key invalidation included.
        let hidden = DictionaryArray::<Int8Type>::try_new(
            Int8Array::from(vec![0, 1]),
            std::sync::Arc::new(Int64Array::from(vec![Some(10), None])),
        )
        .unwrap();
        let keyed = DictionaryArray::<Int8Type>::try_new(
            Int8Array::from(vec![Some(0), None]),
            std::sync::Arc::new(Int64Array::from(vec![10])),
        )
        .unwrap();

        let (old, new) = values(std::sync::Arc::new(hidden), std::sync::Arc::new(keyed));
        assert_eq!(old, new);
        assert_eq!(old[1], CanonicalValue::Null);
        assert!(old[1].invalid_key());
    }

    #[test]
    fn differently_interned_dictionaries_encode_equal_values_equally() {
        use arrow_array::Int64Array;

        // The same logical values with the dictionaries built in opposite
        // orders, so equal bytes can only come from the encoding hydrating
        // values rather than reading keys. This property is load-bearing:
        // each side builds its own converter, and nothing else makes their
        // bytes agree.
        let old = DictionaryArray::<Int8Type>::try_new(
            Int8Array::from(vec![0, 1]),
            std::sync::Arc::new(Int64Array::from(vec![10, 20])),
        )
        .unwrap();
        let new = DictionaryArray::<Int8Type>::try_new(
            Int8Array::from(vec![1, 0]),
            std::sync::Arc::new(Int64Array::from(vec![20, 10])),
        )
        .unwrap();

        let (old, new) = values(std::sync::Arc::new(old), std::sync::Arc::new(new));
        assert_eq!(old, new);
        assert!(matches!(old[0], CanonicalValue::Opaque(_)));
    }

    #[test]
    fn booleans_compare_as_their_exact_encoding() {
        let (old, new) = values(
            column!([true, false, true, true, true]),
            column!([1, 0, 2, -1, i64::MIN]),
        );
        assert_eq!(old[0], new[0]);
        assert_eq!(old[1], new[1]);
        assert_ne!(old[2], new[2]);
        assert_ne!(old[3], new[3]);
        assert_ne!(old[4], new[4]);

        let (old, new) = values(
            column!([true, false, true, false, true]),
            column!([1.0, -0.0, 1.5, f64::NAN, f64::INFINITY]),
        );
        assert_eq!(old[0], new[0]);
        assert_eq!(old[1], new[1]);
        assert_ne!(old[2], new[2]);
        assert_ne!(old[3], new[3]);
        assert_ne!(old[4], new[4]);
    }

    #[test]
    fn nulls_follow_the_usual_rules_in_the_boolean_domains() {
        let (old, new) = values(
            column!([None::<bool>, Some(true)]),
            column!([None::<i64>, None]),
        );
        assert_eq!(old[0], new[0]);
        assert_ne!(old[1], new[1]);
    }

    #[test]
    fn equality_does_not_compose_across_type_triangles() {
        // Each pair compares under its own domain, and the corners disagree:
        // "true" equals true equals 1, but "true" is not 1, and "1" equals 1
        // equals true, but "1" is not true. Deliberate — each string domain
        // parses by its partner's spelling rules — and pinned so the
        // incoherence stays a decision rather than becoming a surprise.
        let (old, new) = values(column!(["true", "1"]), column!([true, true]));
        assert_eq!(old[0], new[0]);
        assert_ne!(old[1], new[1]);

        let (old, new) = values(column!(["true", "1"]), column!([1, 1]));
        assert_ne!(old[0], new[0]);
        assert_eq!(old[1], new[1]);

        let (old, new) = values(column!([true, true]), column!([1, 1]));
        assert_eq!(old[0], new[0]);
        assert_eq!(old[1], new[1]);
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
