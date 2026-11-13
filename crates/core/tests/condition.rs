use framework_core::condition;
use framework_core::helpers::ConditionValue;
use std::{cell::Cell, collections::HashMap};

#[test]
fn true_condition_evaluates_only_the_true_branch() {
    let true_calls = Cell::new(0);
    let false_calls = Cell::new(0);

    let result = condition!(
        true,
        {
            true_calls.set(true_calls.get() + 1);
            "true"
        },
        {
            false_calls.set(false_calls.get() + 1);
            "false"
        },
    );

    assert_eq!(result, "true");
    assert_eq!(true_calls.get(), 1);
    assert_eq!(false_calls.get(), 0);
}

#[test]
fn false_condition_evaluates_only_the_false_branch() {
    let true_calls = Cell::new(0);
    let false_calls = Cell::new(0);

    let result = condition!(
        false,
        {
            true_calls.set(true_calls.get() + 1);
            10
        },
        {
            false_calls.set(false_calls.get() + 1);
            20
        }
    );

    assert_eq!(result, 20);
    assert_eq!(true_calls.get(), 0);
    assert_eq!(false_calls.get(), 1);
}

#[test]
fn condition_expression_is_evaluated_exactly_once() {
    let condition_calls = Cell::new(0);

    let result = condition!(
        {
            condition_calls.set(condition_calls.get() + 1);
            true
        },
        "yes",
        "no"
    );

    assert_eq!(result, "yes");
    assert_eq!(condition_calls.get(), 1);
}

#[test]
fn unselected_panicking_branch_is_not_evaluated() {
    assert_eq!(condition!(true, 42, panic!("false branch ran")), 42);
    assert_eq!(condition!(false, panic!("true branch ran"), 24), 24);
}

#[test]
fn nested_conditions_select_the_expected_value() {
    let result = condition!(true, condition!(false, "first", "nested"), "outer");

    assert_eq!(result, "nested");
}

#[test]
#[allow(clippy::let_and_return)]
fn branches_can_return_block_expressions_and_allow_a_trailing_comma() {
    let result = condition!(
        true,
        {
            let value = String::from("block");
            value
        },
        String::new(),
    );

    assert_eq!(result, "block");
}

#[test]
fn condition_macro_matches_php_style_truthiness_for_common_values() {
    let null: Option<()> = None;
    let empty_array: [i32; 0] = [];
    let empty_vector: Vec<i32> = Vec::new();
    let empty_map: HashMap<&str, i32> = HashMap::new();

    assert_eq!(choose_condition(true), "second");
    assert_eq!(choose_condition(false), "third");
    assert_eq!(choose_condition(null), "third");
    assert_eq!(choose_condition(0_i32), "third");
    assert_eq!(choose_condition(1_i32), "second");
    assert_eq!(choose_condition(-1_i32), "second");
    assert_eq!(choose_condition(""), "third");
    assert_eq!(choose_condition("Hola"), "second");
    assert_eq!(choose_condition(empty_array), "third");
    assert_eq!(choose_condition(empty_vector), "third");
    assert_eq!(choose_condition(empty_map), "third");
    assert_eq!(choose_condition([0]), "second");
    assert_eq!(choose_condition(Some(0)), "third");
    assert_eq!(choose_condition(Some(5)), "second");
}

#[test]
fn string_zero_and_zero_floats_are_falsy_but_nonzero_values_are_truthy() {
    assert_eq!(choose_condition("0"), "third");
    assert_eq!(choose_condition(0.0_f64), "third");
    assert_eq!(choose_condition(f64::NAN), "second");
    assert_eq!(choose_condition(vec![0]), "second");
}

#[test]
fn condition_function_uses_fallback_when_check_is_false_even_if_value_is_missing() {
    let selected: Result<u64, &'static str> =
        framework_core::condition(Ok(false), Err("value missing"), 1000_u64);

    assert_eq!(selected, Ok(1000));
}

#[test]
fn condition_function_returns_selected_value_when_check_is_true() {
    let selected = framework_core::condition(
        Ok::<bool, &'static str>(true),
        Ok::<u64, &'static str>(2500),
        1000_u64,
    );

    assert_eq!(selected, Ok(2500));
}

#[test]
fn condition_function_propagates_check_and_selected_value_errors() {
    let check_error = framework_core::condition(
        Err::<bool, _>("could not read presence"),
        Ok(2500_u64),
        1000_u64,
    );
    let value_error =
        framework_core::condition(Ok(true), Err::<u64, _>("invalid integer"), 1000_u64);

    assert_eq!(check_error, Err("could not read presence"));
    assert_eq!(value_error, Err("invalid integer"));
}

fn choose_condition(value: impl ConditionValue) -> &'static str {
    condition!(value, "second", "third")
}
