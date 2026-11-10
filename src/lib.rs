use chumsky::{prelude::*, pratt::*};
use serde::Serialize;
use std::collections::BTreeSet;
use wasm_bindgen::prelude::*;

const MAX_FORMULA: usize = 1024;
const MAX_NODES: usize = 500;

#[derive(Clone, Debug)]
enum Expr {
    Number(f64),
    Variable(String),
    Unary(&'static str, Box<Expr>),
    Binary(&'static str, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
    Conditional(Box<Expr>, Box<Expr>, Box<Expr>),
}

fn unary(op: &'static str, expr: Expr) -> Expr { Expr::Unary(op, Box::new(expr)) }
fn binary(op: &'static str, a: Expr, b: Expr) -> Expr {
    Expr::Binary(op, Box::new(a), Box::new(b))
}

fn parser<'a>() -> impl Parser<'a, &'a str, Expr, extra::Err<Rich<'a, char>>> {
    recursive(|expr| {
        let number = text::int::<_, extra::Err<Rich<char>>>(10)
            .then(just('.').then(text::digits(10)).or_not())
            .to_slice()
            .map(|s: &str| Expr::Number(s.parse().unwrap()));

        // Braces are mandatory. This also removes ambiguity with the dice operator.
        let reference = just('{')
            .ignore_then(any().filter(|c: &char| *c != '}')
                .repeated().at_least(1).to_slice())
            .then_ignore(just('}'))
            .map(|name: &str| Expr::Variable(name.to_owned()));

        let function = one_of("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ_")
            .then(one_of("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ_0123456789").repeated())
            .to_slice()
            .then(expr.clone().separated_by(just(',').padded()).collect::<Vec<_>>()
                .delimited_by(just('(').padded(), just(')').padded()))
            .map(|(name, args): (&str, Vec<Expr>)| Expr::Call(name.to_owned(), args));

        let atom = choice((
            number,
            function,
            reference,
            expr.clone().delimited_by(just('('), just(')')),
        )).padded();

        let operators = atom.pratt((
            prefix(7, just('-').padded(), |_, x, _| unary("-", x)),
            prefix(7, just('+').padded(), |_, x, _| x),
            infix(right(9), just('^').padded(), |a, _, b, _| binary("^", a, b)),
            infix(left(8), just('d').padded(), |a, _, b, _| binary("d", a, b)),
            infix(left(6), just('*').padded(), |a, _, b, _| binary("*", a, b)),
            infix(left(6), just('/').padded(), |a, _, b, _| binary("/", a, b)),
            infix(left(6), just('%').padded(), |a, _, b, _| binary("%", a, b)),
            infix(left(5), just('+').padded(), |a, _, b, _| binary("+", a, b)),
            infix(left(5), just('-').padded(), |a, _, b, _| binary("-", a, b)),
            infix(left(4), just(">=").padded(), |a, _, b, _| binary(">=", a, b)),
            infix(left(4), just("<=").padded(), |a, _, b, _| binary("<=", a, b)),
            infix(left(4), just("==").padded(), |a, _, b, _| binary("==", a, b)),
            infix(left(4), just("!=").padded(), |a, _, b, _| binary("!=", a, b)),
            infix(left(4), just('>').padded(), |a, _, b, _| binary(">", a, b)),
            infix(left(4), just('<').padded(), |a, _, b, _| binary("<", a, b)),
        ));
        operators.clone().then(
            just('?').padded().ignore_then(expr.clone())
                .then_ignore(just(':').padded())
                .then(expr.clone())
                .or_not()
        ).map(|(condition, choice)| match choice {
            Some((yes, no)) => Expr::Conditional(Box::new(condition), Box::new(yes), Box::new(no)),
            None => condition,
        })
    }).then_ignore(end())
}

fn parse(source: &str) -> Result<Expr, String> {
    if source.len() > MAX_FORMULA { return Err("Formula exceeds 1024 characters".into()); }
    let ast = parser().parse(source).into_result()
        .map_err(|errors| errors.into_iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; "))?;
    let mut nodes = 0;
    fn inspect(e: &Expr, nodes: &mut usize) -> Result<(), String> {
        *nodes += 1;
        if *nodes > MAX_NODES { return Err("Formula is too complex".into()); }
        match e {
            Expr::Unary(_, x) => inspect(x, nodes)?,
            Expr::Binary(_, a, b) => { inspect(a, nodes)?; inspect(b, nodes)?; }
            Expr::Conditional(a, b, c) => { inspect(a, nodes)?; inspect(b, nodes)?; inspect(c, nodes)?; }
            Expr::Call(name, args) => {
                let valid = match name.as_str() {
                    "min" | "max" => !args.is_empty(),
                    "kh" | "dl" => args.len() == 2,
                    "floor" | "ceil" | "round" | "abs" | "sqrt" => args.len() == 1,
                    _ => false,
                };
                if !valid { return Err(format!("Unsupported function or invalid arity: {name}")); }
                for arg in args { inspect(arg, nodes)?; }
            }
            _ => {}
        }
        Ok(())
    }
    inspect(&ast, &mut nodes)?;
    Ok(ast)
}
fn gather(expr: &Expr, refs: &mut BTreeSet<String>, dice: &mut bool) {
    match expr {
        Expr::Variable(name) => { refs.insert(name.clone()); }
        Expr::Unary(_, e) => gather(e, refs, dice),
        Expr::Binary(op, a, b) => {
            if *op == "d" { *dice = true; }
            gather(a, refs, dice);
            gather(b, refs, dice);
        }
        Expr::Call(_, args) => for a in args { gather(a, refs, dice); },
        Expr::Conditional(a, b, c) => {
            gather(a, refs, dice); gather(b, refs, dice); gather(c, refs, dice);
        }
        Expr::Number(_) => {}
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct DiceResult { rolls: Vec<u32>, kept: Vec<bool>, total: f64 }
impl DiceResult {
    fn total(&mut self) {
        self.total = self.rolls.iter().zip(&self.kept)
            .filter(|(_, keep)| **keep).map(|(n, _)| *n as f64).sum();
    }
}
#[derive(Clone, Debug)]
enum Value { Number(f64), Bool(bool), Dice(usize) }
impl Value {
    fn number(&self, log: &[DiceResult]) -> Result<f64, String> {
        Ok(match self {
            Value::Number(n) => *n,
            Value::Bool(b) => if *b { 1.0 } else { 0.0 },
            Value::Dice(i) => log[*i].total,
        })
    }
    fn truth(&self, log: &[DiceResult]) -> Result<bool, String> {
        Ok(match self { Value::Bool(b) => *b, _ => self.number(log)? != 0.0 })
    }
}
#[derive(Serialize)]
struct Outcome { value: BaseValue, dice: Vec<DiceResult> }
#[derive(Serialize)]
#[serde(untagged)]
enum BaseValue { Number(f64), Bool(bool) }
fn finite(n: f64) -> Result<f64, String> {
    if n.is_finite() { Ok(n) } else { Err("Non-finite result".into()) }
}
fn checked_integer(n: f64, low: f64, high: f64, label: &str) -> Result<u32, String> {
    if !n.is_finite() || n.fract() != 0.0 || n < low || n > high {
        return Err(format!("Invalid {label}: expected integer between {low} and {high}"));
    }
    Ok(n as u32)
}
fn evaluate(expr: &Expr, log: &mut Vec<DiceResult>, can_roll: bool) -> Result<Value, String> {
    match expr {
        Expr::Number(n) => Ok(Value::Number(*n)),
        Expr::Variable(name) => Err(format!("Unresolved field: {name}")),
        Expr::Unary(_, e) => {
            let value = evaluate(e, log, can_roll)?.number(log)?;
            Ok(Value::Number(finite(-value)?))
        }
        Expr::Conditional(c, a, b) => {
            if evaluate(c, log, can_roll)?.truth(log)? {
                evaluate(a, log, can_roll)
            } else { evaluate(b, log, can_roll) }
        }
        Expr::Binary(op, a, b) => {
            let left = evaluate(a, log, can_roll)?.number(log)?;
            let right = evaluate(b, log, can_roll)?.number(log)?;
            if *op == "d" {
                if !can_roll { return Err("Roll requires an explicit action".into()); }
                let count = checked_integer(left, 1.0, 100.0, "dice count")?;
                let sides = checked_integer(right, 2.0, 1000.0, "dice sides")?;
                let rolls: Vec<u32> = (0..count).map(|_| {
                    (js_sys::Math::random() * sides as f64).floor() as u32 + 1
                }).collect();
                let mut pool = DiceResult { kept: vec![true; rolls.len()], rolls, total: 0.0 };
                pool.total();
                log.push(pool);
                return Ok(Value::Dice(log.len() - 1));
            }
            let result = match *op {
                ">" => return Ok(Value::Bool(left > right)),
                "<" => return Ok(Value::Bool(left < right)),
                ">=" => return Ok(Value::Bool(left >= right)),
                "<=" => return Ok(Value::Bool(left <= right)),
                "==" => return Ok(Value::Bool(left == right)),
                "!=" => return Ok(Value::Bool(left != right)),
                "+" => left + right, "-" => left - right,
                "*" => left * right, "/" => left / right,
                "^" => left.powf(right), "%" => left % right,
                _ => return Err(format!("Unsupported operator: {op}")),
            };
            Ok(Value::Number(finite(result)?))
        }
        Expr::Call(name, args) => {
            if name == "kh" || name == "dl" {
                let first = evaluate(&args[0], log, can_roll)?;
                let index = match first {
                    Value::Dice(i) => i,
                    _ => return Err(format!("{name} requires a dice result")),
                };
                let amount = evaluate(&args[1], log, can_roll)?.number(log)?;
                let count = checked_integer(amount, 0.0, log[index].rolls.len() as f64, "keep/drop count")? as usize;
                let rolls = &log[index].rolls;
                let mut indices: Vec<usize> = (0..rolls.len()).collect();
                indices.sort_by(|&a, &b| {
                    if name == "kh" { rolls[b].cmp(&rolls[a]) } else { rolls[a].cmp(&rolls[b]) }
                });
                let mut kept = vec![name == "dl"; rolls.len()];
                for &i in indices.iter().take(count) { kept[i] = name == "kh"; }
                log[index].kept = kept;
                log[index].total();
                return Ok(Value::Dice(index));
            }
            let nums: Vec<f64> = args.iter()
                .map(|e| evaluate(e, log, can_roll).and_then(|v| v.number(log)))
                .collect::<Result<_, _>>()?;
            let answer = match name.as_str() {
                "min" => nums.iter().copied().reduce(f64::min).unwrap(),
                "max" => nums.iter().copied().reduce(f64::max).unwrap(),
                "floor" => nums[0].floor(),
                "ceil" => nums[0].ceil(),
                "round" => (nums[0] + 0.5).floor(),
                "abs" => nums[0].abs(),
                "sqrt" => nums[0].sqrt(),
                _ => return Err(format!("Unknown function: {name}")),
            };
            Ok(Value::Number(finite(answer)?))
        }
    }
}

#[wasm_bindgen]
pub struct CompiledFormula {
    ast: Expr,
    deps: Vec<String>,
    has_dice: bool,
}

#[wasm_bindgen]
impl CompiledFormula {
    #[wasm_bindgen(constructor)]
    pub fn new(source: &str) -> Result<CompiledFormula, JsValue> {
        let ast = parse(source).map_err(|e| JsValue::from_str(&e))?;
        let mut deps = BTreeSet::new();
        let mut has_dice = false;
        gather(&ast, &mut deps, &mut has_dice);
        Ok(Self { ast, deps: deps.into_iter().collect(), has_dice })
    }
    pub fn dependencies(&self) -> Result<JsValue, JsValue> {
        serde_wasm_bindgen::to_value(&self.deps).map_err(|e| JsValue::from_str(&e.to_string()))
    }
    pub fn rollable(&self) -> bool { self.has_dice }
    pub fn evaluate(&self) -> Result<JsValue, JsValue> { self.run(false) }
    pub fn roll(&self) -> Result<JsValue, JsValue> { self.run(true) }
    fn run(&self, allow_dice: bool) -> Result<JsValue, JsValue> {
        let mut dice = Vec::new();
        let value = evaluate(&self.ast, &mut dice, allow_dice)
            .map_err(|e| JsValue::from_str(&e))?;
        let value = match value {
            Value::Bool(b) => BaseValue::Bool(b),
            other => BaseValue::Number(finite(other.number(&dice).map_err(|e| JsValue::from_str(&e))?)
                .map_err(|e| JsValue::from_str(&e))?),
        };
        serde_wasm_bindgen::to_value(&Outcome { value, dice })
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_dice_operands() {
        let expr = parse("({STR}+2)d(max(6,{DEX}))").unwrap();
        let mut refs = BTreeSet::new();
        let mut rollable = false;
        gather(&expr, &mut refs, &mut rollable);
        assert!(rollable);
        assert_eq!(refs.into_iter().collect::<Vec<_>>(), vec!["DEX", "STR"]);
    }

    #[test]
    fn parses_nested_functions_and_comparisons() {
        let expr = parse("min(kh(4d8,2),floor(({STR}-10)/2)) > 3").unwrap();
        let mut refs = BTreeSet::new();
        let mut rollable = false;
        gather(&expr, &mut refs, &mut rollable);
        assert!(rollable);
        assert!(refs.contains("STR"));
    }

    #[test]
    fn bare_references_are_rejected() {
        assert!(parse("STR + 2").is_err());
        assert!(parse("{STR} + 2").is_ok());
    }

    #[test]
    fn deterministic_math_and_boolean_results() {
        let ast = parse("min(3,7,4) + 2 * 4").unwrap();
        let value = evaluate(&ast, &mut vec![], false).unwrap();
        assert_eq!(value.number(&[]).unwrap(), 11.0);
        let comparison = parse("3 < 4").unwrap();
        assert!(matches!(evaluate(&comparison, &mut vec![], false).unwrap(), Value::Bool(true)));
    }

    #[test]
    fn dice_cannot_roll_during_normal_evaluation() {
        let ast = parse("3d8 + 1").unwrap();
        assert!(evaluate(&ast, &mut vec![], false).is_err());
    }
}
