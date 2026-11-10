use chumsky::{prelude::*, pratt::*};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use wasm_bindgen::prelude::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Expr {
    Number { value: f64 },
    Variable { name: String },
    Unary { op: String, expr: Box<Expr> },
    Binary { op: String, left: Box<Expr>, right: Box<Expr> },
    Call { name: String, args: Vec<Expr> },
}

fn unary(op: &str, expr: Expr) -> Expr {
    Expr::Unary { op: op.into(), expr: Box::new(expr) }
}
fn binary(op: &str, left: Expr, right: Expr) -> Expr {
    Expr::Binary { op: op.into(), left: Box::new(left), right: Box::new(right) }
}

fn parser<'a>() -> impl Parser<'a, &'a str, Expr, extra::Err<Rich<'a, char>>> {
    recursive(|expr| {
        let digits = text::int::<_, extra::Err<Rich<char>>>(10)
            .then(just('.').then(text::digits(10)).or_not())
            .to_slice()
            .map(|s: &str| Expr::Number { value: s.parse().unwrap() });

        let variable = just('{')
        .ignore_then(
            any()
                .filter(|c: &char| *c != '}')
                .repeated()
                .at_least(1)
                .to_slice()
        )
        .then_ignore(just('}'))
        .map(|name: &str| Expr::Variable { name: name.to_owned() })
        .padded();

        let function = one_of("abcdefghijklmnopqrstuvwxyz_")
            .then(one_of("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ_0123456789").repeated())
            .to_slice()
            .then(
                expr.clone().separated_by(just(',').padded()).collect::<Vec<_>>()
                    .delimited_by(just('('), just(')'))
            )
            .map(|(name, args): (&str, Vec<Expr>)| Expr::Call {
                name: name.to_string(),
                args,
            });

        let atom = choice((
            digits,
            function,
            variable,
            expr.delimited_by(just('('), just(')')),
        )).padded();

        atom.pratt((
            prefix(7, just('-').padded(), |_, x, _| unary("-", x)),
            prefix(7, just('+').padded(), |_, x, _| x),
            infix(left(8), just('d').padded(), |x, _, y, _| binary("d", x, y)),
            infix(left(6), just('*').padded(), |x, _, y, _| binary("*", x, y)),
            infix(left(6), just('/').padded(), |x, _, y, _| binary("/", x, y)),
            infix(left(5), just('+').padded(), |x, _, y, _| binary("+", x, y)),
            infix(left(5), just('-').padded(), |x, _, y, _| binary("-", x, y)),
            infix(left(4), just(">=").padded(), |x, _, y, _| binary(">=", x, y)),
            infix(left(4), just("<=").padded(), |x, _, y, _| binary("<=", x, y)),
            infix(left(4), just("==").padded(), |x, _, y, _| binary("==", x, y)),
            infix(left(4), just("!=").padded(), |x, _, y, _| binary("!=", x, y)),
            infix(left(4), just('>').padded(), |x, _, y, _| binary(">", x, y)),
            infix(left(4), just('<').padded(), |x, _, y, _| binary("<", x, y)),
        ))
    })
    .then_ignore(end())
}

fn parse_source(source: &str) -> Result<Expr, String> {
    parser().parse(source).into_result()
        .map_err(|errors| errors.into_iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; "))
}

fn dependencies(expr: &Expr, output: &mut BTreeSet<String>) {
    match expr {
        Expr::Variable { name } => { output.insert(name.clone()); }
        Expr::Unary { expr, .. } => dependencies(expr, output),
        Expr::Binary { left, right, .. } => { dependencies(left, output); dependencies(right, output); }
        Expr::Call { args, .. } => for arg in args { dependencies(arg, output); },
        Expr::Number { .. } => {}
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
enum Value { Number(f64), Boolean(bool) }

impl Value {
    fn number(&self) -> Result<f64, String> { match self { Value::Number(n) => Ok(*n), _ => Err("Expected a number".into()) } }
    fn truth(&self) -> Result<bool, String> { match self { Value::Boolean(b) => Ok(*b), _ => Err("Expected a boolean".into()) } }
}

#[derive(Serialize)]
struct RollResult { value: Value, dice: Vec<Vec<u32>> }

fn evaluate(expr: &Expr, vars: &HashMap<String, f64>, dice: &mut Vec<Vec<u32>>) -> Result<Value, String> {
    match expr {
        Expr::Number { value } => Ok(Value::Number(*value)),
        Expr::Variable { name } => vars.get(name).copied().map(Value::Number).ok_or_else(|| format!("Unknown field: {name}")),
        Expr::Unary { op, expr } => {
            let n = evaluate(expr, vars, dice)?.number()?;
            match op.as_str() { "-" => Ok(Value::Number(-n)), _ => Err(format!("Unknown unary operator: {op}")) }
        }
        Expr::Binary { op, left, right } => {
            let a = evaluate(left, vars, dice)?.number()?;
            let b = evaluate(right, vars, dice)?.number()?;
            Ok(match op.as_str() {
                "+" => Value::Number(a + b), "-" => Value::Number(a - b),
                "*" => Value::Number(a * b), "/" => Value::Number(a / b),
                ">" => Value::Boolean(a > b), "<" => Value::Boolean(a < b),
                ">=" => Value::Boolean(a >= b), "<=" => Value::Boolean(a <= b),
                "==" => Value::Boolean(a == b), "!=" => Value::Boolean(a != b),
                "d" => {
                    if !a.is_finite() || !b.is_finite() || a.fract() != 0.0 || b.fract() != 0.0 || a < 0.0 || a > 1000.0 || b < 1.0 || b > 1000000.0 {
                        return Err("Dice count must be 0..1000 and sides 1..1000000, both integers".into());
                    }
                    let mut rolls = Vec::with_capacity(a as usize);
                    for _ in 0..a as u32 {
                        let r = (js_sys::Math::random() * b).floor() as u32 + 1;
                        rolls.push(r);
                    }
                    let sum = rolls.iter().map(|&n| n as f64).sum();
                    dice.push(rolls);
                    Value::Number(sum)
                }
                _ => return Err(format!("Unknown binary operator: {op}"))
            })
        }
        Expr::Call { name, args } => {
            if name == "kh" || name == "dl" {
                if args.len() != 2 { return Err(format!("{name} expects 2 arguments")); }
                // The first argument must be a dice expression, e.g. 4d8.
                if !matches!(&args[0], Expr::Binary { op, .. } if op == "d") {
                    return Err(format!("{name} requires a dice expression as its first argument"));
                }
                let group_start = dice.len();
                let _ = evaluate(&args[0], vars, dice)?;
                let group = dice.get(group_start).ok_or("No dice rolled")?.clone();
                let n = evaluate(&args[1], vars, dice)?.number()?;
                if !n.is_finite() || n < 0.0 || n.fract() != 0.0 { return Err("Invalid keep/drop count".into()); }
                let mut selected = group;
                selected.sort_unstable_by(|a, b| b.cmp(a));
                let take = (n as usize).min(selected.len());
                let result = if name == "kh" {
                    selected[..take].iter().map(|&v| v as f64).sum()
                } else {
                    selected[take..].iter().map(|&v| v as f64).sum()
                };
                return Ok(Value::Number(result));
            }
            let values: Vec<f64> = args.iter().map(|e| evaluate(e, vars, dice)?.number()).collect::<Result<_, _>>()?;
            let value = match name.as_str() {
                "min" => values.iter().copied().reduce(f64::min).ok_or("min requires arguments")?,
                "max" => values.iter().copied().reduce(f64::max).ok_or("max requires arguments")?,
                "floor" if values.len() == 1 => values[0].floor(),
                "ceil" if values.len() == 1 => values[0].ceil(),
                "abs" if values.len() == 1 => values[0].abs(),
                _ => return Err(format!("Unknown function or wrong arity: {name}"))
            };
            Ok(Value::Number(value))
        }
    }
}

#[wasm_bindgen]
pub struct CompiledFormula { ast: Expr, deps: Vec<String> }

#[wasm_bindgen]
impl CompiledFormula {
    #[wasm_bindgen(constructor)]
    pub fn new(source: &str) -> Result<CompiledFormula, JsValue> {
        let ast = parse_source(source).map_err(|e| JsValue::from_str(&e))?;
        let mut deps = BTreeSet::new();
        dependencies(&ast, &mut deps);
        Ok(Self { ast, deps: deps.into_iter().collect() })
    }

    pub fn dependencies(&self) -> Result<JsValue, JsValue> {
        serde_wasm_bindgen::to_value(&self.deps).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    pub fn ast(&self) -> Result<JsValue, JsValue> {
        serde_wasm_bindgen::to_value(&self.ast).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    pub fn evaluate(&self, vars: JsValue) -> Result<JsValue, JsValue> {
        let vars: HashMap<String, f64> = serde_wasm_bindgen::from_value(vars)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        let mut dice = Vec::new();
        let value = evaluate(&self.ast, &vars, &mut dice).map_err(|e| JsValue::from_str(&e))?;
        serde_wasm_bindgen::to_value(&RollResult { value, dice })
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }
}
