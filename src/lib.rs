use chumsky::{pratt::*, prelude::*};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use wasm_bindgen::prelude::*;

const MAX_FORMULA: usize = 1024;
const MAX_NODES: usize = 500;
const MAX_STEPS: usize = 100_000;
const MAX_DEPTH: usize = 128;
const MAX_DISPLAY: usize = 65_536;

#[derive(Clone, Debug, PartialEq)]
enum Expr {
    Number(f64),
    Bool(bool),
    Variable(String),
    Unary(&'static str, Rc<Expr>),
    Binary(&'static str, Rc<Expr>, Rc<Expr>),
    Call(String, Vec<Rc<Expr>>),
    Conditional(Rc<Expr>, Rc<Expr>, Rc<Expr>),
}

fn unary(op: &'static str, expr: Expr) -> Expr {
    Expr::Unary(op, Rc::new(expr))
}
fn binary(op: &'static str, a: Expr, b: Expr) -> Expr {
    Expr::Binary(op, Rc::new(a), Rc::new(b))
}

fn parser<'a>() -> impl Parser<'a, &'a str, Expr, extra::Err<Rich<'a, char>>> {
    recursive(|expr| {
        let number = text::int::<_, extra::Err<Rich<char>>>(10)
            .then(just('.').then(text::digits(10)).or_not())
            .to_slice()
            .map(|s: &str| Expr::Number(s.parse().unwrap()));

        // Braces are mandatory. This also removes ambiguity with the dice operator.
        let reference = just('{')
            .ignore_then(
                any()
                    .filter(|c: &char| *c != '}')
                    .repeated()
                    .at_least(1)
                    .to_slice(),
            )
            .then_ignore(just('}'))
            .map(|name: &str| Expr::Variable(name.to_owned()));

        let function = one_of("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ_")
            .then(
                one_of("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ_0123456789")
                    .repeated(),
            )
            .to_slice()
            .then(
                expr.clone()
                    .separated_by(just(',').padded())
                    .collect::<Vec<_>>()
                    .delimited_by(just('(').padded(), just(')').padded()),
            )
            .map(|(name, args): (&str, Vec<Expr>)| {
                Expr::Call(name.to_owned(), args.into_iter().map(Rc::new).collect())
            });

        let atom = choice((
            number,
            function,
            just("true").to(Expr::Bool(true)),
            just("false").to(Expr::Bool(false)),
            reference,
            expr.clone().delimited_by(just('('), just(')')),
        ))
        .padded();

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
            infix(left(4), just(">=").padded(), |a, _, b, _| {
                binary(">=", a, b)
            }),
            infix(left(4), just("<=").padded(), |a, _, b, _| {
                binary("<=", a, b)
            }),
            infix(left(4), just("==").padded(), |a, _, b, _| {
                binary("==", a, b)
            }),
            infix(left(4), just("!=").padded(), |a, _, b, _| {
                binary("!=", a, b)
            }),
            infix(left(4), just('>').padded(), |a, _, b, _| binary(">", a, b)),
            infix(left(4), just('<').padded(), |a, _, b, _| binary("<", a, b)),
        ));
        operators
            .clone()
            .then(
                just('?')
                    .padded()
                    .ignore_then(expr.clone())
                    .then_ignore(just(':').padded())
                    .then(expr.clone())
                    .or_not(),
            )
            .map(|(condition, choice)| match choice {
                Some((yes, no)) => Expr::Conditional(Rc::new(condition), Rc::new(yes), Rc::new(no)),
                None => condition,
            })
    })
    .then_ignore(end())
}

fn parse(source: &str) -> Result<Expr, String> {
    if source.len() > MAX_FORMULA {
        return Err("Formula exceeds 1024 characters".into());
    }
    let ast = parser().parse(source).into_result().map_err(|errors| {
        errors
            .into_iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("; ")
    })?;
    let mut nodes = 0;
    fn inspect(e: &Expr, nodes: &mut usize) -> Result<(), String> {
        *nodes += 1;
        if *nodes > MAX_NODES {
            return Err("Formula is too complex".into());
        }
        match e {
            Expr::Unary(_, x) => inspect(x, nodes)?,
            Expr::Binary(_, a, b) => {
                inspect(a, nodes)?;
                inspect(b, nodes)?;
            }
            Expr::Conditional(a, b, c) => {
                inspect(a, nodes)?;
                inspect(b, nodes)?;
                inspect(c, nodes)?;
            }
            Expr::Call(name, args) => {
                let valid = match name.as_str() {
                    "min" | "max" => !args.is_empty(),
                    "kh" | "dl" => args.len() == 2,
                    "floor" | "ceil" | "round" | "abs" | "sqrt" => args.len() == 1,
                    _ => false,
                };
                if !valid {
                    return Err(format!("Unsupported function or invalid arity: {name}"));
                }
                for arg in args {
                    inspect(arg, nodes)?;
                }
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
        Expr::Variable(name) => {
            refs.insert(name.clone());
        }
        Expr::Unary(_, e) => gather(e, refs, dice),
        Expr::Binary(op, a, b) => {
            if *op == "d" {
                *dice = true;
            }
            gather(a, refs, dice);
            gather(b, refs, dice);
        }
        Expr::Call(_, args) => {
            for a in args {
                gather(a, refs, dice);
            }
        }
        Expr::Conditional(a, b, c) => {
            gather(a, refs, dice);
            gather(b, refs, dice);
            gather(c, refs, dice);
        }
        Expr::Number(_) | Expr::Bool(_) => {}
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct DiceResult {
    rolls: Vec<u32>,
    kept: Vec<bool>,
    total: f64,
}
impl DiceResult {
    fn total(&mut self) {
        self.total = self
            .rolls
            .iter()
            .zip(&self.kept)
            .filter(|(_, keep)| **keep)
            .map(|(n, _)| *n as f64)
            .sum();
    }
}
#[derive(Clone, Debug)]
enum Value {
    Number(f64),
    Bool(bool),
    Dice(usize),
}
impl Value {
    fn number(&self, log: &[DiceResult]) -> Result<f64, String> {
        Ok(match self {
            Value::Number(n) => *n,
            Value::Bool(b) => {
                if *b {
                    1.0
                } else {
                    0.0
                }
            }
            Value::Dice(i) => log[*i].total,
        })
    }
    fn truth(&self, log: &[DiceResult]) -> Result<bool, String> {
        Ok(match self {
            Value::Bool(b) => *b,
            _ => self.number(log)? != 0.0,
        })
    }
}
#[derive(Serialize)]
struct Outcome {
    value: BaseValue,
    dice: Vec<DiceResult>,
}
#[derive(Serialize)]
#[serde(untagged)]
enum BaseValue {
    Number(f64),
    Bool(bool),
}
fn finite(n: f64) -> Result<f64, String> {
    if n.is_finite() {
        Ok(n)
    } else {
        Err("Non-finite result".into())
    }
}
fn checked_integer(n: f64, low: f64, high: f64, label: &str) -> Result<u32, String> {
    if !n.is_finite() || n.fract() != 0.0 || n < low || n > high {
        return Err(format!(
            "Invalid {label}: expected integer between {low} and {high}"
        ));
    }
    Ok(n as u32)
}
struct Evaluation<'a> {
    fields: &'a BTreeMap<String, CompiledFormula>,
    log: Vec<DiceResult>,
    can_roll: bool,
    steps: usize,
    depth: usize,
}
impl<'a> Evaluation<'a> {
    fn new(fields: &'a BTreeMap<String, CompiledFormula>, can_roll: bool) -> Self {
        Self {
            fields,
            log: vec![],
            can_roll,
            steps: 0,
            depth: 0,
        }
    }
    fn evaluate(&mut self, expr: &Expr) -> Result<Value, String> {
        self.steps += 1;
        if self.steps > MAX_STEPS || self.depth >= MAX_DEPTH {
            return Err("Evaluation is too complex".into());
        }
        self.depth += 1;
        let result = self.inner(expr);
        self.depth -= 1;
        result
    }
    fn inner(&mut self, expr: &Expr) -> Result<Value, String> {
        match expr {
            Expr::Number(n) => Ok(Value::Number(*n)),
            Expr::Bool(b) => Ok(Value::Bool(*b)),
            Expr::Variable(name) => {
                let ast = self
                    .fields
                    .get(name)
                    .ok_or_else(|| format!("Unknown field: {name}"))?
                    .ast
                    .clone();
                self.evaluate(&ast)
            }
            Expr::Unary(_, e) => {
                let value = self.evaluate(e)?.number(&self.log)?;
                Ok(Value::Number(finite(-value)?))
            }
            Expr::Conditional(c, a, b) => {
                if self.evaluate(c)?.truth(&self.log)? {
                    self.evaluate(a)
                } else {
                    self.evaluate(b)
                }
            }
            Expr::Binary(op, a, b) => {
                let left = self.evaluate(a)?.number(&self.log)?;
                let right = self.evaluate(b)?.number(&self.log)?;
                if *op == "d" {
                    if !self.can_roll {
                        return Err("Roll requires an explicit action".into());
                    }
                    let count = checked_integer(left, 1.0, 100.0, "dice count")?;
                    let sides = checked_integer(right, 2.0, 1000.0, "dice sides")?;
                    let rolls: Vec<u32> = (0..count)
                        .map(|_| (js_sys::Math::random() * sides as f64).floor() as u32 + 1)
                        .collect();
                    let mut pool = DiceResult {
                        kept: vec![true; rolls.len()],
                        rolls,
                        total: 0.0,
                    };
                    pool.total();
                    self.log.push(pool);
                    return Ok(Value::Dice(self.log.len() - 1));
                }
                let result = match *op {
                    ">" => return Ok(Value::Bool(left > right)),
                    "<" => return Ok(Value::Bool(left < right)),
                    ">=" => return Ok(Value::Bool(left >= right)),
                    "<=" => return Ok(Value::Bool(left <= right)),
                    "==" => return Ok(Value::Bool(left == right)),
                    "!=" => return Ok(Value::Bool(left != right)),
                    "+" => left + right,
                    "-" => left - right,
                    "*" => left * right,
                    "/" => left / right,
                    "^" => left.powf(right),
                    "%" => left % right,
                    _ => return Err(format!("Unsupported operator: {op}")),
                };
                Ok(Value::Number(finite(result)?))
            }
            Expr::Call(name, args) => {
                if name == "kh" || name == "dl" {
                    let first = self.evaluate(&args[0])?;
                    let index = match first {
                        Value::Dice(i) => i,
                        _ => return Err(format!("{name} requires a dice result")),
                    };
                    let amount = self.evaluate(&args[1])?.number(&self.log)?;
                    let count = checked_integer(
                        amount,
                        0.0,
                        self.log[index].rolls.len() as f64,
                        "keep/drop count",
                    )? as usize;
                    let rolls = &self.log[index].rolls;
                    let mut indices: Vec<usize> = (0..rolls.len()).collect();
                    indices.sort_by(|&a, &b| {
                        if name == "kh" {
                            rolls[b].cmp(&rolls[a])
                        } else {
                            rolls[a].cmp(&rolls[b])
                        }
                    });
                    let mut kept = vec![name == "dl"; rolls.len()];
                    for &i in indices.iter().take(count) {
                        kept[i] = name == "kh";
                    }
                    self.log[index].kept = kept;
                    self.log[index].total();
                    return Ok(Value::Dice(index));
                }
                let nums: Vec<f64> = args
                    .iter()
                    .map(|e| self.evaluate(e).and_then(|v| v.number(&self.log)))
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
}

#[derive(Clone)]
#[wasm_bindgen]
pub struct CompiledFormula {
    ast: Rc<Expr>,
    deps: Rc<Vec<String>>,
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
        Ok(Self {
            ast: Rc::new(ast),
            deps: Rc::new(deps.into_iter().collect()),
            has_dice,
        })
    }
    pub fn dependencies(&self) -> Result<JsValue, JsValue> {
        serde_wasm_bindgen::to_value(self.deps.as_ref())
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }
    pub fn rollable(&self) -> bool {
        self.has_dice
    }
    pub fn evaluate(&self) -> Result<JsValue, JsValue> {
        self.run(false)
    }
    pub fn roll(&self) -> Result<JsValue, JsValue> {
        self.run(true)
    }
    fn run(&self, allow_dice: bool) -> Result<JsValue, JsValue> {
        let fields = BTreeMap::new();
        run(&self.ast, &fields, allow_dice)
            .and_then(|outcome| serde_wasm_bindgen::to_value(&outcome).map_err(|e| e.to_string()))
            .map_err(|e| JsValue::from_str(&e))
    }
}

fn run(
    ast: &Expr,
    fields: &BTreeMap<String, CompiledFormula>,
    can_roll: bool,
) -> Result<Outcome, String> {
    let mut evaluation = Evaluation::new(fields, can_roll);
    let value = evaluation.evaluate(ast)?;
    let value = match value {
        Value::Bool(b) => BaseValue::Bool(b),
        other => BaseValue::Number(finite(other.number(&evaluation.log)?)?),
    };
    Ok(Outcome {
        value,
        dice: evaluation.log,
    })
}

#[derive(Clone)]
struct Reduced {
    ast: Rc<Expr>,
    rollable: bool,
}
fn literal(expr: &Expr) -> bool {
    matches!(expr, Expr::Number(_) | Expr::Bool(_))
}

// Cache each referenced reduction for this operation. Repeated dice references
// share a tree, never a rolled value; each occurrence is evaluated independently.
struct Reduction<'a> {
    fields: &'a BTreeMap<String, CompiledFormula>,
    cache: BTreeMap<String, Reduced>,
    steps: usize,
    depth: usize,
}
impl<'a> Reduction<'a> {
    fn new(fields: &'a BTreeMap<String, CompiledFormula>) -> Self {
        Self {
            fields,
            cache: BTreeMap::new(),
            steps: 0,
            depth: 0,
        }
    }
    fn field(&mut self, name: &str) -> Result<Reduced, String> {
        if let Some(previous) = self.cache.get(name) {
            return Ok(previous.clone());
        }
        let ast = self
            .fields
            .get(name)
            .ok_or_else(|| format!("Unknown field: {name}"))?
            .ast
            .clone();
        let reduced = self.reduce(&ast)?;
        self.cache.insert(name.to_owned(), reduced.clone());
        Ok(reduced)
    }
    fn reduce(&mut self, expr: &Rc<Expr>) -> Result<Reduced, String> {
        self.steps += 1;
        if self.steps > MAX_STEPS || self.depth >= MAX_DEPTH {
            return Err("Resolved formula is too complex".into());
        }
        self.depth += 1;
        let result = self.inner(expr);
        self.depth -= 1;
        result
    }
    fn inner(&mut self, expr: &Rc<Expr>) -> Result<Reduced, String> {
        let (ast, rollable, fold) = match expr.as_ref() {
            Expr::Number(_) | Expr::Bool(_) => {
                return Ok(Reduced {
                    ast: expr.clone(),
                    rollable: false,
                })
            }
            Expr::Variable(name) => return self.field(name),
            Expr::Unary(op, child) => {
                let child = self.reduce(child)?;
                let fold = literal(&child.ast);
                (Expr::Unary(op, child.ast), child.rollable, fold)
            }
            Expr::Binary(op, left, right) => {
                let left = self.reduce(left)?;
                let right = self.reduce(right)?;
                let fold = *op != "d" && literal(&left.ast) && literal(&right.ast);
                let rollable = *op == "d" || left.rollable || right.rollable;
                (Expr::Binary(op, left.ast, right.ast), rollable, fold)
            }
            Expr::Call(name, args) => {
                let args = args
                    .iter()
                    .map(|arg| self.reduce(arg))
                    .collect::<Result<Vec<_>, _>>()?;
                let rollable = args.iter().any(|arg| arg.rollable);
                let fold = args.iter().all(|arg| literal(&arg.ast));
                (
                    Expr::Call(name.clone(), args.into_iter().map(|arg| arg.ast).collect()),
                    rollable,
                    fold,
                )
            }
            Expr::Conditional(condition, yes, no) => {
                let condition = self.reduce(condition)?;
                let choice = match condition.ast.as_ref() {
                    Expr::Number(n) => Some(*n != 0.0),
                    Expr::Bool(b) => Some(*b),
                    _ => None,
                };
                if let Some(choice) = choice {
                    return self.reduce(if choice { yes } else { no });
                }
                let yes = self.reduce(yes)?;
                let no = self.reduce(no)?;
                let rollable = condition.rollable || yes.rollable || no.rollable;
                (
                    Expr::Conditional(condition.ast, yes.ast, no.ast),
                    rollable,
                    false,
                )
            }
        };
        if fold {
            // Leave failed computations intact: an unknown conditional may never
            // execute this branch. Normal evaluation reports errors when reached.
            let fields = BTreeMap::new();
            let mut evaluation = Evaluation::new(&fields, false);
            match evaluation.evaluate(&ast) {
                Ok(Value::Number(n)) => {
                    return Ok(Reduced {
                        ast: Rc::new(Expr::Number(n)),
                        rollable: false,
                    })
                }
                Ok(Value::Bool(b)) => {
                    return Ok(Reduced {
                        ast: Rc::new(Expr::Bool(b)),
                        rollable: false,
                    })
                }
                _ => {}
            }
        }
        Ok(Reduced {
            ast: Rc::new(ast),
            rollable,
        })
    }
}

fn precedence(expr: &Expr) -> u8 {
    match expr {
        Expr::Conditional(..) => 0,
        Expr::Binary(op, ..) => match *op {
            "^" => 9,
            "d" => 8,
            "*" | "/" | "%" => 6,
            "+" | "-" => 5,
            _ => 4,
        },
        Expr::Unary(..) => 7,
        Expr::Number(n) if n.is_sign_negative() => 7,
        _ => 10,
    }
}
fn is_prefix(expr: &Expr) -> bool {
    matches!(expr, Expr::Unary(..)) || matches!(expr, Expr::Number(n) if n.is_sign_negative())
}
struct Formatter {
    text: String,
    steps: usize,
    depth: usize,
}
impl Formatter {
    fn push(&mut self, text: &str) -> Result<(), String> {
        if self.text.len() + text.len() > MAX_DISPLAY {
            return Err("Resolved display is too large".into());
        }
        self.text.push_str(text);
        Ok(())
    }
    fn child(&mut self, expr: &Expr, wrap: bool, following: Option<u8>) -> Result<(), String> {
        if wrap {
            self.push("(")?;
        }
        self.expression(expr, if wrap { None } else { following })?;
        if wrap {
            self.push(")")?;
        }
        Ok(())
    }
    fn expression(&mut self, expr: &Expr, following: Option<u8>) -> Result<(), String> {
        self.steps += 1;
        if self.steps > MAX_STEPS || self.depth >= MAX_DEPTH {
            return Err("Resolved display is too complex".into());
        }
        self.depth += 1;
        let result = self.inner(expr, following);
        self.depth -= 1;
        result
    }
    fn inner(&mut self, expr: &Expr, following: Option<u8>) -> Result<(), String> {
        match expr {
            Expr::Number(n) => self.push(&n.to_string()),
            Expr::Bool(b) => self.push(if *b { "true" } else { "false" }),
            Expr::Variable(name) => {
                self.push("{")?;
                self.push(name)?;
                self.push("}")
            }
            Expr::Unary(op, child) => {
                self.push(op)?;
                self.child(child, precedence(child) < 7, following)
            }
            Expr::Binary(op, left, right) => {
                let p = precedence(expr);
                let lp = precedence(left);
                let rp = precedence(right);
                self.child(left, lp < p || (lp == p && *op == "^"), Some(p))?;
                if *op == "d" {
                    self.push("d")?;
                } else {
                    self.push(" ")?;
                    self.push(op)?;
                    self.push(" ")?;
                }
                // Pratt prefix operators can start a right operand even when
                // their binding power is lower. Protect their trailing operand
                // only if an operator outside this expression would capture it.
                let wrap = if is_prefix(right) {
                    following.is_some_and(|next| next > 7)
                } else {
                    rp < p || (rp == p && *op != "^")
                };
                self.child(right, wrap, following)
            }
            Expr::Call(name, args) => {
                self.push(name)?;
                self.push("(")?;
                for (i, arg) in args.iter().enumerate() {
                    if i != 0 {
                        self.push(", ")?;
                    }
                    self.expression(arg, None)?;
                }
                self.push(")")
            }
            Expr::Conditional(condition, yes, no) => {
                self.child(condition, precedence(condition) == 0, None)?;
                self.push(" ? ")?;
                self.expression(yes, None)?;
                self.push(" : ")?;
                self.expression(no, following)
            }
        }
    }
}
fn format_expression(expr: &Expr) -> Result<String, String> {
    let mut formatter = Formatter {
        text: String::new(),
        steps: 0,
        depth: 0,
    };
    formatter.expression(expr, None)?;
    Ok(formatter.text)
}

#[derive(Serialize)]
struct Resolution {
    expression: String,
    value: Option<BaseValue>,
    rollable: bool,
    dependencies: Vec<String>,
}

/// Owns references to compiled formulas, not expanded strings or copied trees.
#[wasm_bindgen]
#[derive(Default)]
pub struct FormulaContext {
    fields: BTreeMap<String, CompiledFormula>,
}
impl FormulaContext {
    fn dependencies_for(&self, root: &str) -> Result<Vec<String>, String> {
        fn visit(
            context: &FormulaContext,
            name: &str,
            active: &mut Vec<String>,
            done: &mut BTreeSet<String>,
            steps: &mut usize,
        ) -> Result<(), String> {
            *steps += 1;
            if *steps > MAX_STEPS || active.len() >= MAX_DEPTH {
                return Err("Reference graph is too complex".into());
            }
            if let Some(index) = active.iter().position(|id| id == name) {
                let mut path = active[index..].to_vec();
                path.push(name.to_owned());
                return Err(format!("Circular reference: {}", path.join(" → ")));
            }
            if done.contains(name) {
                return Ok(());
            }
            let field = context
                .fields
                .get(name)
                .ok_or_else(|| format!("Unknown field: {name}"))?;
            active.push(name.to_owned());
            for reference in field.deps.iter() {
                visit(context, reference, active, done, steps)?;
            }
            active.pop();
            done.insert(name.to_owned());
            Ok(())
        }
        let mut done = BTreeSet::new();
        visit(self, root, &mut Vec::new(), &mut done, &mut 0)?;
        done.remove(root);
        Ok(done.into_iter().collect())
    }
    fn describe(&self, name: &str) -> Result<Resolution, String> {
        let dependencies = self.dependencies_for(name)?;
        let reduced = Reduction::new(&self.fields).field(name)?;
        let value = if reduced.rollable {
            None
        } else {
            Some(run(&reduced.ast, &self.fields, false)?.value)
        };
        Ok(Resolution {
            expression: format_expression(&reduced.ast)?,
            value,
            rollable: reduced.rollable,
            dependencies,
        })
    }
    fn roll_outcome(&self, name: &str) -> Result<Outcome, String> {
        self.dependencies_for(name)?;
        let reduced = Reduction::new(&self.fields).field(name)?;
        run(&reduced.ast, &self.fields, true)
    }
}
#[wasm_bindgen]
impl FormulaContext {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::default()
    }
    pub fn set(&mut self, name: &str, formula: &CompiledFormula) {
        self.fields.insert(name.to_owned(), formula.clone());
    }
    pub fn remove(&mut self, name: &str) {
        self.fields.remove(name);
    }
    pub fn resolve(&self, name: &str) -> Result<JsValue, JsValue> {
        self.describe(name)
            .and_then(|value| serde_wasm_bindgen::to_value(&value).map_err(|e| e.to_string()))
            .map_err(|e| JsValue::from_str(&e))
    }
    pub fn roll(&self, name: &str) -> Result<JsValue, JsValue> {
        self.roll_outcome(name)
            .and_then(|value| serde_wasm_bindgen::to_value(&value).map_err(|e| e.to_string()))
            .map_err(|e| JsValue::from_str(&e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evaluate(expr: &Expr, log: &mut Vec<DiceResult>, can_roll: bool) -> Result<Value, String> {
        let fields = BTreeMap::new();
        let mut evaluation = Evaluation::new(&fields, can_roll);
        let result = evaluation.evaluate(expr);
        *log = evaluation.log;
        result
    }

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
        assert!(matches!(
            evaluate(&comparison, &mut vec![], false).unwrap(),
            Value::Bool(true)
        ));
    }

    #[test]
    fn dice_cannot_roll_during_normal_evaluation() {
        let ast = parse("3d8 + 1").unwrap();
        assert!(evaluate(&ast, &mut vec![], false).is_err());
    }
}

#[cfg(test)]
mod context_tests;
