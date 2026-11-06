//! Small framework helpers that are useful across modules.

use std::{
    collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, HashSet, LinkedList, VecDeque},
    hash::BuildHasher,
};

/// Defines PHP-style truthiness for values accepted by [`condition!`].
pub trait ConditionValue {
    /// Returns whether this value selects the true branch.
    fn is_truthy(&self) -> bool;
}

/// Tests the truthiness of a value supported by [`condition!`].
pub fn is_truthy<T: ConditionValue + ?Sized>(value: &T) -> bool {
    value.is_truthy()
}

/// Selects a value from a truthy check, using a fallback when the check is
/// false. This form is useful when the check and selected value are fallible,
/// such as reading an optional typed environment variable.
///
/// Errors from `check` are returned. `when_true` is returned only when the
/// check is truthy, so an error held in that result is ignored when the check
/// is false. Since Rust evaluates function arguments before calling this
/// function, both `check` and `when_true` expressions are evaluated eagerly.
///
/// ```
/// use framework_core::condition;
///
/// let interval = condition(Ok::<bool, &'static str>(false), Err("missing"), 1000_u64)?;
/// assert_eq!(interval, 1000);
/// # Ok::<(), &'static str>(())
/// ```
pub fn condition<C, T, E>(
    check: Result<C, E>,
    when_true: Result<T, E>,
    when_false: T,
) -> Result<T, E>
where
    C: ConditionValue,
{
    match check {
        Ok(value) if value.is_truthy() => when_true,
        Ok(_) => Ok(when_false),
        Err(error) => Err(error),
    }
}

impl<T: ConditionValue + ?Sized> ConditionValue for &T {
    fn is_truthy(&self) -> bool {
        (**self).is_truthy()
    }
}

impl ConditionValue for bool {
    fn is_truthy(&self) -> bool {
        *self
    }
}

macro_rules! implement_numeric_truthiness {
    ($($number:ty),+ $(,)?) => {
        $(
            impl ConditionValue for $number {
                fn is_truthy(&self) -> bool {
                    *self != 0 as $number
                }
            }
        )+
    };
}

implement_numeric_truthiness!(
    i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize, f32, f64
);

impl ConditionValue for str {
    fn is_truthy(&self) -> bool {
        !self.is_empty() && self != "0"
    }
}

impl ConditionValue for String {
    fn is_truthy(&self) -> bool {
        self.as_str().is_truthy()
    }
}

impl ConditionValue for char {
    fn is_truthy(&self) -> bool {
        true
    }
}

impl ConditionValue for () {
    fn is_truthy(&self) -> bool {
        false
    }
}

impl<T: ConditionValue> ConditionValue for Option<T> {
    fn is_truthy(&self) -> bool {
        self.as_ref().is_some_and(ConditionValue::is_truthy)
    }
}

impl<T> ConditionValue for [T] {
    fn is_truthy(&self) -> bool {
        !self.is_empty()
    }
}

impl<T, const N: usize> ConditionValue for [T; N] {
    fn is_truthy(&self) -> bool {
        N != 0
    }
}

macro_rules! implement_collection_truthiness {
    ($($collection:ty),+ $(,)?) => {
        $(
            impl<T> ConditionValue for $collection {
                fn is_truthy(&self) -> bool {
                    !self.is_empty()
                }
            }
        )+
    };
}

implement_collection_truthiness!(Vec<T>, VecDeque<T>, LinkedList<T>, BinaryHeap<T>, HashSet<T, std::collections::hash_map::RandomState>, BTreeSet<T>);

impl<K, V, S: BuildHasher> ConditionValue for HashMap<K, V, S> {
    fn is_truthy(&self) -> bool {
        !self.is_empty()
    }
}

impl<K, V> ConditionValue for BTreeMap<K, V> {
    fn is_truthy(&self) -> bool {
        !self.is_empty()
    }
}

/// Selects one expression based on the truthiness of a value and evaluates only
/// that branch.
///
/// Booleans use their value; numeric zero, `None`, unit, empty strings and
/// empty collections are false. Non-empty strings other than `"0"`, non-zero
/// numbers and non-empty collections are true. Custom types can implement
/// [`ConditionValue`].
///
/// ```
/// use framework_core::condition;
///
/// let enabled = true;
/// let label = condition!(enabled, "on", "off");
/// assert_eq!(label, "on");
/// ```
#[macro_export]
macro_rules! condition {
    ($condition:expr, $when_true:expr, $when_false:expr $(,)?) => {
        if $crate::helpers::is_truthy(&$condition) {
            $when_true
        } else {
            $when_false
        }
    };
}
