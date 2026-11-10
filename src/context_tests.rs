use super::*;

fn compiled(source: &str) -> CompiledFormula {
    let ast = parse(source).unwrap();
    let mut deps = BTreeSet::new();
    let mut has_dice = false;
    gather(&ast, &mut deps, &mut has_dice);
    CompiledFormula {
        ast: Rc::new(ast),
        deps: Rc::new(deps.into_iter().collect()),
        has_dice,
    }
}
fn context(fields: &[(&str, &str)]) -> FormulaContext {
    let mut context = FormulaContext::new();
    for (id, source) in fields {
        context.set(id, &compiled(source));
    }
    context
}
fn variable(name: &str) -> Expr {
    Expr::Variable(name.to_owned())
}

#[test]
fn deterministic_references_reduce_to_typed_values() {
    let mut context = context(&[
        ("A", "2 + 3"),
        ("B", "{A} * 4"),
        ("C", "{B} > 19"),
        ("D", "{C} + 2"),
    ]);
    let b = context.describe("B").unwrap();
    assert_eq!(b.expression, "20");
    assert!(!b.rollable);
    assert!(matches!(b.value, Some(BaseValue::Number(20.0))));
    assert_eq!(b.dependencies, vec!["A"]);
    assert_eq!(context.describe("C").unwrap().expression, "true");
    assert_eq!(context.describe("D").unwrap().expression, "3");
    let unchanged = context.fields["B"].ast.clone();
    context.set("A", &compiled("10"));
    assert_eq!(context.describe("B").unwrap().expression, "40");
    assert!(Rc::ptr_eq(&unchanged, &context.fields["B"].ast));
}

#[test]
fn deterministic_parts_fold_without_rolling() {
    let context = context(&[
        ("STAT", "18"),
        ("MOD", "floor(({STAT} - 10) / 2)"),
        ("POOL", "3d6"),
        ("ROOT", "dl({POOL}, 1) + {MOD}"),
        ("REPEATED", "{POOL} + {POOL}"),
    ]);
    let result = context.describe("ROOT").unwrap();
    assert_eq!(result.expression, "dl(3d6, 1) + 4");
    assert!(result.rollable);
    assert!(result.value.is_none());
    assert_eq!(result.dependencies, vec!["MOD", "POOL", "STAT"]);
    let reduced = Reduction::new(&context.fields).field("REPEATED").unwrap();
    match reduced.ast.as_ref() {
        Expr::Binary("+", a, b) => assert!(Rc::ptr_eq(a, b)),
        other => panic!("unexpected tree: {other:?}"),
    }
    assert_eq!(format_expression(&reduced.ast).unwrap(), "3d6 + 3d6");
}

#[test]
fn conditional_reduction_preserves_lazy_evaluation() {
    let context = context(&[
        ("SAFE", "0 ? sqrt(-1) : 3"),
        ("UNUSED_DICE", "1 ? 7 : 2d6"),
        ("CHOICE", "1d6 > 3 ? 1 / 0 : 2 + 3"),
        ("ERROR", "sqrt(-1)"),
    ]);
    assert_eq!(context.describe("SAFE").unwrap().expression, "3");
    assert!(!context.describe("UNUSED_DICE").unwrap().rollable);
    assert_eq!(
        context.describe("CHOICE").unwrap().expression,
        "1d6 > 3 ? 1 / 0 : 5"
    );
    assert!(context.describe("ERROR").is_err());
}

#[test]
fn graph_validation_reports_missing_and_circular_references() {
    let mut context = context(&[("A", "{B} + 1"), ("B", "{C}"), ("C", "{A}")]);
    assert!(context
        .describe("A")
        .err()
        .unwrap()
        .contains("A → B → C → A"));
    context.remove("C");
    assert_eq!(context.describe("A").err().unwrap(), "Unknown field: C");
    context.set("C", &compiled("4"));
    assert_eq!(context.describe("A").unwrap().expression, "5");
    context.set("C", &compiled("0 ? {MISSING} : 1"));
    assert!(context.describe("A").err().unwrap().contains("MISSING"));
}

#[test]
fn deterministic_diamonds_do_not_expand_exponentially() {
    let mut context = context(&[("N0", "1")]);
    for n in 1..50 {
        context.set(
            &format!("N{n}"),
            &compiled(&format!("{{N{}}} + {{N{}}}", n - 1, n - 1)),
        );
    }
    let result = context.describe("N49").unwrap();
    assert_eq!(result.expression, "562949953421312");
    assert_eq!(result.dependencies.len(), 49);
}

#[test]
fn expanded_dice_display_has_a_bounded_size() {
    let mut context = context(&[("N0", "1d6")]);
    for n in 1..20 {
        context.set(
            &format!("N{n}"),
            &compiled(&format!("{{N{}}} + {{N{}}}", n - 1, n - 1)),
        );
    }
    assert!(context
        .describe("N19")
        .err()
        .unwrap()
        .contains("display is too large"));
}

fn check_format(ast: &Expr) {
    let text = format_expression(ast).unwrap();
    let reparsed = parse(&text).unwrap_or_else(|error| panic!("{text}: {error}"));
    assert_eq!(&reparsed, ast, "changed grouping: {text}");
    // Every emitted pair must be necessary to recover the original structure.
    let mut stack = vec![];
    for (index, ch) in text.char_indices() {
        if ch == '(' {
            stack.push(index);
        }
        if ch == ')' {
            let open = stack.pop().unwrap();
            let without = format!(
                "{}{}{}",
                &text[..open],
                &text[open + 1..index],
                &text[index + 1..]
            );
            if let Ok(parsed) = parse(&without) {
                assert_ne!(&parsed, ast, "redundant parentheses: {text}");
            }
        }
    }
}

#[test]
fn formatter_preserves_every_operator_pair_with_minimal_grouping() {
    let ops = [
        "+", "-", "*", "/", "%", "^", "d", ">", "<", ">=", "<=", "==", "!=",
    ];
    for outer in ops {
        for inner in ops {
            check_format(&binary(
                outer,
                binary(inner, variable("A"), variable("B")),
                variable("C"),
            ));
            check_format(&binary(
                outer,
                variable("A"),
                binary(inner, variable("B"), variable("C")),
            ));
            check_format(&binary(
                outer,
                unary("-", binary(inner, variable("A"), variable("B"))),
                variable("C"),
            ));
            check_format(&binary(
                outer,
                variable("A"),
                unary("-", binary(inner, variable("B"), variable("C"))),
            ));
            check_format(&binary(
                outer,
                binary(inner, variable("A"), unary("-", variable("B"))),
                variable("C"),
            ));
        }
    }
}

#[test]
fn formatter_handles_ternaries_functions_and_prefix_power_interactions() {
    for source in [
        "({A} ? {B} : {C}) ? {D} : {E}",
        "{A} ? ({B} ? {C} : {D}) : ({E} ? {F} : {G})",
        "({A} ? {B} : {C}) + {D}",
        "{A} + ({B} ? {C} : {D})",
        "-({A} ? {B} : {C})",
        "floor(({A} + {B}) / {C})",
        "kh(({A} + {B})d({C} * {D}), 2)",
        "({A} ^ (-{B}))d{C}",
        "{A} ^ (-{B})",
        "(-{A}) ^ {B}",
        "-{A} ^ {B}",
        "{A} - ({B} - {C})",
        "{A} / ({B} * {C})",
        "true ? 2 : false",
    ] {
        check_format(&parse(source).unwrap());
    }
}

#[test]
fn folded_negative_numbers_keep_required_grouping_and_signed_zero() {
    let context = context(&[
        ("NEG", "-2"),
        ("POW", "{NEG} ^ (1d6)"),
        ("RIGHT", "(1d6) ^ {NEG}"),
        ("ZERO", "-0"),
    ]);
    assert_eq!(context.describe("POW").unwrap().expression, "(-2) ^ (1d6)");
    assert_eq!(context.describe("RIGHT").unwrap().expression, "(1d6) ^ -2");
    assert_eq!(context.describe("ZERO").unwrap().expression, "-0");
}

#[test]
fn formatter_preserves_three_operator_chains() {
    let ops = ["+", "-", "*", "/", "^", "d", "<"];
    for outer in ops {
        for middle in ops {
            for inner in ops {
                check_format(&binary(
                    outer,
                    binary(
                        middle,
                        binary(inner, variable("A"), variable("B")),
                        variable("C"),
                    ),
                    variable("D"),
                ));
                check_format(&binary(
                    outer,
                    binary(
                        middle,
                        variable("A"),
                        binary(inner, variable("B"), variable("C")),
                    ),
                    variable("D"),
                ));
                check_format(&binary(
                    outer,
                    binary(middle, variable("A"), variable("B")),
                    binary(inner, variable("C"), variable("D")),
                ));
                check_format(&binary(
                    outer,
                    variable("A"),
                    binary(
                        middle,
                        binary(inner, variable("B"), variable("C")),
                        variable("D"),
                    ),
                ));
                check_format(&binary(
                    outer,
                    variable("A"),
                    binary(
                        middle,
                        variable("B"),
                        binary(inner, variable("C"), variable("D")),
                    ),
                ));
                check_format(&binary(
                    outer,
                    binary(
                        middle,
                        variable("A"),
                        binary(inner, variable("B"), unary("-", variable("C"))),
                    ),
                    variable("D"),
                ));
            }
        }
    }
}
