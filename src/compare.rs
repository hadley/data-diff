use arrow_array::{
    Array, BooleanArray, Date32Array, Date64Array, Decimal128Array, Decimal256Array, Float32Array,
    Float64Array, Int8Array, Int16Array, Int32Array, Int64Array, LargeStringArray, StringArray,
    TimestampMicrosecondArray, TimestampMillisecondArray, TimestampNanosecondArray,
    TimestampSecondArray, UInt8Array, UInt16Array, UInt32Array, UInt64Array, make_array,
};
use arrow_buffer::i256;
use arrow_cast::cast;
use arrow_row::{RowConverter, SortField};
use arrow_schema::{DataType, TimeUnit};
use xxhash_rust::xxh3::{Xxh3 as Xxh3State, xxh3_128_with_seed};

/// Whether a timestamp type carries a timezone.
///
/// An aware timestamp names an instant — Arrow stores its value as a UTC epoch
/// offset and keeps the timezone string as presentation metadata — while a
/// naive one names a calendar reading. The two are different claims, so
/// awareness divides the timestamp kind and no domain crosses the divide.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Awareness {
    Aware,
    Naive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Kind {
    Boolean,
    Int,
    Double,
    String,
    Timestamp(Awareness),
    Date,
    Decimal,
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
    Timestamp,
    Date,
    DateTimestamp,
    Decimal,
    DecimalInt,
    DecimalDouble,
    StringDate,
    StringNaiveTimestamp,
    StringAwareTimestamp,
    StringDecimal,
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
    /// A temporal value in nanoseconds, held wide enough that no unit
    /// conversion can overflow: a UTC epoch offset for an aware timestamp, a
    /// wall-clock reading for a naive one, and midnight-based values for
    /// dates.
    Nanos(i128),
    /// An exact rational, the canonical form of the decimal domains.
    Rational(Rational),
    /// Canonical row-format bytes of a value outside the matrix.
    ///
    /// Equal exactly when the values are, for two arrays of one type, which is
    /// the only pairing the `Opaque` domain admits.
    Opaque(Vec<u8>),
}

/// An exact rational in the canonical form m · 2ᵃ · 5ᵇ with m coprime to 10.
///
/// Every decimal value and every finite double reduces to exactly one such
/// triple, so equality of triples is equality of values across precision,
/// scale, 128/256 width, and binary-versus-decimal representation. The
/// mantissa is an [`i256`] stored as little-endian bytes, which every reduced
/// decimal and double mantissa fits; exponents are held separately rather
/// than multiplied out, so no construction can overflow.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub(crate) struct Rational {
    mantissa: [u8; 32],
    exp2: i32,
    exp5: i32,
}

impl Rational {
    /// The canonical triple of mantissa × 10^exponent10, or `None` where the
    /// exponents overflow an `i32`.
    ///
    /// Only a parsed string can reach the overflow — a decimal's exponents
    /// are bounded by its `i8` scale and the mantissa's bit width, a double's
    /// by its own exponent field — and such a string names a value no decimal
    /// column can hold, so `None` is a mismatch and never a lost value.
    fn new(mut mantissa: i256, exponent10: i32) -> Option<Self> {
        if mantissa == i256::ZERO {
            return Some(Self {
                mantissa: i256::ZERO.to_le_bytes(),
                exp2: 0,
                exp5: 0,
            });
        }
        let twos = strip_factor(&mut mantissa, i256::from_i128(2));
        let fives = strip_factor(&mut mantissa, i256::from_i128(5));
        Some(Self {
            mantissa: mantissa.to_le_bytes(),
            exp2: twos.checked_add(exponent10)?,
            exp5: fives.checked_add(exponent10)?,
        })
    }

    /// The value as an `i64`, where it is exactly one.
    fn to_i64(self) -> Option<i64> {
        if self.exp2 < 0 || self.exp5 < 0 {
            return None;
        }
        let mut value = i256::from_le_bytes(self.mantissa);
        for _ in 0..self.exp2 {
            value = value.checked_mul(i256::from_i128(2))?;
        }
        for _ in 0..self.exp5 {
            value = value.checked_mul(i256::from_i128(5))?;
        }
        i64::try_from(value.to_i128()?).ok()
    }
}

/// Divide out every factor of `factor` and return how many there were.
fn strip_factor(value: &mut i256, factor: i256) -> i32 {
    let mut count = 0;
    loop {
        let quotient = value.wrapping_div(factor);
        if quotient.wrapping_mul(factor) == *value {
            *value = quotient;
            count += 1;
        } else {
            return count;
        }
    }
}

/// The exact rational a finite double is; `None` for `NaN` and infinities.
fn rational_of_double(value: f64) -> Option<Rational> {
    if !value.is_finite() {
        return None;
    }
    if value == 0.0 {
        return Rational::new(i256::ZERO, 0);
    }
    let bits = value.to_bits();
    let fraction = (bits & ((1 << 52) - 1)) as i128;
    let exponent = ((bits >> 52) & 0x7ff) as i32;
    // Subnormals have no implicit bit and share the minimum exponent.
    let (significand, exp2) = if exponent == 0 {
        (fraction, -1074)
    } else {
        (fraction | (1 << 52), exponent - 1075)
    };
    let significand = if value < 0.0 {
        -significand
    } else {
        significand
    };
    let base = Rational::new(i256::from_i128(significand), 0)
        .expect("a bare significand strips at most its own bit width");
    Some(Rational {
        mantissa: base.mantissa,
        // Bounded by the double's own exponent field plus the significand's
        // 53 bits, far inside an `i32`.
        exp2: base.exp2 + exp2,
        exp5: base.exp5,
    })
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
    /// The four normalized types compare across the whole matrix, and the
    /// promoted families — timestamps, dates, decimals — compare by the
    /// decided rules recorded in the design: within a family by exact value,
    /// against the matrix where a row below says so, and against ISO 8601 or
    /// exact numeric strings. Outside the decided matrix, identity of type is
    /// the whole of comparability: a pair of one admitted type gets the
    /// `Opaque` domain, and every other pair — aware against naive, date
    /// against an aware timestamp, opaque against anything else — has no plan
    /// and is declined wherever values would be measured.
    pub(crate) fn new(old: &DataType, new: &DataType) -> Option<Self> {
        use Awareness::{Aware, Naive};

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
            (Kind::Timestamp(old), Kind::Timestamp(new)) if old == new => Domain::Timestamp,
            (Kind::Date, Kind::Date) => Domain::Date,
            (Kind::Date, Kind::Timestamp(Naive)) | (Kind::Timestamp(Naive), Kind::Date) => {
                Domain::DateTimestamp
            }
            (Kind::Decimal, Kind::Decimal) => Domain::Decimal,
            (Kind::Decimal, Kind::Int) | (Kind::Int, Kind::Decimal) => Domain::DecimalInt,
            (Kind::Decimal, Kind::Double) | (Kind::Double, Kind::Decimal) => Domain::DecimalDouble,
            (Kind::String, Kind::Date) | (Kind::Date, Kind::String) => Domain::StringDate,
            (Kind::String, Kind::Timestamp(Naive)) | (Kind::Timestamp(Naive), Kind::String) => {
                Domain::StringNaiveTimestamp
            }
            (Kind::String, Kind::Timestamp(Aware)) | (Kind::Timestamp(Aware), Kind::String) => {
                Domain::StringAwareTimestamp
            }
            (Kind::String, Kind::Decimal) | (Kind::Decimal, Kind::String) => Domain::StringDecimal,
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

/// Same-type equality straight off the arrow arrays.
///
/// A pair is eligible when the canonical equality verdict is a pure function
/// of the raw values — canonicalization restricted to the type is injective
/// up to the normalization the comparator applies inline — and each arm below
/// states its argument. Every type without one returns `None` and keeps the
/// canonicalizing path: dictionaries for their hydration and logical-null
/// subtleties, opaque columns whose canonical form *is* the comparison, and
/// every cross-type pair. This is an optimization, never a mode — the answer
/// is the canonical answer, produced without materializing it.
///
/// Null equals null and differs from every value, exactly as canonical
/// `Null` does.
pub(crate) struct NativeEq<'a>(Box<dyn Fn(usize, usize) -> bool + 'a>);

impl<'a> NativeEq<'a> {
    pub(crate) fn for_pair(old: &'a dyn Array, new: &'a dyn Array) -> Option<Self> {
        if old.data_type() != new.data_type() {
            return None;
        }
        Some(match old.data_type() {
            // A boolean canonicalizes as itself against a boolean.
            DataType::Boolean => {
                native::<BooleanArray>(old, new, |a, i, b, j| a.value(i) == b.value(j))
            }
            // Every integer width maps bijectively into the canonical i64 —
            // u64 by wrapping cast — so raw equality is canonical equality.
            DataType::Int8 => native::<Int8Array>(old, new, |a, i, b, j| a.value(i) == b.value(j)),
            DataType::Int16 => {
                native::<Int16Array>(old, new, |a, i, b, j| a.value(i) == b.value(j))
            }
            DataType::Int32 => {
                native::<Int32Array>(old, new, |a, i, b, j| a.value(i) == b.value(j))
            }
            DataType::Int64 => {
                native::<Int64Array>(old, new, |a, i, b, j| a.value(i) == b.value(j))
            }
            DataType::UInt8 => {
                native::<UInt8Array>(old, new, |a, i, b, j| a.value(i) == b.value(j))
            }
            DataType::UInt16 => {
                native::<UInt16Array>(old, new, |a, i, b, j| a.value(i) == b.value(j))
            }
            DataType::UInt32 => {
                native::<UInt32Array>(old, new, |a, i, b, j| a.value(i) == b.value(j))
            }
            DataType::UInt64 => {
                native::<UInt64Array>(old, new, |a, i, b, j| a.value(i) == b.value(j))
            }
            // Doubles are compared by their normalized bits: raw bits alone
            // would wrongly split -0.0 from 0.0 and NaN payloads from each
            // other, so the canonical normalization is applied inline.
            DataType::Float32 => native::<Float32Array>(old, new, |a, i, b, j| {
                canonical_double_bits(f64::from(a.value(i)))
                    == canonical_double_bits(f64::from(b.value(j)))
            }),
            DataType::Float64 => native::<Float64Array>(old, new, |a, i, b, j| {
                canonical_double_bits(a.value(i)) == canonical_double_bits(b.value(j))
            }),
            // A string keeps its bytes against a string.
            DataType::Utf8 => {
                native::<StringArray>(old, new, |a, i, b, j| a.value(i) == b.value(j))
            }
            DataType::LargeUtf8 => {
                native::<LargeStringArray>(old, new, |a, i, b, j| a.value(i) == b.value(j))
            }
            // Equal data types share the unit and awareness, and the exact
            // scaling into canonical nanoseconds is injective, so raw equality
            // is canonical equality.
            DataType::Timestamp(TimeUnit::Second, _) => {
                native::<TimestampSecondArray>(old, new, |a, i, b, j| a.value(i) == b.value(j))
            }
            DataType::Timestamp(TimeUnit::Millisecond, _) => {
                native::<TimestampMillisecondArray>(old, new, |a, i, b, j| a.value(i) == b.value(j))
            }
            DataType::Timestamp(TimeUnit::Microsecond, _) => {
                native::<TimestampMicrosecondArray>(old, new, |a, i, b, j| a.value(i) == b.value(j))
            }
            DataType::Timestamp(TimeUnit::Nanosecond, _) => {
                native::<TimestampNanosecondArray>(old, new, |a, i, b, j| a.value(i) == b.value(j))
            }
            DataType::Date32 => {
                native::<Date32Array>(old, new, |a, i, b, j| a.value(i) == b.value(j))
            }
            DataType::Date64 => {
                native::<Date64Array>(old, new, |a, i, b, j| a.value(i) == b.value(j))
            }
            // At one fixed scale, mantissa equality is value equality.
            DataType::Decimal128(_, _) => {
                native::<Decimal128Array>(old, new, |a, i, b, j| a.value(i) == b.value(j))
            }
            DataType::Decimal256(_, _) => {
                native::<Decimal256Array>(old, new, |a, i, b, j| a.value(i) == b.value(j))
            }
            _ => return None,
        })
    }

    pub(crate) fn equal(&self, old_row: usize, new_row: usize) -> bool {
        (self.0)(old_row, new_row)
    }
}

fn native<'a, A: Array + 'static>(
    old: &'a dyn Array,
    new: &'a dyn Array,
    equal: impl Fn(&A, usize, &A, usize) -> bool + 'a,
) -> NativeEq<'a> {
    let old = old
        .as_any()
        .downcast_ref::<A>()
        .expect("dispatched on the data type");
    let new = new
        .as_any()
        .downcast_ref::<A>()
        .expect("dispatched on the data type");
    NativeEq(Box::new(move |old_row, new_row| {
        match (old.is_null(old_row), new.is_null(new_row)) {
            (true, true) => true,
            (false, false) => equal(old, old_row, new, new_row),
            _ => false,
        }
    }))
}

/// Per-row canonical digests straight off an arrow column, without
/// materializing a `CanonicalValue` per value.
///
/// Eligible only under the column type's own identity plan — a cross-type
/// plan canonicalizes into the pair's shared domain, which the raw value
/// alone does not determine — and only for the types [`NativeEq`] admits, on
/// the same arguments. The digests are bit-identical to hashing the
/// materialized values: the fixed-size variants construct the very
/// `CanonicalValue` on the stack and hash it through the one shared encoding,
/// and strings hash the same byte frame from borrowed bytes.
pub(crate) struct NativeHasher<'a>(Box<dyn Fn(usize) -> u128 + 'a>);

impl<'a> NativeHasher<'a> {
    pub(crate) fn for_column(values: &'a dyn Array, plan: ComparisonPlan) -> Option<Self> {
        if ComparisonPlan::new(values.data_type(), values.data_type()) != Some(plan) {
            return None;
        }
        Some(match values.data_type() {
            DataType::Boolean => hashing::<BooleanArray>(values, |a, row| {
                stable_hash(&CanonicalValue::Boolean(a.value(row)))
            }),
            DataType::Int8 => hashing::<Int8Array>(values, |a, row| {
                stable_hash(&CanonicalValue::Int(i64::from(a.value(row))))
            }),
            DataType::Int16 => hashing::<Int16Array>(values, |a, row| {
                stable_hash(&CanonicalValue::Int(i64::from(a.value(row))))
            }),
            DataType::Int32 => hashing::<Int32Array>(values, |a, row| {
                stable_hash(&CanonicalValue::Int(i64::from(a.value(row))))
            }),
            DataType::Int64 => hashing::<Int64Array>(values, |a, row| {
                stable_hash(&CanonicalValue::Int(a.value(row)))
            }),
            DataType::UInt8 => hashing::<UInt8Array>(values, |a, row| {
                stable_hash(&CanonicalValue::Int(i64::from(a.value(row))))
            }),
            DataType::UInt16 => hashing::<UInt16Array>(values, |a, row| {
                stable_hash(&CanonicalValue::Int(i64::from(a.value(row))))
            }),
            DataType::UInt32 => hashing::<UInt32Array>(values, |a, row| {
                stable_hash(&CanonicalValue::Int(i64::from(a.value(row))))
            }),
            DataType::UInt64 => hashing::<UInt64Array>(values, |a, row| {
                stable_hash(&CanonicalValue::Int(a.value(row) as i64))
            }),
            DataType::Float32 => hashing::<Float32Array>(values, |a, row| {
                stable_hash(&canonical_double(f64::from(a.value(row))))
            }),
            DataType::Float64 => hashing::<Float64Array>(values, |a, row| {
                stable_hash(&canonical_double(a.value(row)))
            }),
            DataType::Utf8 => hashing::<StringArray>(values, |a, row| {
                stable_hash_borrowed_string(a.value(row).as_bytes())
            }),
            DataType::LargeUtf8 => hashing::<LargeStringArray>(values, |a, row| {
                stable_hash_borrowed_string(a.value(row).as_bytes())
            }),
            // The factors mirror `temporal_values` exactly, and the per-type
            // digest tests hold the two to the same values.
            DataType::Timestamp(TimeUnit::Second, _) => {
                hashing::<TimestampSecondArray>(values, |a, row| {
                    stable_hash(&CanonicalValue::Nanos(
                        i128::from(a.value(row)) * 1_000_000_000,
                    ))
                })
            }
            DataType::Timestamp(TimeUnit::Millisecond, _) => {
                hashing::<TimestampMillisecondArray>(values, |a, row| {
                    stable_hash(&CanonicalValue::Nanos(i128::from(a.value(row)) * 1_000_000))
                })
            }
            DataType::Timestamp(TimeUnit::Microsecond, _) => {
                hashing::<TimestampMicrosecondArray>(values, |a, row| {
                    stable_hash(&CanonicalValue::Nanos(i128::from(a.value(row)) * 1_000))
                })
            }
            DataType::Timestamp(TimeUnit::Nanosecond, _) => {
                hashing::<TimestampNanosecondArray>(values, |a, row| {
                    stable_hash(&CanonicalValue::Nanos(i128::from(a.value(row))))
                })
            }
            DataType::Date32 => hashing::<Date32Array>(values, |a, row| {
                stable_hash(&CanonicalValue::Nanos(
                    i128::from(a.value(row)) * NANOS_PER_DAY,
                ))
            }),
            DataType::Date64 => hashing::<Date64Array>(values, |a, row| {
                stable_hash(&CanonicalValue::Nanos(i128::from(a.value(row)) * 1_000_000))
            }),
            DataType::Decimal128(_, scale) => {
                let scale = *scale;
                hashing::<Decimal128Array>(values, move |a, row| {
                    stable_hash(&CanonicalValue::Rational(
                        Rational::new(i256::from_i128(a.value(row)), -i32::from(scale))
                            .expect("a decimal's exponents are bounded by its i8 scale"),
                    ))
                })
            }
            DataType::Decimal256(_, scale) => {
                let scale = *scale;
                hashing::<Decimal256Array>(values, move |a, row| {
                    stable_hash(&CanonicalValue::Rational(
                        Rational::new(a.value(row), -i32::from(scale))
                            .expect("a decimal's exponents are bounded by its i8 scale"),
                    ))
                })
            }
            _ => return None,
        })
    }

    pub(crate) fn hash(&self, row: usize) -> u128 {
        (self.0)(row)
    }
}

fn hashing<'a, A: Array + 'static>(
    values: &'a dyn Array,
    hash: impl Fn(&A, usize) -> u128 + 'a,
) -> NativeHasher<'a> {
    let array = values
        .as_any()
        .downcast_ref::<A>()
        .expect("dispatched on the data type");
    NativeHasher(Box::new(move |row| {
        if array.is_null(row) {
            stable_hash(&CanonicalValue::Null)
        } else {
            hash(array, row)
        }
    }))
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
    /// A temporal value already normalized to nanoseconds.
    Nanos(i128),
    /// A decimal's unscaled mantissa and its type's scale.
    Decimal(i256, i8),
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
        DataType::Timestamp(_, Some(_)) => Kind::Timestamp(Awareness::Aware),
        DataType::Timestamp(_, None) => Kind::Timestamp(Awareness::Naive),
        DataType::Date32 | DataType::Date64 => Kind::Date,
        DataType::Decimal128(_, _) | DataType::Decimal256(_, _) => Kind::Decimal,
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
        Kind::Timestamp(_) | Kind::Date => temporal_values(values),
        Kind::Decimal => decimal_values(values),
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

/// Nanoseconds in one day, the factor a `Date32` value scales by.
const NANOS_PER_DAY: i128 = 86_400_000_000_000;

/// Read a temporal column as nanoseconds.
///
/// Every unit widens into `i128` nanoseconds exactly — the extreme `i64`
/// seconds value is ~9.2e27 nanoseconds against an `i128` range of ~1.7e38 —
/// so the conversion is total and overflow is unrepresentable rather than
/// checked. A `Date32` scales from days and a `Date64` from milliseconds,
/// which lands both spellings of one calendar day on one value.
fn temporal_values(values: &dyn Array) -> Vec<RawValue> {
    macro_rules! nanos {
        ($array:ty, $factor:expr) => {
            primitive_values::<$array, _>(values, |array, row| {
                RawValue::Nanos(i128::from(array.value(row)) * $factor)
            })
        };
    }
    match values.data_type() {
        DataType::Timestamp(TimeUnit::Second, _) => nanos!(TimestampSecondArray, 1_000_000_000),
        DataType::Timestamp(TimeUnit::Millisecond, _) => {
            nanos!(TimestampMillisecondArray, 1_000_000)
        }
        DataType::Timestamp(TimeUnit::Microsecond, _) => nanos!(TimestampMicrosecondArray, 1_000),
        DataType::Timestamp(TimeUnit::Nanosecond, _) => nanos!(TimestampNanosecondArray, 1),
        DataType::Date32 => nanos!(Date32Array, NANOS_PER_DAY),
        DataType::Date64 => nanos!(Date64Array, 1_000_000),
        _ => unreachable!("validated temporal type"),
    }
}

fn decimal_values(values: &dyn Array) -> Vec<RawValue> {
    match values.data_type() {
        DataType::Decimal128(_, scale) => {
            let scale = *scale;
            primitive_values::<Decimal128Array, _>(values, move |array, row| {
                RawValue::Decimal(i256::from_i128(array.value(row)), scale)
            })
        }
        DataType::Decimal256(_, scale) => {
            let scale = *scale;
            primitive_values::<Decimal256Array, _>(values, move |array, row| {
                RawValue::Decimal(array.value(row), scale)
            })
        }
        _ => unreachable!("validated decimal type"),
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
            // A finite double is an exact rational; `NaN` and the infinities
            // keep the double canonicalization, so `NaN` agrees with `NaN`
            // and still invalidates keys, and nothing equals a decimal.
            Domain::DecimalDouble => rational_of_double(value)
                .map(CanonicalValue::Rational)
                .unwrap_or_else(|| canonical_double(value)),
            _ => canonical_double(value),
        },
        // Every temporal domain shares one canonical form, so the nanoseconds
        // pass through: cross-unit, cross-spelling, and date-at-midnight
        // equality are all equality of this value.
        RawValue::Nanos(value) => CanonicalValue::Nanos(value),
        RawValue::Decimal(mantissa, scale) => {
            let value = Rational::new(mantissa, -i32::from(scale))
                .expect("a decimal's exponents are bounded by its i8 scale and 256 bits");
            match domain {
                // The `IntDouble` precedent: a decimal that is exactly an
                // integer meets the integer in its own form, and any other
                // decimal keeps a form no integer has.
                Domain::DecimalInt => value
                    .to_i64()
                    .map(CanonicalValue::Int)
                    .unwrap_or(CanonicalValue::Rational(value)),
                _ => CanonicalValue::Rational(value),
            }
        }
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
            Domain::StringDate => parse_bytes(&value, |value| {
                parse_iso_date(value).map(CanonicalValue::Nanos)
            }),
            Domain::StringNaiveTimestamp => parse_bytes(&value, |value| {
                parse_iso_naive(value).map(CanonicalValue::Nanos)
            }),
            Domain::StringAwareTimestamp => parse_bytes(&value, |value| {
                parse_iso_instant(value).map(CanonicalValue::Nanos)
            }),
            Domain::StringDecimal => parse_bytes(&value, |value| {
                parse_exact_rational(value).map(CanonicalValue::Rational)
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
    CanonicalValue::Double(canonical_double_bits(value))
}

/// The normalized bit pattern doubles are compared by: every NaN collapses to
/// one payload and both zeros to one, so bit equality is value equality under
/// the canonical rules that NaN agrees with NaN and `-0.0` with `0.0`.
fn canonical_double_bits(value: f64) -> u64 {
    if value.is_nan() {
        f64::NAN.to_bits()
    } else if value == 0.0 {
        0
    } else {
        value.to_bits()
    }
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

/// Parse a decimal string exactly, to the canonical rational form.
///
/// The grammar is the exact numeric parser's — sign, digits, optional single
/// fraction, optional exponent — read into a rational rather than through an
/// `i64` or a float, so `"1.50"` is exactly 1.5 and `"1e2"` exactly 100.
/// Trailing zeros leave the digits before accumulation, so every value any
/// decimal column can hold is reachable; a string with more significant
/// digits than an `i256` holds equals no decimal and fails the parse.
fn parse_exact_rational(value: &str) -> Option<Rational> {
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
        Some(index) => (&rest[..index], parse_exponent(&rest[index + 1..])?),
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
        return Rational::new(i256::ZERO, 0);
    }
    let mut scale = exponent.checked_sub(i64::try_from(fraction.len()).ok()?)?;
    while digits.last() == Some(&b'0') {
        digits.pop();
        scale = scale.checked_add(1)?;
    }
    let digits = &digits[digits.iter().position(|digit| *digit != b'0')?..];

    let mut magnitude = i256::ZERO;
    for digit in digits {
        magnitude = magnitude
            .checked_mul(i256::from_i128(10))?
            .checked_add(i256::from_i128(i128::from(digit - b'0')))?;
    }
    let mantissa = if negative {
        i256::ZERO.checked_sub(magnitude)?
    } else {
        magnitude
    };
    // An exponent near the `i32` edge survives to here but overflows the
    // canonical triple, which no decimal's exponents can; `Rational::new`
    // declines it, and the string is a mismatch like any other parse failure.
    Rational::new(mantissa, i32::try_from(scale).ok()?)
}

/// Parse the ISO 8601 extended calendar date, `YYYY-MM-DD`, to midnight.
fn parse_iso_date(value: &str) -> Option<i128> {
    parse_civil_days(value.as_bytes()).map(|days| i128::from(days) * NANOS_PER_DAY)
}

/// Parse the ISO 8601 unzoned date-time, `YYYY-MM-DDTHH:MM:SS[.f…]`.
fn parse_iso_naive(value: &str) -> Option<i128> {
    let bytes = value.as_bytes();
    let (nanos, consumed) = parse_wall_nanos(bytes)?;
    (consumed == bytes.len()).then_some(nanos)
}

/// Parse the ISO 8601 instant: the unzoned date-time with a required `Z` or
/// `±HH:MM` offset, normalized to UTC.
fn parse_iso_instant(value: &str) -> Option<i128> {
    let bytes = value.as_bytes();
    let (wall, consumed) = parse_wall_nanos(bytes)?;
    let offset = parse_offset_seconds(&bytes[consumed..])?;
    Some(wall - i128::from(offset) * 1_000_000_000)
}

/// Parse `YYYY-MM-DD` — the whole of `bytes` — to days since the epoch.
fn parse_civil_days(bytes: &[u8]) -> Option<i64> {
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let year = fixed_digits(&bytes[0..4])?;
    let month = fixed_digits(&bytes[5..7])?;
    let day = fixed_digits(&bytes[8..10])?;
    if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
        return None;
    }
    Some(days_from_civil(i64::from(year), month, day))
}

/// Parse a leading `YYYY-MM-DDTHH:MM:SS[.f…]` to wall-clock nanoseconds and
/// the number of bytes consumed, leaving any offset for the caller.
fn parse_wall_nanos(bytes: &[u8]) -> Option<(i128, usize)> {
    if bytes.len() < 19 || bytes[10] != b'T' || bytes[13] != b':' || bytes[16] != b':' {
        return None;
    }
    let days = parse_civil_days(&bytes[..10])?;
    let hours = fixed_digits(&bytes[11..13])?;
    let minutes = fixed_digits(&bytes[14..16])?;
    let seconds = fixed_digits(&bytes[17..19])?;
    if hours > 23 || minutes > 59 || seconds > 59 {
        return None;
    }
    let (fraction, consumed) = if bytes.get(19) == Some(&b'.') {
        let digits = bytes[20..]
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .count();
        if !(1..=9).contains(&digits) {
            return None;
        }
        let value = fixed_digits(&bytes[20..20 + digits])?;
        (
            u64::from(value) * 10_u64.pow(9 - u32::try_from(digits).unwrap()),
            20 + digits,
        )
    } else {
        (0, 19)
    };

    let seconds_of_day = i128::from(hours) * 3_600 + i128::from(minutes) * 60 + i128::from(seconds);
    Some((
        i128::from(days) * NANOS_PER_DAY + seconds_of_day * 1_000_000_000 + i128::from(fraction),
        consumed,
    ))
}

/// Parse an offset designator — the whole of `bytes` — to seconds.
fn parse_offset_seconds(bytes: &[u8]) -> Option<i64> {
    match bytes {
        [b'Z'] => Some(0),
        [sign @ (b'+' | b'-'), rest @ ..] if rest.len() == 5 && rest[2] == b':' => {
            let hours = fixed_digits(&rest[0..2])?;
            let minutes = fixed_digits(&rest[3..5])?;
            // ISO 8601 offsets stop at ±14:00, with the minutes zero at the
            // boundary; anything past that names no timezone and no instant.
            if hours > 14 || minutes > 59 || (hours == 14 && minutes != 0) {
                return None;
            }
            let seconds = i64::from(hours) * 3_600 + i64::from(minutes) * 60;
            if *sign == b'-' {
                // ISO 8601 has no negative zero offset; RFC 3339 gives
                // `-00:00` a meaning ("offset unknown") that is precisely not
                // an instant, so it does not parse.
                (seconds != 0).then_some(-seconds)
            } else {
                Some(seconds)
            }
        }
        _ => None,
    }
}

/// Read a fixed-width run of ASCII digits.
fn fixed_digits(bytes: &[u8]) -> Option<u32> {
    if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let mut value = 0_u32;
    for digit in bytes {
        value = value
            .checked_mul(10)?
            .checked_add(u32::from(digit - b'0'))?;
    }
    Some(value)
}

fn leap_year(year: u32) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

/// Days from 1970-01-01 to the given civil date (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let month_index = i64::from(if month > 2 { month - 3 } else { month + 9 });
    let day_of_year = (153 * month_index + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
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

/// The buffered reference the streaming production path is tested against;
/// injectable so a collision can be forced, which only tests do.
#[cfg(test)]
pub(crate) trait StableHasher {
    fn hash(&self, bytes: &[u8]) -> u128;
}

#[cfg(test)]
pub(crate) struct Xxh3;

#[cfg(test)]
impl StableHasher for Xxh3 {
    fn hash(&self, bytes: &[u8]) -> u128 {
        xxh3_128_with_seed(bytes, 0)
    }
}

/// Where encoded bytes go. One `encode_value` serves every consumer — the
/// production hash streams, the injectable [`StableHasher`] path buffers — so
/// no two spellings of the encoding can drift apart.
trait Sink {
    fn write(&mut self, bytes: &[u8]);
}

impl Sink for Vec<u8> {
    fn write(&mut self, bytes: &[u8]) {
        self.extend_from_slice(bytes);
    }
}

impl Sink for Xxh3State {
    fn write(&mut self, bytes: &[u8]) {
        self.update(bytes);
    }
}

/// Encodings at most this long are hashed in one shot from the stack; only
/// longer ones pay for streaming state. Every fixed-size variant fits — the
/// largest is `Rational` at 41 bytes — so the split is really short strings
/// against long ones.
const INLINE: usize = 256;

/// A stack buffer for the one-shot path. Callers check [`encoded_len`] first;
/// the slice indexing turns a miscounted write into a panic rather than a
/// truncated hash.
struct Inline {
    buf: [u8; INLINE],
    len: usize,
}

impl Inline {
    fn new() -> Self {
        Self {
            buf: [0; INLINE],
            len: 0,
        }
    }

    fn bytes(&self) -> &[u8] {
        &self.buf[..self.len]
    }
}

impl Sink for Inline {
    fn write(&mut self, bytes: &[u8]) {
        self.buf[self.len..self.len + bytes.len()].copy_from_slice(bytes);
        self.len += bytes.len();
    }
}

pub(crate) fn stable_hash(value: &CanonicalValue) -> u128 {
    if encoded_len(value) <= INLINE {
        let mut inline = Inline::new();
        encode_value(value, &mut inline);
        xxh3_128_with_seed(inline.bytes(), 0)
    } else {
        let mut state = Xxh3State::with_seed(0);
        encode_value(value, &mut state);
        state.digest128()
    }
}

/// Hash an ordered sequence of values.
///
/// Lengths are written before the values they describe, so no two sequences
/// share a byte encoding. Key tuples and whole columns are both sequences, and
/// hashing them the same way keeps row identity and column identity on one
/// definition of equality.
pub(crate) fn sequence_hash(values: &[CanonicalValue]) -> u128 {
    sequence_hash_of(values.len(), values.iter().map(stable_hash))
}

/// The sequence digest from already-computed per-value hashes.
///
/// One frame writer serves this and [`sequence_hash`], which delegates here,
/// so a sequence of values and the stream of their hashes cannot digest
/// differently.
pub(crate) fn sequence_hash_of(count: usize, hashes: impl Iterator<Item = u128>) -> u128 {
    // The frame is 8 bytes of length plus 24 per value.
    if count <= (INLINE - 8) / 24 {
        let mut inline = Inline::new();
        encode_sequence(count, hashes, &mut inline);
        xxh3_128_with_seed(inline.bytes(), 0)
    } else {
        let mut state = Xxh3State::with_seed(0);
        encode_sequence(count, hashes, &mut state);
        state.digest128()
    }
}

fn encode_sequence(count: usize, hashes: impl Iterator<Item = u128>, sink: &mut impl Sink) {
    sink.write(&(count as u64).to_le_bytes());
    for hash in hashes {
        let hash = hash.to_le_bytes();
        sink.write(&(hash.len() as u64).to_le_bytes());
        sink.write(&hash);
    }
}

/// The digest of a string value's canonical frame, from borrowed bytes.
///
/// Identical to hashing `CanonicalValue::String(bytes.to_vec())` without the
/// clone — the frame is written by the same `encode_bytes` — which is what
/// lets the native digest path hash string columns without materializing
/// their values.
fn stable_hash_borrowed_string(bytes: &[u8]) -> u128 {
    if 9 + bytes.len() <= INLINE {
        let mut inline = Inline::new();
        encode_bytes(4, bytes, &mut inline);
        xxh3_128_with_seed(inline.bytes(), 0)
    } else {
        let mut state = Xxh3State::with_seed(0);
        encode_bytes(4, bytes, &mut state);
        state.digest128()
    }
}

fn encoded_len(value: &CanonicalValue) -> usize {
    match value {
        CanonicalValue::Null => 1,
        CanonicalValue::Boolean(_) => 2,
        CanonicalValue::Int(_) | CanonicalValue::Double(_) => 9,
        CanonicalValue::Nanos(_) => 17,
        CanonicalValue::Rational(_) => 41,
        CanonicalValue::String(value)
        | CanonicalValue::UnparsedString(value)
        | CanonicalValue::Opaque(value) => 9 + value.len(),
    }
}

fn encode_value(value: &CanonicalValue, sink: &mut impl Sink) {
    match value {
        CanonicalValue::Null => sink.write(&[0]),
        CanonicalValue::Boolean(value) => sink.write(&[1, u8::from(*value)]),
        CanonicalValue::Int(value) => {
            sink.write(&[2]);
            sink.write(&value.to_le_bytes());
        }
        CanonicalValue::Double(value) => {
            sink.write(&[3]);
            sink.write(&value.to_le_bytes());
        }
        CanonicalValue::String(value) => encode_bytes(4, value, sink),
        CanonicalValue::UnparsedString(value) => encode_bytes(5, value, sink),
        CanonicalValue::Opaque(value) => encode_bytes(6, value, sink),
        CanonicalValue::Nanos(value) => {
            sink.write(&[7]);
            sink.write(&value.to_le_bytes());
        }
        CanonicalValue::Rational(value) => {
            sink.write(&[8]);
            sink.write(&value.mantissa);
            sink.write(&value.exp2.to_le_bytes());
            sink.write(&value.exp5.to_le_bytes());
        }
    }
}

fn encode_bytes(tag: u8, value: &[u8], sink: &mut impl Sink) {
    sink.write(&[tag]);
    sink.write(&(value.len() as u64).to_le_bytes());
    sink.write(value);
}

#[cfg(test)]
fn hash_with(value: &CanonicalValue, hasher: &impl StableHasher) -> u128 {
    let mut bytes = Vec::new();
    encode_value(value, &mut bytes);
    hasher.hash(&bytes)
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
    use std::sync::Arc;

    use arrow_array::types::Int8Type;
    use arrow_array::{Array, ArrayRef, BooleanArray, DictionaryArray, Int8Array, StringArray};
    use test_support::column;

    use super::{
        CanonicalValue, ComparisonPlan, INLINE, NativeEq, NativeHasher, StableHasher, Xxh3,
        encode_sequence, equal_after_hash, hash_with, parse_exact_i64, rational_of_double,
        sequence_hash, sequence_hash_of, stable_hash,
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
    fn the_promoted_pairs_have_plans_and_the_refused_pairs_stay_planless() {
        use arrow_schema::{DataType, TimeUnit};

        let date = DataType::Date32;
        let wide = DataType::Date64;
        let aware = DataType::Timestamp(TimeUnit::Millisecond, Some("UTC".into()));
        let aware_ns = DataType::Timestamp(TimeUnit::Nanosecond, Some("+02:00".into()));
        let naive = DataType::Timestamp(TimeUnit::Millisecond, None);
        let naive_us = DataType::Timestamp(TimeUnit::Microsecond, None);
        let decimal = DataType::Decimal128(10, 2);
        let decimal_wide = DataType::Decimal256(60, 10);
        let string = DataType::Utf8;

        // Every decided row of the promoted matrix has a plan, both ways.
        for (left, right) in [
            (&aware, &aware_ns),
            (&naive, &naive_us),
            (&date, &wide),
            (&date, &naive),
            (&wide, &naive_us),
            (&decimal, &decimal_wide),
            (&decimal, &DataType::Int64),
            (&decimal, &DataType::Float64),
            (&string, &date),
            (&string, &naive),
            (&string, &aware),
            (&string, &decimal),
        ] {
            assert!(
                ComparisonPlan::new(left, right).is_some(),
                "{left:?}/{right:?}"
            );
            assert!(
                ComparisonPlan::new(right, left).is_some(),
                "{right:?}/{left:?}"
            );
        }

        // The refusals: awareness is never crossed, a date meets no instant,
        // booleans meet no promoted type, and integers meet no temporal one.
        for (left, right) in [
            (&aware, &naive),
            (&date, &aware),
            (&string, &DataType::Duration(TimeUnit::Second)),
            (&DataType::Boolean, &date),
            (&DataType::Boolean, &decimal),
            (&DataType::Int64, &date),
            (&DataType::Int64, &naive),
            (&DataType::Float64, &aware),
        ] {
            assert!(
                ComparisonPlan::new(left, right).is_none(),
                "{left:?}/{right:?}"
            );
            assert!(
                ComparisonPlan::new(right, left).is_none(),
                "{right:?}/{left:?}"
            );
        }

        // Outside the promoted families, identity of type is still the whole
        // of comparability, dictionary-wrapped promoted types included.
        let duration = DataType::Duration(TimeUnit::Second);
        let wrapped = DataType::Dictionary(Box::new(DataType::Int8), Box::new(naive.clone()));
        assert!(ComparisonPlan::new(&duration, &duration).is_some());
        assert!(ComparisonPlan::new(&wrapped, &wrapped).is_some());
        assert!(
            ComparisonPlan::new(&duration, &DataType::Duration(TimeUnit::Millisecond)).is_none()
        );
        assert!(ComparisonPlan::new(&wrapped, &naive).is_none());
    }

    #[test]
    fn opaque_values_compare_and_hash_by_their_encoding() {
        let (old, new) = values(
            column!(binary[Some("a"), Some("b"), None, None]),
            column!(binary[Some("a"), Some("c"), None, Some("d")]),
        );
        assert_eq!(old[0], new[0]);
        assert!(matches!(old[0], CanonicalValue::Opaque(_)));
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
    fn timestamps_compare_as_instants_across_units_and_timezones() {
        use std::sync::Arc;

        use arrow_array::TimestampSecondArray;

        // One second is a billion nanoseconds; the unit is representation.
        let (old, new) = values(
            column!(ts_ms[Some(1000), Some(2000), None]),
            column!(ts_us[Some(1_000_000), Some(3_000_000), None]),
        );
        assert_eq!(old[0], new[0]);
        assert!(matches!(old[0], CanonicalValue::Nanos(_)));
        assert_ne!(old[1], new[1]);
        assert_eq!(old[2], CanonicalValue::Null);
        assert_eq!(stable_hash(&old[0]), stable_hash(&new[0]));

        // The timezone string is presentation metadata: the stored value is
        // the instant, so one day into 1970 is one day into 1970 in New York.
        let new_york =
            Arc::new(TimestampSecondArray::from(vec![86_400]).with_timezone("America/New_York"))
                as ArrayRef;
        let (old, new) = values(column!(ts_ms[86_400_000]), new_york);
        assert_eq!(old, new);

        // The extremes of the widest and narrowest units canonicalize
        // without overflow, nanoseconds being held in an `i128`.
        let seconds =
            Arc::new(TimestampSecondArray::from(vec![i64::MAX, i64::MIN]).with_timezone("UTC"))
                as ArrayRef;
        let (old, new) = values(seconds, column!(ts_us[i64::MAX, i64::MIN]));
        assert_ne!(old[0], new[0]);
        assert_ne!(old[1], new[1]);
    }

    #[test]
    fn naive_timestamps_compare_as_wall_clock_across_units() {
        let (old, new) = values(
            column!(ts_ms_naive[Some(1500), None]),
            column!(ts_us_naive[Some(1_500_000), None]),
        );
        assert_eq!(old[0], new[0]);
        assert_eq!(old[1], CanonicalValue::Null);
        assert_eq!(stable_hash(&old[0]), stable_hash(&new[0]));
    }

    #[test]
    fn the_two_date_spellings_meet_on_the_day() {
        let (old, new) = values(
            column!(date32[Some(1), Some(-1), Some(2), None]),
            column!(date64[
                Some(86_400_000),
                Some(-86_400_000),
                Some(172_800_001),
                None
            ]),
        );
        assert_eq!(old[0], new[0]);
        assert_eq!(old[1], new[1]);
        // A non-midnight `Date64` is a real value, equal to no `Date32`.
        assert_ne!(old[2], new[2]);
        assert_eq!(old[3], CanonicalValue::Null);
    }

    #[test]
    fn a_date_is_the_exact_midnight_of_a_naive_timestamp() {
        let (old, new) = values(
            column!(date32[Some(1), Some(1), Some(-1)]),
            column!(ts_ms_naive[
                Some(86_400_000),
                Some(86_400_001),
                Some(-86_400_000)
            ]),
        );
        assert_eq!(old[0], new[0]);
        // One millisecond past midnight is another value entirely.
        assert_ne!(old[1], new[1]);
        assert_eq!(old[2], new[2]);
    }

    #[test]
    fn decimals_compare_by_value_across_precision_scale_and_width() {
        use std::sync::Arc;

        use arrow_array::Decimal256Array;
        use arrow_buffer::i256;

        let (old, new) = values(
            column!(dec[Some(150), Some(150), None]),
            column!(dec_wide[Some(150_000), Some(150_010), None]),
        );
        assert_eq!(old[0], new[0]);
        assert!(matches!(old[0], CanonicalValue::Rational(_)));
        assert_ne!(old[1], new[1]);
        assert_eq!(old[2], CanonicalValue::Null);
        assert_eq!(stable_hash(&old[0]), stable_hash(&new[0]));

        // A `Decimal256` at a third scale reaches the same canonical value.
        let wide = Arc::new(
            Decimal256Array::from(vec![i256::from_i128(15_000_000_000)])
                .with_precision_and_scale(60, 10)
                .unwrap(),
        ) as ArrayRef;
        let (old, new) = values(column!(dec[150]), wide);
        assert_eq!(old, new);
    }

    #[test]
    fn a_decimal_equals_the_integer_it_exactly_is() {
        use std::sync::Arc;

        use arrow_array::Decimal128Array;

        let (old, new) = values(
            column!(dec[Some(500), Some(501), None]),
            column!([Some(5), Some(5), None]),
        );
        assert_eq!(old[0], new[0]);
        assert_eq!(old[0], CanonicalValue::Int(5));
        assert_ne!(old[1], new[1]);
        assert_eq!(old[2], CanonicalValue::Null);

        // A decimal beyond every `i64` keeps a form no integer has.
        let huge = Arc::new(
            Decimal128Array::from(vec![i128::from(i64::MAX) + 1])
                .with_precision_and_scale(38, 0)
                .unwrap(),
        ) as ArrayRef;
        let (old, new) = values(huge, column!([i64::MAX]));
        assert!(matches!(old[0], CanonicalValue::Rational(_)));
        assert_ne!(old[0], new[0]);

        // Negative scale multiplies out rather than dividing: 5 × 10² is 500.
        let shifted = Arc::new(
            Decimal128Array::from(vec![5])
                .with_precision_and_scale(10, -2)
                .unwrap(),
        ) as ArrayRef;
        let (old, new) = values(shifted, column!([500]));
        assert_eq!(old, new);
        assert_eq!(old[0], CanonicalValue::Int(500));
    }

    #[test]
    fn a_decimal_meets_a_double_only_at_the_exact_value() {
        let (old, new) = values(
            column!(dec[Some(50), Some(10), Some(0)]),
            column!([Some(0.5), Some(0.1), Some(-0.0)]),
        );
        // 0.50 is exactly the double 0.5; 0.10 is not the double nearest 0.1,
        // whose exact value has fifty-five decimal digits; zero is zero,
        // signed or not.
        assert_eq!(old[0], new[0]);
        assert_ne!(old[1], new[1]);
        assert_eq!(old[2], new[2]);

        // `NaN` and infinity equal no decimal, and `NaN` still invalidates
        // keys, its canonicalization being the double domain's own.
        let (old, new) = values(
            column!(dec[Some(0), Some(0)]),
            column!([f64::NAN, f64::INFINITY]),
        );
        assert_ne!(old[0], new[0]);
        assert_ne!(old[1], new[1]);
        assert!(new[0].invalid_key());
    }

    #[test]
    fn iso_date_strings_parse_against_date_columns() {
        let (old, new) = values(
            column!([
                "2026-08-03",
                "2024-02-29",
                "2023-02-29",
                "1969-12-31",
                "2026-8-3",
                " 2026-08-03"
            ]),
            column!(date32[20668, 19782, 19782, -1, 19782, 20668]),
        );
        assert_eq!(old[0], new[0]);
        // A leap day parses exactly in a leap year and not at all outside one.
        assert_eq!(old[1], new[1]);
        assert!(matches!(old[2], CanonicalValue::UnparsedString(_)));
        assert_eq!(old[3], new[3]);
        // Extended format only, untrimmed: no elided zeros, no whitespace.
        assert!(matches!(old[4], CanonicalValue::UnparsedString(_)));
        assert!(matches!(old[5], CanonicalValue::UnparsedString(_)));
    }

    #[test]
    fn iso_naive_strings_parse_wall_clock_readings() {
        let (old, new) = values(
            column!([
                "1970-01-01T00:00:01",
                "1970-01-01T00:00:00.5",
                "1970-01-02T00:00:00",
                "1970-01-01",
                "1970-01-01T00:00:01Z",
                "1970-01-01t00:00:01"
            ]),
            column!(ts_ms_naive[1000, 500, 86_400_000, 0, 1000, 1000]),
        );
        assert_eq!(old[0], new[0]);
        assert_eq!(old[1], new[1]);
        assert_eq!(old[2], new[2]);
        // A date-only string is the date profile's, not an implicit midnight;
        // an offset belongs to instants; designators are uppercase.
        assert!(matches!(old[3], CanonicalValue::UnparsedString(_)));
        assert!(matches!(old[4], CanonicalValue::UnparsedString(_)));
        assert!(matches!(old[5], CanonicalValue::UnparsedString(_)));
    }

    #[test]
    fn fractional_seconds_reach_nanoseconds_and_stop_there() {
        use std::sync::Arc;

        use arrow_array::TimestampNanosecondArray;

        let nanos = Arc::new(TimestampNanosecondArray::from(vec![123_456_789, 0, 0])) as ArrayRef;
        let (old, new) = values(
            column!([
                "1970-01-01T00:00:00.123456789",
                "1970-01-01T00:00:00.0000000000",
                "1970-01-01T00:00:00."
            ]),
            nanos,
        );
        assert_eq!(old[0], new[0]);
        // Ten fractional digits name nothing a timestamp can hold, and a
        // bare point names nothing at all.
        assert!(matches!(old[1], CanonicalValue::UnparsedString(_)));
        assert!(matches!(old[2], CanonicalValue::UnparsedString(_)));
    }

    #[test]
    fn iso_instant_strings_require_an_offset_and_normalize_to_utc() {
        let (old, new) = values(
            column!([
                "1970-01-01T00:00:01Z",
                "1970-01-01T01:00:00+01:00",
                "1969-12-31T19:00:00-05:00",
                "1970-01-01T14:00:00+14:00",
                "1970-01-01T00:00:01",
                "1970-01-01T00:00:01z",
                "1970-01-01T00:00:01-00:00",
                "1970-01-01T14:01:00+14:01",
                "1970-01-01T15:00:00+15:00"
            ]),
            column!(ts_ms[1000, 0, 0, 0, 1000, 1000, 1000, 0, 0]),
        );
        assert_eq!(old[0], new[0]);
        // Two spellings of the epoch, one instant — and the offset range's
        // far edge, ±14:00, is a real offset that reaches it too.
        assert_eq!(old[1], new[1]);
        assert_eq!(old[2], new[2]);
        assert_eq!(old[3], new[3]);
        // No offset is no instant; designators are uppercase; ISO 8601 has no
        // negative zero offset, no minutes past a ±14-hour offset, and
        // nothing beyond it.
        assert!(matches!(old[4], CanonicalValue::UnparsedString(_)));
        assert!(matches!(old[5], CanonicalValue::UnparsedString(_)));
        assert!(matches!(old[6], CanonicalValue::UnparsedString(_)));
        assert!(matches!(old[7], CanonicalValue::UnparsedString(_)));
        assert!(matches!(old[8], CanonicalValue::UnparsedString(_)));
    }

    #[test]
    fn decimal_strings_parse_exactly_to_rationals() {
        let (old, new) = values(
            column!(["1.50", "1.5", "1e2", "-0.00", "0.1", "1.51", "NaN", "1_000"]),
            column!(dec[150, 150, 10_000, 0, 10, 150, 0, 100_000]),
        );
        assert_eq!(old[0], new[0]);
        assert_eq!(old[1], new[1]);
        assert_eq!(old[2], new[2]);
        assert_eq!(old[3], new[3]);
        // "0.1" is exactly the decimal 0.10 — the parse never touches a float.
        assert_eq!(old[4], new[4]);
        assert_ne!(old[5], new[5]);
        // A decimal holds no `NaN`, and the grammar holds no underscores.
        assert!(matches!(old[6], CanonicalValue::UnparsedString(_)));
        assert!(matches!(old[7], CanonicalValue::UnparsedString(_)));
        assert_eq!(stable_hash(&old[0]), stable_hash(&new[0]));
    }

    #[test]
    fn decimal_strings_reach_the_far_edge_of_the_decimal_range() {
        use std::sync::Arc;

        use arrow_array::Decimal256Array;
        use arrow_buffer::i256;

        // Trailing zeros strip before accumulation, so a long spelling of a
        // representable value parses; eighty significant digits name a value
        // no decimal column can hold, and mismatch; an exponent at the `i32`
        // edge would overflow the canonical triple's exponents, which no
        // decimal's can, and mismatches rather than wrapping or panicking.
        let long = format!("1{}", "0".repeat(80));
        let dense = "9".repeat(80);
        let strings = Arc::new(StringArray::from(vec![
            long.as_str(),
            dense.as_str(),
            "2e2147483647",
            "2e-2147483649",
        ])) as ArrayRef;
        let wide = Arc::new(
            Decimal256Array::from(vec![
                i256::from_i128(10_000),
                i256::from_i128(0),
                i256::from_i128(0),
                i256::from_i128(0),
            ])
            .with_precision_and_scale(76, -76)
            .unwrap(),
        ) as ArrayRef;

        let (old, new) = values(strings, wide);
        assert_eq!(old[0], new[0]);
        assert!(matches!(old[1], CanonicalValue::UnparsedString(_)));
        assert!(matches!(old[2], CanonicalValue::UnparsedString(_)));
        assert!(matches!(old[3], CanonicalValue::UnparsedString(_)));
    }

    #[test]
    fn temporal_equality_does_not_compose_across_triangles() {
        // "2026-08-03" equals the date, the date equals its midnight naive
        // timestamp, and the string does not equal that timestamp: each pair
        // compares under its own profile, as the numeric triangles always
        // have.
        let day = column!(date32[20668]);
        let midnight = column!(ts_ms_naive[20_668_i64 * 86_400_000]);
        let text = column!(["2026-08-03"]);

        let (old, new) = values(text.clone(), day.clone());
        assert_eq!(old, new);
        let (old, new) = values(day, midnight.clone());
        assert_eq!(old, new);
        let (old, new) = values(text, midnight);
        assert!(matches!(old[0], CanonicalValue::UnparsedString(_)));
        assert_ne!(old[0], new[0]);
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

    /// The digests row sampling selects by, pinned as literals (captured
    /// 2026-08-05). These are load-bearing values, not an implementation
    /// detail: the bottom-k key digests choose which rows sampled inference
    /// measures, so a changed digest silently changes output on large tables.
    /// A failure here means the encoding or the hash moved, and the sampled
    /// baselines moved with it.
    #[test]
    fn stable_hash_digests_are_pinned() {
        let cases: [(CanonicalValue, u128); 10] = [
            (CanonicalValue::Null, 0xa6cd5e9392000f6ac44bdff4074eecdb),
            (
                CanonicalValue::Boolean(true),
                0x50e58cbd6ada00a1070d80d6a76865f3,
            ),
            (CanonicalValue::Int(42), 0x932241402dd83e6bf37f9a8d5d376ff9),
            (
                CanonicalValue::Double(1.5f64.to_bits()),
                0x2d0719929098c2ee4848f1440928d509,
            ),
            (
                CanonicalValue::String(b"hello".to_vec()),
                0x09883e13ee49e90df19b452684f6edfb,
            ),
            (
                CanonicalValue::UnparsedString(b"1.5x".to_vec()),
                0x7636d752bb08d959706440445ccac165,
            ),
            (
                CanonicalValue::Nanos(1_000_000_000),
                0xd63c4f3c54038e1149f0d2ea94e0c878,
            ),
            (
                CanonicalValue::Opaque(vec![1, 2, 3]),
                0x3ec6974d8d3d978aed59221690025f10,
            ),
            (
                CanonicalValue::String(vec![b'x'; 1024]),
                0x7f6465c271757c6e2af2181384d2c8a2,
            ),
            (
                CanonicalValue::Rational(rational_of_double(0.1).expect("0.1 is finite")),
                0x58510aaaf0e60291cfe04dd9ad61e405,
            ),
        ];
        for (value, digest) in &cases {
            assert_eq!(stable_hash(value), *digest, "digest moved for {value:?}");
        }
    }

    /// Sequence digests pinned the same way (captured 2026-08-05): key tuples
    /// and column digests both come from `sequence_hash`.
    #[test]
    fn sequence_hash_digests_are_pinned() {
        assert_eq!(sequence_hash(&[]), 0x2c0a8a99dc147d5445c3b49d035665b2);
        assert_eq!(
            sequence_hash(&[
                CanonicalValue::Int(1),
                CanonicalValue::String(b"a".to_vec()),
            ]),
            0x9d030fe3a00f96cab4d0c049492c1de1
        );
        let column: Vec<CanonicalValue> = (0..1000).map(CanonicalValue::Int).collect();
        assert_eq!(sequence_hash(&column), 0x6defbb4dd2223299d3628af6c544ba61);
    }

    /// For every eligible type, the native comparator and hasher must produce
    /// exactly the canonicalizing path's verdicts and digests — over every
    /// ordered pair of the fixture's values, nulls and adversaries included.
    fn assert_native_matches_canonical(old: ArrayRef, new: ArrayRef) {
        let plan = ComparisonPlan::new(old.data_type(), new.data_type())
            .expect("eligible types have identity plans");
        let native = NativeEq::for_pair(old.as_ref(), new.as_ref()).expect("eligible type");
        let hasher = NativeHasher::for_column(old.as_ref(), plan).expect("eligible type");
        let canonical_old = plan.canonicalize_old(old.as_ref());
        let canonical_new = plan.canonicalize_new(new.as_ref());
        for (i, old_value) in canonical_old.iter().enumerate() {
            assert_eq!(
                hasher.hash(i),
                stable_hash(old_value),
                "digest diverged at row {i} of {:?}",
                old.data_type()
            );
            for (j, new_value) in canonical_new.iter().enumerate() {
                assert_eq!(
                    native.equal(i, j),
                    old_value == new_value,
                    "verdict diverged at ({i}, {j}) of {:?}",
                    old.data_type()
                );
            }
        }
        assert_eq!(
            sequence_hash_of(
                canonical_old.len(),
                (0..canonical_old.len()).map(|row| hasher.hash(row)),
            ),
            sequence_hash(&canonical_old),
            "column digest diverged for {:?}",
            old.data_type()
        );
    }

    #[test]
    fn native_integers_and_booleans_match_the_canonical_path() {
        use arrow_array::{
            Int16Array, Int32Array, Int64Array, UInt8Array, UInt16Array, UInt32Array, UInt64Array,
        };
        let pairs: [(ArrayRef, ArrayRef); 9] = [
            (
                Arc::new(BooleanArray::from(vec![Some(true), Some(false), None])),
                Arc::new(BooleanArray::from(vec![Some(false), Some(true), None])),
            ),
            (
                Arc::new(Int8Array::from(vec![Some(i8::MIN), Some(i8::MAX), None])),
                Arc::new(Int8Array::from(vec![Some(0), Some(i8::MAX), None])),
            ),
            (
                Arc::new(Int16Array::from(vec![Some(i16::MIN), Some(i16::MAX), None])),
                Arc::new(Int16Array::from(vec![Some(0), Some(i16::MAX), None])),
            ),
            (
                Arc::new(Int32Array::from(vec![Some(i32::MIN), Some(i32::MAX), None])),
                Arc::new(Int32Array::from(vec![Some(0), Some(i32::MAX), None])),
            ),
            (
                Arc::new(Int64Array::from(vec![
                    Some(i64::MIN),
                    Some(i64::MAX),
                    Some(-1),
                    None,
                ])),
                Arc::new(Int64Array::from(vec![Some(0), Some(i64::MAX), None, None])),
            ),
            (
                Arc::new(UInt8Array::from(vec![Some(0), Some(u8::MAX), None])),
                Arc::new(UInt8Array::from(vec![Some(1), Some(u8::MAX), None])),
            ),
            (
                Arc::new(UInt16Array::from(vec![Some(0), Some(u16::MAX), None])),
                Arc::new(UInt16Array::from(vec![Some(1), Some(u16::MAX), None])),
            ),
            (
                Arc::new(UInt32Array::from(vec![Some(0), Some(u32::MAX), None])),
                Arc::new(UInt32Array::from(vec![Some(1), Some(u32::MAX), None])),
            ),
            // u64::MAX wraps to -1 as canonical i64, which only ever meets
            // values wrapped the same way: the pair is same-type by the gate.
            (
                Arc::new(UInt64Array::from(vec![Some(0), Some(u64::MAX), None])),
                Arc::new(UInt64Array::from(vec![Some(1), Some(u64::MAX), None])),
            ),
        ];
        for (old, new) in pairs {
            assert_native_matches_canonical(old, new);
        }
    }

    #[test]
    fn native_doubles_normalize_zeros_and_nan_payloads() {
        use arrow_array::{Float32Array, Float64Array};
        let quiet_nan_with_payload = f64::from_bits(0x7ff8_0000_0000_0001);
        assert_native_matches_canonical(
            Arc::new(Float64Array::from(vec![
                Some(0.0),
                Some(-0.0),
                Some(f64::NAN),
                Some(quiet_nan_with_payload),
                Some(1.5),
                Some(f64::INFINITY),
                None,
            ])),
            Arc::new(Float64Array::from(vec![
                Some(-0.0),
                Some(0.0),
                Some(quiet_nan_with_payload),
                Some(f64::NAN),
                Some(-1.5),
                Some(f64::NEG_INFINITY),
                None,
            ])),
        );
        assert_native_matches_canonical(
            Arc::new(Float32Array::from(vec![
                Some(0.0),
                Some(-0.0),
                Some(f32::NAN),
                Some(2.5),
                None,
            ])),
            Arc::new(Float32Array::from(vec![
                Some(-0.0),
                Some(f32::NAN),
                Some(2.5),
                Some(-2.5),
                None,
            ])),
        );
    }

    #[test]
    fn native_strings_temporals_and_decimals_match_the_canonical_path() {
        use arrow_array::{
            Date32Array, Date64Array, Decimal128Array, LargeStringArray, TimestampMillisecondArray,
            TimestampSecondArray,
        };
        assert_native_matches_canonical(
            Arc::new(StringArray::from(vec![
                Some(""),
                Some("a"),
                Some("value-1"),
                Some("1.0"),
                None,
            ])),
            Arc::new(StringArray::from(vec![
                Some("a"),
                Some(""),
                Some("value-1"),
                Some("1"),
                None,
            ])),
        );
        assert_native_matches_canonical(
            Arc::new(LargeStringArray::from(vec![Some("x"), Some(""), None])),
            Arc::new(LargeStringArray::from(vec![Some(""), Some("x"), None])),
        );
        assert_native_matches_canonical(
            Arc::new(TimestampSecondArray::from(vec![
                Some(0),
                Some(-1),
                Some(1_600_000_000),
                None,
            ])),
            Arc::new(TimestampSecondArray::from(vec![
                Some(-1),
                Some(0),
                Some(1_600_000_000),
                None,
            ])),
        );
        assert_native_matches_canonical(
            Arc::new(
                TimestampMillisecondArray::from(vec![Some(0), Some(86_400_000), None])
                    .with_timezone("UTC"),
            ),
            Arc::new(
                TimestampMillisecondArray::from(vec![Some(86_400_000), Some(0), None])
                    .with_timezone("UTC"),
            ),
        );
        assert_native_matches_canonical(
            Arc::new(Date32Array::from(vec![
                Some(0),
                Some(-1),
                Some(19_000),
                None,
            ])),
            Arc::new(Date32Array::from(vec![
                Some(-1),
                Some(19_000),
                Some(0),
                None,
            ])),
        );
        assert_native_matches_canonical(
            Arc::new(Date64Array::from(vec![Some(0), Some(86_400_000), None])),
            Arc::new(Date64Array::from(vec![Some(86_400_000), Some(0), None])),
        );
        let decimals = |values: Vec<Option<i128>>| {
            Decimal128Array::from(values)
                .with_precision_and_scale(10, 2)
                .expect("valid precision and scale")
        };
        assert_native_matches_canonical(
            Arc::new(decimals(vec![
                Some(100),
                Some(-100),
                Some(0),
                Some(12_345),
                None,
            ])),
            Arc::new(decimals(vec![
                Some(-100),
                Some(100),
                Some(12_345),
                Some(0),
                None,
            ])),
        );
    }

    #[test]
    fn ineligible_pairs_fall_back_to_the_canonical_path() {
        use arrow_array::{Int64Array, TimestampMillisecondArray, TimestampSecondArray};
        // A dictionary hides hydration and logical nulls behind its keys.
        let dictionary: DictionaryArray<Int8Type> = vec!["a", "b", "a"].into_iter().collect();
        let same: DictionaryArray<Int8Type> = vec!["a", "b", "a"].into_iter().collect();
        assert!(NativeEq::for_pair(&dictionary, &same).is_none());
        let plan = ComparisonPlan::new(dictionary.data_type(), same.data_type())
            .expect("string dictionaries compare");
        assert!(NativeHasher::for_column(&dictionary, plan).is_none());

        // A cross-type pair canonicalizes into a shared domain the raw values
        // alone do not determine.
        let strings = StringArray::from(vec!["1", "2"]);
        let integers = Int64Array::from(vec![1, 2]);
        assert!(NativeEq::for_pair(&strings, &integers).is_none());
        let parsing = ComparisonPlan::new(strings.data_type(), integers.data_type())
            .expect("strings compare against integers");
        assert!(NativeHasher::for_column(&strings, parsing).is_none());

        // Different units are different data types, even within one family.
        let seconds = TimestampSecondArray::from(vec![1]);
        let milliseconds = TimestampMillisecondArray::from(vec![1_000]);
        assert!(NativeEq::for_pair(&seconds, &milliseconds).is_none());
    }

    /// The one-shot stack path and the streaming path must agree with the
    /// buffered encoding either side of the `INLINE` boundary, where a string
    /// payload of `INLINE - 9` bytes is the last one-shot value.
    #[test]
    fn inline_and_streaming_paths_agree_with_the_buffered_encoding() {
        for len in [0, 5, INLINE - 10, INLINE - 9, INLINE - 8, 1024, 4096] {
            let value = CanonicalValue::String(vec![b'x'; len]);
            assert_eq!(
                stable_hash(&value),
                hash_with(&value, &Xxh3),
                "paths disagree at payload length {len}"
            );
        }
        // Sequences cross their own boundary at (INLINE - 8) / 24 values.
        let boundary = (INLINE - 8) / 24;
        for len in [0, 1, boundary, boundary + 1, 100] {
            let values: Vec<CanonicalValue> = (0..len as i64).map(CanonicalValue::Int).collect();
            let mut frame = Vec::new();
            encode_sequence(values.len(), values.iter().map(stable_hash), &mut frame);
            assert_eq!(
                sequence_hash(&values),
                Xxh3.hash(&frame),
                "sequence paths disagree at {len} values"
            );
        }
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
