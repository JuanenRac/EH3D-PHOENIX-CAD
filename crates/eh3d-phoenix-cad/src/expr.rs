//! The language of expressions of the parameters.
//!
//! An expression is a formula such as `2 * wall + 1 mm` or `if(width > 80 mm, 3 mm, 2 mm)`. It is
//! read by a lexer and a parser with the usual precedence, and evaluated with exact rational
//! numbers of 128 bits: nothing here uses floating point, every operation checks that it does not
//! overflow, and the same expression with the same names always gives the same value. The grammar
//! is written in `docs/EXPRESSIONS.md`.
//!
//! A value is a plain number, a length (kept in nanometres), an angle (kept in nano-degrees) or a
//! truth value. Units are written after a number (`mm`, `um`, `cm`, `m`, `in`, `mil`, `nm`, `deg`,
//! `turn`). Lengths add and subtract with lengths, a number scales a length, a length divided by a
//! length is a number; anything else is refused with a message that says why and where.
//!
//! The limits (length of the text, depth of the tree, steps of an evaluation, largest exponent) are
//! not in the code: they are read from the settings file ([`ExprLimits`]).
//!
//! ```
//! use eh3d_phoenix_cad::config::Settings;
//! use eh3d_phoenix_cad::expr::{parse, Constants};
//!
//! let limits = Settings::defaults().expr;
//! let e = parse("2 * 3 mm + 1 mm", &limits).unwrap();
//! let none = Constants::new();
//! assert_eq!(e.eval_length(&none, &limits).unwrap().get(), 7_000_000);
//! ```

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;

use crate::units::{div_round_half_away, isqrt_u128, Angle, Unit};
use crate::{CadError, Length};

/// The largest value the settings may give to the length of an expression, in characters.
pub const HARD_MAX_LENGTH: u32 = 65_536;

/// The largest value the settings may give to the depth of an expression.
pub const HARD_MAX_DEPTH: u32 = 128;

/// The largest value the settings may give to the steps of one evaluation.
pub const HARD_MAX_STEPS: u32 = 100_000_000;

/// The largest value the settings may give to the exponent of a power.
pub const HARD_MAX_EXPONENT: u32 = 1024;

/// The most digits of a number written in an expression.
const MAX_DIGITS: u32 = 18;

/// The hard limits of the language, read from the settings file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExprLimits {
    /// The most characters of one expression.
    pub max_length: u32,
    /// The deepest nesting of parentheses, signs and operators.
    pub max_depth: u32,
    /// The most steps (nodes evaluated) of one calculation.
    pub max_steps: u32,
    /// The largest absolute value of the exponent of a power.
    pub max_exponent: u32,
}

impl ExprLimits {
    /// Limits checked against the hard caps: each one is at least 1 and at most its cap.
    pub fn new(
        max_length: u32,
        max_depth: u32,
        max_steps: u32,
        max_exponent: u32,
    ) -> Result<ExprLimits, CadError> {
        for (key, value, cap) in [
            ("max_length", max_length, HARD_MAX_LENGTH),
            ("max_depth", max_depth, HARD_MAX_DEPTH),
            ("max_steps", max_steps, HARD_MAX_STEPS),
            ("max_exponent", max_exponent, HARD_MAX_EXPONENT),
        ] {
            if value == 0 || value > cap {
                return Err(CadError::Config(format!(
                    "expr.{key} must be between 1 and {cap}"
                )));
            }
        }
        Ok(ExprLimits {
            max_length,
            max_depth,
            max_steps,
            max_exponent,
        })
    }
}

/// The steps left for the evaluations that share it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Budget {
    left: u32,
}

impl Budget {
    /// A full budget of `limits.max_steps` steps.
    pub fn new(limits: &ExprLimits) -> Budget {
        Budget {
            left: limits.max_steps,
        }
    }

    /// The steps that are left.
    pub fn left(&self) -> u32 {
        self.left
    }

    fn spend(&mut self, pos: usize) -> Result<(), CadError> {
        if self.left == 0 {
            Err(limit(pos, "steps"))
        } else {
            self.left -= 1;
            Ok(())
        }
    }
}

fn syntax(pos: usize, why: impl Into<String>) -> CadError {
    CadError::ExprSyntax {
        pos,
        why: why.into(),
    }
}

fn failure(pos: usize, why: impl Into<String>) -> CadError {
    CadError::ExprEval {
        pos,
        why: why.into(),
    }
}

fn limit(pos: usize, what: &'static str) -> CadError {
    CadError::ExprLimit { pos, what }
}

fn overflow(pos: usize) -> CadError {
    CadError::ExprOverflow { pos }
}

// ---------------------------------------------------------------------------------------------
// Exact numbers
// ---------------------------------------------------------------------------------------------

fn gcd(a: u128, b: u128) -> u128 {
    let (mut a, mut b) = (a, b);
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

/// A rational number of two 128-bit integers, always reduced, with a positive denominator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rat {
    n: i128,
    d: i128,
}

impl Rat {
    /// Zero.
    pub const ZERO: Rat = Rat { n: 0, d: 1 };

    /// A whole number.
    pub fn int(n: i128) -> Rat {
        Rat { n, d: 1 }
    }

    /// `n / d` reduced, or `None` when `d` is zero or the sign cannot be moved without overflow.
    pub fn new(n: i128, d: i128) -> Option<Rat> {
        if d == 0 {
            return None;
        }
        let (n, d) = if d < 0 {
            (n.checked_neg()?, d.checked_neg()?)
        } else {
            (n, d)
        };
        let g = i128::try_from(gcd(n.unsigned_abs(), d.unsigned_abs())).ok()?;
        Some(Rat { n: n / g, d: d / g })
    }

    /// The numerator of the reduced fraction.
    pub fn numer(self) -> i128 {
        self.n
    }

    /// The denominator of the reduced fraction, always positive.
    pub fn denom(self) -> i128 {
        self.d
    }

    /// Whether the number is a whole number.
    pub fn is_integer(self) -> bool {
        self.d == 1
    }

    fn is_zero(self) -> bool {
        self.n == 0
    }

    fn negated(self) -> Option<Rat> {
        Some(Rat {
            n: self.n.checked_neg()?,
            d: self.d,
        })
    }

    fn plus(self, other: Rat) -> Option<Rat> {
        let g = i128::try_from(gcd(self.d.unsigned_abs(), other.d.unsigned_abs())).ok()?;
        let own = self.d / g;
        let theirs = other.d / g;
        let n = self
            .n
            .checked_mul(theirs)?
            .checked_add(other.n.checked_mul(own)?)?;
        let d = self.d.checked_mul(theirs)?;
        Rat::new(n, d)
    }

    fn minus(self, other: Rat) -> Option<Rat> {
        self.plus(other.negated()?)
    }

    fn times(self, other: Rat) -> Option<Rat> {
        let g1 = i128::try_from(gcd(self.n.unsigned_abs(), other.d.unsigned_abs())).ok()?;
        let g2 = i128::try_from(gcd(other.n.unsigned_abs(), self.d.unsigned_abs())).ok()?;
        let n = (self.n / g1).checked_mul(other.n / g2)?;
        let d = (self.d / g2).checked_mul(other.d / g1)?;
        Some(Rat { n, d })
    }

    fn over(self, other: Rat) -> Option<Rat> {
        if other.is_zero() {
            return None;
        }
        let inverse = if other.n < 0 {
            Rat {
                n: other.d.checked_neg()?,
                d: other.n.checked_neg()?,
            }
        } else {
            Rat {
                n: other.d,
                d: other.n,
            }
        };
        self.times(inverse)
    }

    fn compare(self, other: Rat) -> Option<Ordering> {
        if self.d == other.d {
            return Some(self.n.cmp(&other.n));
        }
        let left = self.n.checked_mul(other.d)?;
        let right = other.n.checked_mul(self.d)?;
        Some(left.cmp(&right))
    }

    fn floor_int(self) -> i128 {
        self.n.div_euclid(self.d)
    }

    fn ceil_int(self) -> i128 {
        let floor = self.floor_int();
        if self.n.rem_euclid(self.d) == 0 {
            floor
        } else {
            floor + 1
        }
    }

    /// The nearest whole number, half away from zero.
    fn round_int(self) -> Option<i128> {
        div_round_half_away(self.n, self.d).ok()
    }

    fn exact_sqrt(self) -> Option<Rat> {
        let n = u128::try_from(self.n).ok()?;
        let d = u128::try_from(self.d).ok()?;
        let rn = isqrt_u128(n);
        let rd = isqrt_u128(d);
        if rn.checked_mul(rn)? != n || rd.checked_mul(rd)? != d {
            return None;
        }
        Rat::new(i128::try_from(rn).ok()?, i128::try_from(rd).ok()?)
    }
}

impl fmt::Display for Rat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.d == 1 {
            write!(f, "{}", self.n)
        } else {
            write!(f, "{}/{}", self.n, self.d)
        }
    }
}

fn pow_rat(base: Rat, exponent: u32) -> Option<Rat> {
    let mut result = Rat::int(1);
    let mut square = base;
    let mut e = exponent;
    while e > 0 {
        if e & 1 == 1 {
            result = result.times(square)?;
        }
        e >>= 1;
        if e > 0 {
            square = square.times(square)?;
        }
    }
    Some(result)
}

/// The kind of a numeric value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tag {
    Number,
    Length,
    Angle,
}

impl Tag {
    fn wrap(self, r: Rat) -> Value {
        match self {
            Tag::Number => Value::Number(r),
            Tag::Length => Value::Length(r),
            Tag::Angle => Value::Angle(r),
        }
    }
}

/// The result of evaluating an expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value {
    /// A plain number.
    Number(Rat),
    /// A length in nanometres (a rational number: a length is rounded to the nanometre only when it
    /// becomes a [`Length`]).
    Length(Rat),
    /// An angle in nano-degrees.
    Angle(Rat),
    /// A truth value.
    Bool(bool),
}

impl Value {
    /// The value of a length.
    pub fn from_length(length: Length) -> Value {
        Value::Length(Rat::int(i128::from(length.get())))
    }

    /// The word for the kind of the value, as used in the messages.
    pub fn kind_name(self) -> &'static str {
        match self {
            Value::Number(_) => "number",
            Value::Length(_) => "length",
            Value::Angle(_) => "angle",
            Value::Bool(_) => "boolean",
        }
    }

    fn numeric(self) -> Option<(Tag, Rat)> {
        match self {
            Value::Number(r) => Some((Tag::Number, r)),
            Value::Length(r) => Some((Tag::Length, r)),
            Value::Angle(r) => Some((Tag::Angle, r)),
            Value::Bool(_) => None,
        }
    }

    /// The value as a [`Length`], rounded half away from zero to the nanometre. A value that is
    /// not a length, or is outside the range of lengths, is refused; `pos` is where the error is
    /// reported.
    pub fn to_length(self, pos: usize) -> Result<Length, CadError> {
        match self {
            Value::Length(r) => {
                let nm = r.round_int().ok_or_else(|| overflow(pos))?;
                Length::new(i64::try_from(nm).map_err(|_| overflow(pos))?)
            }
            other => Err(failure(
                pos,
                format!(
                    "a length was expected but the result is a {}",
                    other.kind_name()
                ),
            )),
        }
    }

    /// Five numbers that identify the value, for the hash of the graph.
    pub fn hash_parts(self) -> [u64; 5] {
        let (tag, r) = match self {
            Value::Number(r) => (1u64, r),
            Value::Length(r) => (2, r),
            Value::Angle(r) => (3, r),
            Value::Bool(b) => (4, Rat::int(i128::from(b))),
        };
        [
            tag,
            r.n as u64,
            (r.n >> 64) as u64,
            r.d as u64,
            (r.d >> 64) as u64,
        ]
    }
}

// ---------------------------------------------------------------------------------------------
// Names
// ---------------------------------------------------------------------------------------------

/// Where the names of an expression take their values from.
pub trait Names {
    /// The value of a name, or `None` when there is none.
    fn lookup(&self, name: &str) -> Option<Value>;
}

impl Names for BTreeMap<String, Value> {
    fn lookup(&self, name: &str) -> Option<Value> {
        self.get(name).copied()
    }
}

/// Named values that come from the settings file (the diameters of the screws, the compensation of
/// the printer, the constants that the person defines).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Constants {
    map: BTreeMap<String, Value>,
}

impl Constants {
    /// No constant.
    pub fn new() -> Constants {
        Constants::default()
    }

    /// Sets a constant. The name follows the rules of the names of variables.
    pub fn set(&mut self, name: &str, value: Value) -> Result<(), CadError> {
        crate::vars::check_name(name)?;
        self.map.insert(name.to_string(), value);
        Ok(())
    }

    /// The value of a constant.
    pub fn get(&self, name: &str) -> Option<Value> {
        self.map.get(name).copied()
    }

    /// The names in order.
    pub fn names(&self) -> Vec<&str> {
        self.map.keys().map(String::as_str).collect()
    }

    /// How many constants.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Whether there is no constant.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

impl Names for Constants {
    fn lookup(&self, name: &str) -> Option<Value> {
        self.get(name)
    }
}

/// Whether a word has a meaning of its own in the language, so that it cannot be the name of a
/// variable or a constant: the words of the logic, `true` and `false`, and the units.
pub fn is_reserved(name: &str) -> bool {
    matches!(
        name,
        "and" | "or" | "not" | "true" | "false" | "deg" | "turn"
    ) || Unit::from_suffix(name).is_some()
}

// ---------------------------------------------------------------------------------------------
// The tree
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Or,
    And,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    Add,
    Sub,
    Mul,
    Div,
    Pow,
}

impl Op {
    fn symbol(self) -> &'static str {
        match self {
            Op::Or => "or",
            Op::And => "and",
            Op::Lt => "<",
            Op::Le => "<=",
            Op::Gt => ">",
            Op::Ge => ">=",
            Op::Eq => "==",
            Op::Ne => "!=",
            Op::Add => "+",
            Op::Sub => "-",
            Op::Mul => "*",
            Op::Div => "/",
            Op::Pow => "^",
        }
    }

    fn prec(self) -> u8 {
        match self {
            Op::Or => 10,
            Op::And => 20,
            Op::Lt | Op::Le | Op::Gt | Op::Ge | Op::Eq | Op::Ne => 40,
            Op::Add | Op::Sub => 50,
            Op::Mul | Op::Div => 60,
            Op::Pow => 80,
        }
    }

    /// The least precedence that the left and the right operand may have without parentheses.
    fn child_minimums(self) -> (u8, u8) {
        match self {
            Op::Pow => (90, 70),
            Op::Lt | Op::Le | Op::Gt | Op::Ge | Op::Eq | Op::Ne => (50, 50),
            other => (other.prec(), other.prec() + 1),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Func {
    Min,
    Max,
    Abs,
    Floor,
    Ceil,
    Round,
    Clamp,
    Sqrt,
    Hypot,
    Sin,
    Cos,
    If,
}

impl Func {
    const ALL: [Func; 12] = [
        Func::Min,
        Func::Max,
        Func::Abs,
        Func::Floor,
        Func::Ceil,
        Func::Round,
        Func::Clamp,
        Func::Sqrt,
        Func::Hypot,
        Func::Sin,
        Func::Cos,
        Func::If,
    ];

    fn name(self) -> &'static str {
        match self {
            Func::Min => "min",
            Func::Max => "max",
            Func::Abs => "abs",
            Func::Floor => "floor",
            Func::Ceil => "ceil",
            Func::Round => "round",
            Func::Clamp => "clamp",
            Func::Sqrt => "sqrt",
            Func::Hypot => "hypot",
            Func::Sin => "sin",
            Func::Cos => "cos",
            Func::If => "if",
        }
    }

    fn from_name(name: &str) -> Option<Func> {
        Func::ALL.into_iter().find(|f| f.name() == name)
    }

    /// The fewest and the most arguments.
    fn arity(self) -> (usize, usize) {
        match self {
            Func::Min | Func::Max => (1, usize::MAX),
            Func::Abs | Func::Sqrt | Func::Sin | Func::Cos => (1, 1),
            Func::Floor | Func::Ceil | Func::Round => (1, 2),
            Func::Clamp | Func::If => (3, 3),
            Func::Hypot => (2, 2),
        }
    }

    fn arity_text(self) -> String {
        let (lo, hi) = self.arity();
        if lo == hi {
            format!("exactly {lo}")
        } else if hi == usize::MAX {
            format!("at least {lo}")
        } else {
            format!("{lo} or {hi}")
        }
    }
}

/// The names of the functions of the language.
pub fn function_names() -> Vec<&'static str> {
    Func::ALL.iter().map(|f| f.name()).collect()
}

/// A unit written after a number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnitWord {
    Len(Unit),
    Deg,
    Turn,
}

impl UnitWord {
    fn from_word(word: &str) -> Option<UnitWord> {
        match word {
            "deg" => Some(UnitWord::Deg),
            "turn" => Some(UnitWord::Turn),
            other => Unit::from_suffix(other).map(UnitWord::Len),
        }
    }

    fn word(self) -> &'static str {
        match self {
            UnitWord::Len(u) => u.suffix(),
            UnitWord::Deg => "deg",
            UnitWord::Turn => "turn",
        }
    }

    /// The kind of value the unit makes and how many nanometres or nano-degrees it is.
    fn factor(self) -> (Tag, i128) {
        match self {
            UnitWord::Len(u) => (Tag::Length, u.nanometres()),
            UnitWord::Deg => (Tag::Angle, 1_000_000_000),
            UnitWord::Turn => (Tag::Angle, 360_000_000_000),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    Literal { mantissa: i128, scale: u32 },
    Bool(bool),
    Name(String),
    Neg(Box<Expr>),
    Not(Box<Expr>),
    Binary(Op, Box<Expr>, Box<Expr>),
    Call(Func, Vec<Expr>),
    Unit(Box<Expr>, UnitWord),
}

/// A parsed expression. Two expressions are equal when they have the same structure; where each
/// part was written in the text does not count.
#[derive(Debug, Clone)]
pub struct Expr {
    kind: Kind,
    pos: usize,
    depth: u32,
}

impl PartialEq for Expr {
    fn eq(&self, other: &Expr) -> bool {
        self.kind == other.kind
    }
}

impl Eq for Expr {}

impl Expr {
    /// The precedence of the outermost operation, 100 for a part that needs no parentheses.
    fn prec(&self) -> u8 {
        match &self.kind {
            Kind::Binary(op, _, _) => op.prec(),
            Kind::Not(_) => 30,
            Kind::Neg(_) => 70,
            Kind::Unit(_, _) => 90,
            Kind::Literal { .. } | Kind::Bool(_) | Kind::Name(_) | Kind::Call(_, _) => 100,
        }
    }

    fn write_child(&self, out: &mut String, min: u8) {
        if self.prec() < min {
            out.push('(');
            self.write(out);
            out.push(')');
        } else {
            self.write(out);
        }
    }

    fn write(&self, out: &mut String) {
        match &self.kind {
            Kind::Literal { mantissa, scale } => {
                if *scale == 0 {
                    out.push_str(&mantissa.to_string());
                } else {
                    let scale = *scale as usize;
                    let width = scale + 1;
                    let digits = format!("{mantissa:0>width$}");
                    let split = digits.len() - scale;
                    out.push_str(&digits[..split]);
                    out.push('.');
                    out.push_str(&digits[split..]);
                }
            }
            Kind::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Kind::Name(name) => out.push_str(name),
            Kind::Neg(inner) => {
                out.push('-');
                inner.write_child(out, 70);
            }
            Kind::Not(inner) => {
                out.push_str("not ");
                inner.write_child(out, 30);
            }
            Kind::Binary(op, left, right) => {
                let (left_min, right_min) = op.child_minimums();
                left.write_child(out, left_min);
                out.push(' ');
                out.push_str(op.symbol());
                out.push(' ');
                right.write_child(out, right_min);
            }
            Kind::Call(func, args) => {
                out.push_str(func.name());
                out.push('(');
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    arg.write(out);
                }
                out.push(')');
            }
            Kind::Unit(inner, unit) => {
                inner.write_child(out, 100);
                out.push(' ');
                out.push_str(unit.word());
            }
        }
    }

    /// The expression written in its canonical form: the parentheses that are needed and no
    /// others, one space around each operator. Reading this text gives back an equal expression.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }

    /// The names the expression refers to, in the order they appear (a name used twice is listed
    /// twice), each with the column where it is written (counting from 0).
    pub fn names(&self) -> Vec<(String, usize)> {
        let mut out = Vec::new();
        self.collect_names(&mut out);
        out
    }

    fn collect_names(&self, out: &mut Vec<(String, usize)>) {
        match &self.kind {
            Kind::Name(name) => out.push((name.clone(), self.pos)),
            Kind::Neg(inner) | Kind::Not(inner) | Kind::Unit(inner, _) => inner.collect_names(out),
            Kind::Binary(_, left, right) => {
                left.collect_names(out);
                right.collect_names(out);
            }
            Kind::Call(_, args) => {
                for arg in args {
                    arg.collect_names(out);
                }
            }
            Kind::Literal { .. } | Kind::Bool(_) => {}
        }
    }

    /// Evaluates the expression with its own budget of steps.
    pub fn eval(&self, names: &dyn Names, limits: &ExprLimits) -> Result<Value, CadError> {
        let mut budget = Budget::new(limits);
        self.eval_with(names, limits, &mut budget)
    }

    /// Evaluates the expression spending the steps of a shared budget.
    pub fn eval_with(
        &self,
        names: &dyn Names,
        limits: &ExprLimits,
        budget: &mut Budget,
    ) -> Result<Value, CadError> {
        let mut cx = Cx {
            names,
            limits,
            budget,
        };
        eval_node(self, &mut cx)
    }

    /// Evaluates the expression, which must give a length.
    pub fn eval_length(&self, names: &dyn Names, limits: &ExprLimits) -> Result<Length, CadError> {
        self.eval(names, limits)?.to_length(0)
    }
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_text())
    }
}

// ---------------------------------------------------------------------------------------------
// The lexer
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    Number(i128, u32),
    Name(String),
    Plus,
    Minus,
    Star,
    Slash,
    Caret,
    LParen,
    RParen,
    Comma,
    Lt,
    Le,
    Gt,
    Ge,
    EqEq,
    Ne,
    End,
}

#[derive(Debug, Clone)]
struct Token {
    tok: Tok,
    pos: usize,
}

fn describe(tok: &Tok) -> String {
    match tok {
        Tok::Number(_, _) => "a number".to_string(),
        Tok::Name(name) => format!("the word {name}"),
        Tok::Plus => "`+`".to_string(),
        Tok::Minus => "`-`".to_string(),
        Tok::Star => "`*`".to_string(),
        Tok::Slash => "`/`".to_string(),
        Tok::Caret => "`^`".to_string(),
        Tok::LParen => "`(`".to_string(),
        Tok::RParen => "`)`".to_string(),
        Tok::Comma => "`,`".to_string(),
        Tok::Lt => "`<`".to_string(),
        Tok::Le => "`<=`".to_string(),
        Tok::Gt => "`>`".to_string(),
        Tok::Ge => "`>=`".to_string(),
        Tok::EqEq => "`==`".to_string(),
        Tok::Ne => "`!=`".to_string(),
        Tok::End => "the end of the expression".to_string(),
    }
}

fn lex(text: &str) -> Result<Vec<Token>, CadError> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        let pos = i;
        match c {
            b' ' | b'\t' => i += 1,
            b'0'..=b'9' => {
                let mut mantissa: i128 = 0;
                let mut digits = 0u32;
                let mut scale = 0u32;
                let mut dot = false;
                loop {
                    match bytes.get(i).copied() {
                        Some(b @ b'0'..=b'9') => {
                            digits += 1;
                            if digits > MAX_DIGITS {
                                return Err(syntax(pos, "a number has more than 18 digits"));
                            }
                            mantissa = mantissa * 10 + i128::from(b - b'0');
                            if dot {
                                scale += 1;
                            }
                            i += 1;
                        }
                        Some(b'.') if !dot => {
                            if !bytes.get(i + 1).is_some_and(u8::is_ascii_digit) {
                                return Err(syntax(i, "a digit must follow the point"));
                            }
                            dot = true;
                            i += 1;
                        }
                        _ => break,
                    }
                }
                out.push(Token {
                    tok: Tok::Number(mantissa, scale),
                    pos,
                });
            }
            b'a'..=b'z' | b'A'..=b'Z' => {
                while bytes
                    .get(i)
                    .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
                {
                    i += 1;
                }
                out.push(Token {
                    tok: Tok::Name(text[pos..i].to_string()),
                    pos,
                });
            }
            b'+' | b'-' | b'*' | b'/' | b'^' | b'(' | b')' | b',' => {
                let tok = match c {
                    b'+' => Tok::Plus,
                    b'-' => Tok::Minus,
                    b'*' => Tok::Star,
                    b'/' => Tok::Slash,
                    b'^' => Tok::Caret,
                    b'(' => Tok::LParen,
                    b')' => Tok::RParen,
                    _ => Tok::Comma,
                };
                out.push(Token { tok, pos });
                i += 1;
            }
            b'<' | b'>' => {
                let equals = bytes.get(i + 1) == Some(&b'=');
                let tok = match (c, equals) {
                    (b'<', true) => Tok::Le,
                    (b'<', false) => Tok::Lt,
                    (_, true) => Tok::Ge,
                    (_, false) => Tok::Gt,
                };
                out.push(Token { tok, pos });
                i += if equals { 2 } else { 1 };
            }
            b'=' => {
                if bytes.get(i + 1) != Some(&b'=') {
                    return Err(syntax(pos, "use == to compare two values"));
                }
                out.push(Token {
                    tok: Tok::EqEq,
                    pos,
                });
                i += 2;
            }
            b'!' => {
                if bytes.get(i + 1) != Some(&b'=') {
                    return Err(syntax(pos, "use not to negate a condition"));
                }
                out.push(Token { tok: Tok::Ne, pos });
                i += 2;
            }
            _ => {
                return Err(if c >= 0x80 {
                    syntax(pos, "only plain ASCII characters are allowed")
                } else {
                    syntax(pos, format!("unexpected character {:?}", char::from(c)))
                });
            }
        }
    }
    out.push(Token {
        tok: Tok::End,
        pos: bytes.len(),
    });
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// The parser
// ---------------------------------------------------------------------------------------------

struct Parser<'a> {
    toks: Vec<Token>,
    i: usize,
    nest: u32,
    limits: &'a ExprLimits,
}

impl Parser<'_> {
    fn peek(&self) -> Tok {
        self.toks[self.i].tok.clone()
    }

    fn pos(&self) -> usize {
        self.toks[self.i].pos
    }

    fn advance(&mut self) {
        if self.i + 1 < self.toks.len() {
            self.i += 1;
        }
    }

    fn is_word(&self, word: &str) -> bool {
        matches!(&self.toks[self.i].tok, Tok::Name(n) if n == word)
    }

    fn enter(&mut self) -> Result<(), CadError> {
        self.nest += 1;
        if self.nest > self.limits.max_depth {
            Err(limit(self.pos(), "depth"))
        } else {
            Ok(())
        }
    }

    fn leave(&mut self) {
        self.nest -= 1;
    }

    fn make(&self, pos: usize, kind: Kind, below: u32) -> Result<Expr, CadError> {
        let depth = below + 1;
        if depth > self.limits.max_depth {
            return Err(limit(pos, "depth"));
        }
        Ok(Expr { kind, pos, depth })
    }

    fn binary(&self, pos: usize, op: Op, left: Expr, right: Expr) -> Result<Expr, CadError> {
        let below = left.depth.max(right.depth);
        self.make(
            pos,
            Kind::Binary(op, Box::new(left), Box::new(right)),
            below,
        )
    }

    fn expect_rparen(&mut self) -> Result<(), CadError> {
        if self.peek() == Tok::RParen {
            self.advance();
            Ok(())
        } else {
            Err(syntax(
                self.pos(),
                format!("expected `)` but found {}", describe(&self.peek())),
            ))
        }
    }

    /// A nested part of the expression (inside parentheses, after a sign or a `not`, an argument
    /// or an exponent): it counts one level of depth.
    fn nested(&mut self, parse: fn(&mut Self) -> Result<Expr, CadError>) -> Result<Expr, CadError> {
        self.enter()?;
        let result = parse(self);
        self.leave();
        result
    }

    fn parse_or(&mut self) -> Result<Expr, CadError> {
        let mut left = self.parse_and()?;
        while self.is_word("or") {
            let pos = self.pos();
            self.advance();
            let right = self.parse_and()?;
            left = self.binary(pos, Op::Or, left, right)?;
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, CadError> {
        let mut left = self.parse_not()?;
        while self.is_word("and") {
            let pos = self.pos();
            self.advance();
            let right = self.parse_not()?;
            left = self.binary(pos, Op::And, left, right)?;
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<Expr, CadError> {
        if self.is_word("not") {
            let pos = self.pos();
            self.advance();
            let inner = self.nested(Self::parse_not)?;
            let below = inner.depth;
            return self.make(pos, Kind::Not(Box::new(inner)), below);
        }
        self.parse_cmp()
    }

    fn parse_cmp(&mut self) -> Result<Expr, CadError> {
        let left = self.parse_sum()?;
        let op = match self.peek() {
            Tok::Lt => Op::Lt,
            Tok::Le => Op::Le,
            Tok::Gt => Op::Gt,
            Tok::Ge => Op::Ge,
            Tok::EqEq => Op::Eq,
            Tok::Ne => Op::Ne,
            _ => return Ok(left),
        };
        let pos = self.pos();
        self.advance();
        let right = self.parse_sum()?;
        let node = self.binary(pos, op, left, right)?;
        if matches!(
            self.peek(),
            Tok::Lt | Tok::Le | Tok::Gt | Tok::Ge | Tok::EqEq | Tok::Ne
        ) {
            return Err(syntax(
                self.pos(),
                "a comparison cannot be chained; join two comparisons with and or or",
            ));
        }
        Ok(node)
    }

    fn parse_sum(&mut self) -> Result<Expr, CadError> {
        let mut left = self.parse_term()?;
        loop {
            let op = match self.peek() {
                Tok::Plus => Op::Add,
                Tok::Minus => Op::Sub,
                _ => return Ok(left),
            };
            let pos = self.pos();
            self.advance();
            let right = self.parse_term()?;
            left = self.binary(pos, op, left, right)?;
        }
    }

    fn parse_term(&mut self) -> Result<Expr, CadError> {
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                Tok::Star => Op::Mul,
                Tok::Slash => Op::Div,
                _ => return Ok(left),
            };
            let pos = self.pos();
            self.advance();
            let right = self.parse_unary()?;
            left = self.binary(pos, op, left, right)?;
        }
    }

    fn parse_unary(&mut self) -> Result<Expr, CadError> {
        match self.peek() {
            Tok::Minus => {
                let pos = self.pos();
                self.advance();
                let inner = self.nested(Self::parse_unary)?;
                let below = inner.depth;
                self.make(pos, Kind::Neg(Box::new(inner)), below)
            }
            Tok::Plus => {
                self.advance();
                self.nested(Self::parse_unary)
            }
            _ => self.parse_power(),
        }
    }

    fn parse_power(&mut self) -> Result<Expr, CadError> {
        let base = self.parse_postfix()?;
        if self.peek() == Tok::Caret {
            let pos = self.pos();
            self.advance();
            let exponent = self.nested(Self::parse_unary)?;
            return self.binary(pos, Op::Pow, base, exponent);
        }
        Ok(base)
    }

    fn parse_postfix(&mut self) -> Result<Expr, CadError> {
        let inner = self.parse_primary()?;
        if let Tok::Name(word) = self.peek() {
            if let Some(unit) = UnitWord::from_word(&word) {
                let pos = self.pos();
                self.advance();
                let below = inner.depth;
                return self.make(pos, Kind::Unit(Box::new(inner), unit), below);
            }
        }
        Ok(inner)
    }

    fn parse_primary(&mut self) -> Result<Expr, CadError> {
        let pos = self.pos();
        match self.peek() {
            Tok::Number(mantissa, scale) => {
                self.advance();
                self.make(pos, Kind::Literal { mantissa, scale }, 0)
            }
            Tok::LParen => {
                self.advance();
                let inner = self.nested(Self::parse_or)?;
                self.expect_rparen()?;
                Ok(inner)
            }
            Tok::Name(name) => {
                self.advance();
                if self.peek() == Tok::LParen {
                    return self.parse_call(pos, &name);
                }
                if name == "true" || name == "false" {
                    return self.make(pos, Kind::Bool(name == "true"), 0);
                }
                if matches!(name.as_str(), "and" | "or" | "not") {
                    return Err(syntax(
                        pos,
                        format!("{name} is not allowed here: it needs a value before or after it"),
                    ));
                }
                if UnitWord::from_word(&name).is_some() {
                    return Err(syntax(
                        pos,
                        format!("the unit {name} needs a number before it"),
                    ));
                }
                self.make(pos, Kind::Name(name), 0)
            }
            Tok::End => Err(syntax(
                pos,
                "the expression ends where a value was expected",
            )),
            other => Err(syntax(
                pos,
                format!("{} found where a value was expected", describe(&other)),
            )),
        }
    }

    fn parse_call(&mut self, pos: usize, name: &str) -> Result<Expr, CadError> {
        let func = Func::from_name(name)
            .ok_or_else(|| syntax(pos, format!("{name} is not a function of the language")))?;
        self.advance();
        let mut args = Vec::new();
        if self.peek() != Tok::RParen {
            loop {
                args.push(self.nested(Self::parse_or)?);
                if self.peek() == Tok::Comma {
                    self.advance();
                } else {
                    break;
                }
            }
        }
        self.expect_rparen()?;
        let (lo, hi) = func.arity();
        if args.len() < lo || args.len() > hi {
            return Err(syntax(
                pos,
                format!("{name} takes {} arguments", func.arity_text()),
            ));
        }
        let below = args.iter().map(|a| a.depth).max().unwrap_or(0);
        self.make(pos, Kind::Call(func, args), below)
    }
}

/// Reads an expression. A text that is too long, too deep or not written in the language is
/// refused with the column of the first problem.
pub fn parse(text: &str, limits: &ExprLimits) -> Result<Expr, CadError> {
    let max_length = usize::try_from(limits.max_length).unwrap_or(usize::MAX);
    if text.len() > max_length {
        return Err(limit(max_length, "length"));
    }
    let toks = lex(text)?;
    let mut parser = Parser {
        toks,
        i: 0,
        nest: 0,
        limits,
    };
    let expr = parser.parse_or()?;
    if parser.peek() != Tok::End {
        return Err(syntax(
            parser.pos(),
            format!(
                "{} found after the end of the expression",
                describe(&parser.peek())
            ),
        ));
    }
    Ok(expr)
}

// ---------------------------------------------------------------------------------------------
// The evaluation
// ---------------------------------------------------------------------------------------------

struct Cx<'a> {
    names: &'a dyn Names,
    limits: &'a ExprLimits,
    budget: &'a mut Budget,
}

fn want_bool(value: Value, pos: usize, what: &str) -> Result<bool, CadError> {
    match value {
        Value::Bool(b) => Ok(b),
        other => Err(failure(
            pos,
            format!("{what} needs a boolean but found a {}", other.kind_name()),
        )),
    }
}

fn eval_node(e: &Expr, cx: &mut Cx<'_>) -> Result<Value, CadError> {
    cx.budget.spend(e.pos)?;
    match &e.kind {
        Kind::Literal { mantissa, scale } => {
            let den = 10i128.checked_pow(*scale).ok_or_else(|| overflow(e.pos))?;
            Rat::new(*mantissa, den)
                .map(Value::Number)
                .ok_or_else(|| overflow(e.pos))
        }
        Kind::Bool(b) => Ok(Value::Bool(*b)),
        Kind::Name(name) => cx
            .names
            .lookup(name)
            .ok_or_else(|| CadError::UnknownName(format!("{name} (column {})", e.pos + 1))),
        Kind::Neg(inner) => {
            let value = eval_node(inner, cx)?;
            match value.numeric() {
                Some((tag, r)) => Ok(tag.wrap(r.negated().ok_or_else(|| overflow(e.pos))?)),
                None => Err(failure(e.pos, "a boolean cannot be negated")),
            }
        }
        Kind::Not(inner) => {
            let value = eval_node(inner, cx)?;
            Ok(Value::Bool(!want_bool(value, e.pos, "not")?))
        }
        Kind::Binary(Op::And, left, right) => {
            let a = want_bool(eval_node(left, cx)?, e.pos, "and")?;
            if !a {
                return Ok(Value::Bool(false));
            }
            let b = want_bool(eval_node(right, cx)?, e.pos, "and")?;
            Ok(Value::Bool(b))
        }
        Kind::Binary(Op::Or, left, right) => {
            let a = want_bool(eval_node(left, cx)?, e.pos, "or")?;
            if a {
                return Ok(Value::Bool(true));
            }
            let b = want_bool(eval_node(right, cx)?, e.pos, "or")?;
            Ok(Value::Bool(b))
        }
        Kind::Binary(op, left, right) => {
            let a = eval_node(left, cx)?;
            let b = eval_node(right, cx)?;
            binary(*op, a, b, e.pos, cx.limits)
        }
        Kind::Call(Func::If, args) => {
            let condition = want_bool(eval_node(&args[0], cx)?, e.pos, "if")?;
            eval_node(&args[if condition { 1 } else { 2 }], cx)
        }
        Kind::Call(func, args) => {
            let mut values = Vec::with_capacity(args.len());
            for arg in args {
                values.push(eval_node(arg, cx)?);
            }
            call(*func, &values, e.pos)
        }
        Kind::Unit(inner, unit) => match eval_node(inner, cx)? {
            Value::Number(r) => {
                let (tag, factor) = unit.factor();
                let scaled = r.times(Rat::int(factor)).ok_or_else(|| overflow(e.pos))?;
                Ok(tag.wrap(scaled))
            }
            other => Err(failure(
                e.pos,
                format!(
                    "the unit {} cannot follow a {}",
                    unit.word(),
                    other.kind_name()
                ),
            )),
        },
    }
}

fn binary(op: Op, a: Value, b: Value, pos: usize, limits: &ExprLimits) -> Result<Value, CadError> {
    match op {
        Op::Lt | Op::Le | Op::Gt | Op::Ge | Op::Eq | Op::Ne => compare(op, a, b, pos),
        Op::Add | Op::Sub => add_sub(op, a, b, pos),
        Op::Mul => multiply(a, b, pos),
        Op::Div => divide(a, b, pos),
        Op::Pow => power(a, b, pos, limits),
        Op::And | Op::Or => Err(failure(pos, "the logic is evaluated by the caller")),
    }
}

fn compare(op: Op, a: Value, b: Value, pos: usize) -> Result<Value, CadError> {
    if let (Value::Bool(x), Value::Bool(y)) = (a, b) {
        return match op {
            Op::Eq => Ok(Value::Bool(x == y)),
            Op::Ne => Ok(Value::Bool(x != y)),
            _ => Err(failure(pos, "booleans can only be compared with == and !=")),
        };
    }
    match (a.numeric(), b.numeric()) {
        (Some((ta, x)), Some((tb, y))) if ta == tb => {
            let ord = x.compare(y).ok_or_else(|| overflow(pos))?;
            let result = match op {
                Op::Lt => ord.is_lt(),
                Op::Le => ord.is_le(),
                Op::Gt => ord.is_gt(),
                Op::Ge => ord.is_ge(),
                Op::Eq => ord.is_eq(),
                _ => ord.is_ne(),
            };
            Ok(Value::Bool(result))
        }
        _ => Err(failure(
            pos,
            format!(
                "cannot compare a {} with a {}",
                a.kind_name(),
                b.kind_name()
            ),
        )),
    }
}

fn add_sub(op: Op, a: Value, b: Value, pos: usize) -> Result<Value, CadError> {
    match (a.numeric(), b.numeric()) {
        (Some((ta, x)), Some((tb, y))) if ta == tb => {
            let result = if op == Op::Add { x.plus(y) } else { x.minus(y) };
            Ok(ta.wrap(result.ok_or_else(|| overflow(pos))?))
        }
        _ => {
            let verb = if op == Op::Add { "add" } else { "subtract" };
            let hint = if matches!(a, Value::Number(_)) || matches!(b, Value::Number(_)) {
                "; a plain number has no unit, write one after it"
            } else {
                ""
            };
            Err(failure(
                pos,
                format!(
                    "cannot {verb} a {} and a {}{hint}",
                    a.kind_name(),
                    b.kind_name()
                ),
            ))
        }
    }
}

fn multiply(a: Value, b: Value, pos: usize) -> Result<Value, CadError> {
    match (a.numeric(), b.numeric()) {
        (Some((Tag::Number, x)), Some((tb, y))) => {
            Ok(tb.wrap(x.times(y).ok_or_else(|| overflow(pos))?))
        }
        (Some((ta, x)), Some((Tag::Number, y))) => {
            Ok(ta.wrap(x.times(y).ok_or_else(|| overflow(pos))?))
        }
        _ => Err(failure(
            pos,
            format!(
                "cannot multiply a {} by a {}; only a number scales a length or an angle",
                a.kind_name(),
                b.kind_name()
            ),
        )),
    }
}

fn divide(a: Value, b: Value, pos: usize) -> Result<Value, CadError> {
    let (Some((ta, x)), Some((tb, y))) = (a.numeric(), b.numeric()) else {
        return Err(failure(
            pos,
            format!("cannot divide a {} by a {}", a.kind_name(), b.kind_name()),
        ));
    };
    if y.is_zero() {
        return Err(failure(pos, "division by zero"));
    }
    let quotient = x.over(y).ok_or_else(|| overflow(pos))?;
    if tb == Tag::Number {
        Ok(ta.wrap(quotient))
    } else if ta == tb {
        Ok(Value::Number(quotient))
    } else if ta == Tag::Number {
        Err(failure(
            pos,
            "a number cannot be divided by a length or an angle; for a fraction of a unit write (1 / 16) in",
        ))
    } else {
        Err(failure(
            pos,
            format!("cannot divide a {} by a {}", a.kind_name(), b.kind_name()),
        ))
    }
}

fn power(a: Value, b: Value, pos: usize, limits: &ExprLimits) -> Result<Value, CadError> {
    let (Value::Number(base), Value::Number(exponent)) = (a, b) else {
        return Err(failure(
            pos,
            "a power needs plain numbers; a length or an angle cannot be raised to a power",
        ));
    };
    if !exponent.is_integer() {
        return Err(failure(pos, "the exponent must be a whole number"));
    }
    let magnitude = exponent.numer().unsigned_abs();
    if magnitude > u128::from(limits.max_exponent) {
        return Err(limit(pos, "exponent"));
    }
    let k = u32::try_from(magnitude).map_err(|_| limit(pos, "exponent"))?;
    let p = pow_rat(base, k).ok_or_else(|| overflow(pos))?;
    let result = if exponent.numer() < 0 {
        if p.is_zero() {
            return Err(failure(pos, "division by zero"));
        }
        Rat::int(1).over(p)
    } else {
        Some(p)
    };
    Ok(Value::Number(result.ok_or_else(|| overflow(pos))?))
}

fn numeric_arg(value: Value, pos: usize, name: &str) -> Result<(Tag, Rat), CadError> {
    value.numeric().ok_or_else(|| {
        failure(
            pos,
            format!("{name} needs numbers, lengths or angles but found a boolean"),
        )
    })
}

fn call(func: Func, args: &[Value], pos: usize) -> Result<Value, CadError> {
    match func {
        Func::Min | Func::Max => min_max(func, args, pos),
        Func::Abs => {
            let (tag, r) = numeric_arg(args[0], pos, "abs")?;
            let magnitude = if r.n < 0 {
                r.negated().ok_or_else(|| overflow(pos))?
            } else {
                r
            };
            Ok(tag.wrap(magnitude))
        }
        Func::Floor | Func::Ceil | Func::Round => rounding(func, args, pos),
        Func::Clamp => clamp(args, pos),
        Func::Sqrt => match args[0] {
            Value::Number(r) => {
                if r.n < 0 {
                    Err(failure(pos, "the square root of a negative number"))
                } else {
                    r.exact_sqrt()
                        .map(Value::Number)
                        .ok_or_else(|| failure(pos, format!("the square root of {r} is not exact")))
                }
            }
            other => Err(failure(
                pos,
                format!(
                    "sqrt needs a plain number but found a {}",
                    other.kind_name()
                ),
            )),
        },
        Func::Hypot => hypot(args, pos),
        Func::Sin | Func::Cos => trig(func, args, pos),
        Func::If => Err(failure(pos, "if is evaluated by the caller")),
    }
}

fn min_max(func: Func, args: &[Value], pos: usize) -> Result<Value, CadError> {
    let name = func.name();
    let mut items = Vec::with_capacity(args.len());
    for arg in args {
        items.push(numeric_arg(*arg, pos, name)?);
    }
    let (tag, mut best) = items[0];
    for (t, r) in &items {
        if *t != tag {
            return Err(failure(pos, format!("{name} needs values of one kind")));
        }
        let ord = r.compare(best).ok_or_else(|| overflow(pos))?;
        if (func == Func::Min && ord.is_lt()) || (func == Func::Max && ord.is_gt()) {
            best = *r;
        }
    }
    Ok(tag.wrap(best))
}

fn rounding(func: Func, args: &[Value], pos: usize) -> Result<Value, CadError> {
    let name = func.name();
    let (tag, x) = numeric_arg(args[0], pos, name)?;
    let step = if args.len() == 2 {
        let (step_tag, step) = numeric_arg(args[1], pos, name)?;
        if step_tag != tag {
            return Err(failure(
                pos,
                format!("the step of {name} must be of the same kind as the value"),
            ));
        }
        if step.n <= 0 {
            return Err(failure(pos, format!("the step of {name} must be positive")));
        }
        step
    } else {
        Rat::int(1)
    };
    let quotient = x.over(step).ok_or_else(|| overflow(pos))?;
    let whole: Option<i128> = match func {
        Func::Floor => Some(quotient.floor_int()),
        Func::Ceil => Some(quotient.ceil_int()),
        _ => quotient.round_int(),
    };
    let whole = whole.ok_or_else(|| overflow(pos))?;
    let result = Rat::int(whole).times(step).ok_or_else(|| overflow(pos))?;
    Ok(tag.wrap(result))
}

fn clamp(args: &[Value], pos: usize) -> Result<Value, CadError> {
    let (tag, x) = numeric_arg(args[0], pos, "clamp")?;
    let (low_tag, low) = numeric_arg(args[1], pos, "clamp")?;
    let (high_tag, high) = numeric_arg(args[2], pos, "clamp")?;
    if low_tag != tag || high_tag != tag {
        return Err(failure(pos, "clamp needs values of one kind"));
    }
    if low.compare(high).ok_or_else(|| overflow(pos))?.is_gt() {
        return Err(failure(
            pos,
            "the low bound of clamp is above the high bound",
        ));
    }
    let mut value = x;
    if value.compare(low).ok_or_else(|| overflow(pos))?.is_lt() {
        value = low;
    }
    if value.compare(high).ok_or_else(|| overflow(pos))?.is_gt() {
        value = high;
    }
    Ok(tag.wrap(value))
}

fn hypot(args: &[Value], pos: usize) -> Result<Value, CadError> {
    let (Value::Length(a), Value::Length(b)) = (args[0], args[1]) else {
        return Err(failure(pos, "hypot needs two lengths"));
    };
    let a = a.round_int().ok_or_else(|| overflow(pos))?;
    let b = b.round_int().ok_or_else(|| overflow(pos))?;
    let sum = a
        .checked_mul(a)
        .and_then(|aa| b.checked_mul(b).and_then(|bb| aa.checked_add(bb)))
        .ok_or_else(|| overflow(pos))?;
    let n = sum.unsigned_abs();
    let root = isqrt_u128(n);
    // (root + 1/2)^2 = root^2 + root + 1/4, so n above root^2 + root rounds up
    let root = if n - root * root > root {
        root + 1
    } else {
        root
    };
    let nm = i128::try_from(root).map_err(|_| overflow(pos))?;
    Ok(Value::Length(Rat::int(nm)))
}

fn trig(func: Func, args: &[Value], pos: usize) -> Result<Value, CadError> {
    let Value::Angle(r) = args[0] else {
        return Err(failure(
            pos,
            format!(
                "{} needs an angle but found a {}",
                func.name(),
                args[0].kind_name()
            ),
        ));
    };
    let ndeg = r.round_int().ok_or_else(|| overflow(pos))?;
    let out_of_range = || failure(pos, "the angle is outside the range of angles");
    let angle =
        Angle::new(i64::try_from(ndeg).map_err(|_| out_of_range())?).map_err(|_| out_of_range())?;
    let parts_per_billion = if func == Func::Sin {
        angle.sin()
    } else {
        angle.cos()
    };
    Rat::new(i128::from(parts_per_billion), 1_000_000_000)
        .map(Value::Number)
        .ok_or_else(|| overflow(pos))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;

    fn limits() -> ExprLimits {
        Settings::defaults().expr
    }

    fn ev(text: &str) -> Result<Value, CadError> {
        parse(text, &limits())?.eval(&Constants::new(), &limits())
    }

    fn ev_in(text: &str, env: &BTreeMap<String, Value>) -> Result<Value, CadError> {
        parse(text, &limits())?.eval(env, &limits())
    }

    fn num(n: i128, d: i128) -> Value {
        Value::Number(Rat::new(n, d).unwrap())
    }

    fn len(nm: i128) -> Value {
        Value::Length(Rat::int(nm))
    }

    fn code_of(text: &str) -> &'static str {
        ev(text).unwrap_err().code()
    }

    #[test]
    fn the_rationals_are_reduced_and_every_operation_checks_overflow() {
        let r = |n, d| Rat::new(n, d).unwrap();
        assert_eq!(r(2, 4), r(1, 2));
        assert_eq!(r(1, -2), r(-1, 2));
        assert_eq!((r(-6, 3).numer(), r(-6, 3).denom()), (-2, 1));
        assert_eq!(Rat::new(1, 0), None);
        assert_eq!(Rat::new(i128::MIN, -1), None);
        assert_eq!(r(1, 2).plus(r(1, 3)), Some(r(5, 6)));
        assert_eq!(r(1, 2).minus(r(1, 2)), Some(Rat::ZERO));
        assert_eq!(r(2, 3).times(r(3, 4)), Some(r(1, 2)));
        assert_eq!(r(2, 3).over(r(-4, 9)), Some(r(-3, 2)));
        assert_eq!(r(1, 2).over(Rat::ZERO), None);
        assert_eq!(Rat::int(i128::MAX).plus(Rat::int(1)), None);
        assert_eq!(Rat::int(i128::MAX).times(Rat::int(2)), None);
        assert_eq!(r(1, 3).compare(r(1, 2)), Some(Ordering::Less));
        assert_eq!(r(-1, 3).floor_int(), -1);
        assert_eq!(r(-1, 3).ceil_int(), 0);
        assert_eq!(r(7, 2).floor_int(), 3);
        assert_eq!(r(7, 2).ceil_int(), 4);
        assert_eq!(r(5, 2).round_int(), Some(3));
        assert_eq!(r(-5, 2).round_int(), Some(-3));
        assert_eq!(r(9, 4).exact_sqrt(), Some(r(3, 2)));
        assert_eq!(r(2, 1).exact_sqrt(), None);
        assert_eq!(r(5, 2).to_string(), "5/2");
        assert_eq!(Rat::int(4).to_string(), "4");
    }

    #[test]
    fn expressions_give_the_values_of_a_table_made_by_hand() {
        let table: &[(&str, Value)] = &[
            ("1 + 2 * 3", num(7, 1)),
            ("(1 + 2) * 3", num(9, 1)),
            ("2 ^ 3 ^ 2", num(512, 1)),
            ("-2 ^ 2", num(-4, 1)),
            ("(-2) ^ 2", num(4, 1)),
            ("2 ^ -2", num(1, 4)),
            ("0 ^ 0", num(1, 1)),
            ("10 / 4", num(5, 2)),
            ("10 / 4 * 2", num(5, 1)),
            ("7 - 2 - 1", num(4, 1)),
            ("100 / 10 / 5", num(2, 1)),
            ("--3", num(3, 1)),
            ("+3", num(3, 1)),
            ("0.25 + 0.5", num(3, 4)),
            ("2 mm + 1 mm", len(3_000_000)),
            ("1 in", len(25_400_000)),
            ("62 mil", len(1_574_800)),
            ("3 um", len(3_000)),
            ("2 cm", len(20_000_000)),
            ("1 m", len(1_000_000_000)),
            ("5 nm", len(5)),
            ("(1 / 16) in", len(1_587_500)),
            ("2 * 3 mm", len(6_000_000)),
            ("3 mm * 2", len(6_000_000)),
            ("6 mm / 4", len(1_500_000)),
            ("6 mm / 3 mm", num(2, 1)),
            ("-10 mm / 4", len(-2_500_000)),
            ("round(10 mm / 3)", len(3_333_333)),
            ("ceil(10 mm / 3)", len(3_333_334)),
            ("floor(10 mm / 3)", len(3_333_333)),
            ("floor(-10 mm / 3)", len(-3_333_334)),
            ("round(2.5)", num(3, 1)),
            ("round(-2.5)", num(-3, 1)),
            ("round(1.2345 mm, 0.01 mm)", len(1_230_000)),
            ("ceil(1.2345 mm, 0.01 mm)", len(1_240_000)),
            ("floor(1.2345 mm, 0.01 mm)", len(1_230_000)),
            ("min(3, 1, 2)", num(1, 1)),
            ("max(1 mm, 2 mm)", len(2_000_000)),
            ("min(4)", num(4, 1)),
            ("abs(-3 mm)", len(3_000_000)),
            ("abs(3 mm)", len(3_000_000)),
            ("clamp(15, 0, 10)", num(10, 1)),
            ("clamp(-5 mm, 0 mm, 10 mm)", len(0)),
            ("clamp(5 mm, 0 mm, 10 mm)", len(5_000_000)),
            ("sqrt(16)", num(4, 1)),
            ("sqrt(9 / 4)", num(3, 2)),
            ("hypot(3 mm, 4 mm)", len(5_000_000)),
            ("hypot(1 mm, 1 mm)", len(1_414_214)),
            ("sin(30 deg)", num(1, 2)),
            ("cos(60 deg)", num(1, 2)),
            ("sin(90 deg)", num(1, 1)),
            ("cos(180 deg)", num(-1, 1)),
            ("sin(0.5 turn)", num(0, 1)),
            ("sin(45 deg)", num(707_106_781, 1_000_000_000)),
            ("sin(30 deg) * 10 mm", len(5_000_000)),
            ("30 deg * 2", Value::Angle(Rat::int(60_000_000_000))),
            ("1 turn / 4", Value::Angle(Rat::int(90_000_000_000))),
            ("90 deg / 30 deg", num(3, 1)),
            ("if(2 > 1, 10 mm, 20 mm)", len(10_000_000)),
            ("if(2 < 1, 10 mm, 20 mm)", len(20_000_000)),
            ("if(1 > 0, 5 mm, 1 mm / 0)", len(5_000_000)),
            ("1 < 2 and 3 >= 3", Value::Bool(true)),
            ("1 < 2 and 3 > 3", Value::Bool(false)),
            ("not (1 == 2)", Value::Bool(true)),
            ("not 1 == 2", Value::Bool(true)),
            ("1 == 2 or 2 != 3", Value::Bool(true)),
            ("true or 1 / 0 == 0", Value::Bool(true)),
            ("false and 1 / 0 == 0", Value::Bool(false)),
            ("true == false", Value::Bool(false)),
            ("1 mm == 1000 um", Value::Bool(true)),
            ("1 mm < 1 in", Value::Bool(true)),
            ("90 deg <= 0.25 turn", Value::Bool(true)),
            ("1 / 3 == 2 / 6", Value::Bool(true)),
        ];
        for (text, expected) in table {
            assert_eq!(ev(text).as_ref(), Ok(expected), "{text}");
        }
    }

    #[test]
    fn names_are_looked_up_in_the_environment() {
        let mut env = BTreeMap::new();
        env.insert("wall".to_string(), len(2_000_000));
        env.insert("count".to_string(), num(3, 1));
        assert_eq!(ev_in("wall * 2 + 1 mm", &env), Ok(len(5_000_000)));
        assert_eq!(ev_in("count * wall", &env), Ok(len(6_000_000)));
        assert_eq!(
            ev_in("wall + nothing", &env).unwrap_err(),
            CadError::UnknownName("nothing (column 8)".into())
        );
        let e = parse("wall * count + wall", &limits()).unwrap();
        assert_eq!(
            e.names(),
            vec![
                ("wall".to_string(), 0),
                ("count".to_string(), 7),
                ("wall".to_string(), 15)
            ]
        );
    }

    #[test]
    fn a_result_becomes_a_length_rounded_to_the_nanometre_or_is_refused() {
        let e = parse("10 mm / 3", &limits()).unwrap();
        let none = Constants::new();
        assert_eq!(e.eval_length(&none, &limits()).unwrap().get(), 3_333_333);
        let e = parse("20 mm / 3", &limits()).unwrap();
        assert_eq!(e.eval_length(&none, &limits()).unwrap().get(), 6_666_667);
        let e = parse("2", &limits()).unwrap();
        assert_eq!(
            e.eval_length(&none, &limits()).unwrap_err().code(),
            "PHOENIX-CAD-024"
        );
        let e = parse("1000000 m", &limits()).unwrap();
        assert_eq!(
            e.eval_length(&none, &limits()).unwrap_err().code(),
            "PHOENIX-CAD-001"
        );
    }

    #[test]
    fn text_that_is_not_the_language_is_refused_with_the_column() {
        for text in [
            "",
            "1 +",
            "(1",
            "1)",
            "1 2",
            "1 $ 2",
            "foo(1)",
            "min()",
            "if(1, 2)",
            "abs(1, 2)",
            "1 < 2 < 3",
            "a = 1",
            "1.5.2",
            "1.",
            ".5",
            "mm",
            "2 mm mm",
            "x !",
            "1 +* 2",
            "and",
            "not",
            "1 and",
            "min(1,",
            "sqrt 4",
            "2 \u{e9}",
            "1\n+2",
            "1234567890123456789",
        ] {
            let err = parse(text, &limits()).unwrap_err();
            assert_eq!(err.code(), "PHOENIX-CAD-023", "{text:?}: {err}");
        }
        let err = parse("1 + $", &limits()).unwrap_err();
        assert!(err.to_string().contains("column 5"), "{err}");
        let err = parse("2 * (3 + )", &limits()).unwrap_err();
        assert!(err.to_string().contains("column 10"), "{err}");
        let err = parse("2 mm mm", &limits()).unwrap_err();
        assert!(err.to_string().contains("column 6"), "{err}");
    }

    #[test]
    fn a_calculation_that_makes_no_sense_is_refused_and_says_where() {
        for text in [
            "1 mm + 1",
            "1 + 1 mm",
            "1 mm + 1 deg",
            "1 mm * 1 mm",
            "1 / 0",
            "1 mm / 0 mm",
            "1 mm / 0",
            "sqrt(2)",
            "sqrt(-4)",
            "sqrt(4 mm)",
            "2 ^ 0.5",
            "2 mm ^ 2",
            "0 ^ -1",
            "sin(1)",
            "sin(true)",
            "cos(2 mm)",
            "1 and 2",
            "not 1",
            "false or 1",
            "-true",
            "true < false",
            "1 mm < 1",
            "1 / 3 mm",
            "min(1 mm, 1)",
            "max(true, false)",
            "clamp(1, 5, 2)",
            "clamp(1, 0, 1 mm)",
            "round(1, 0)",
            "round(1, 1 mm)",
            "floor(true)",
            "hypot(1, 2)",
            "(1 mm) mm",
            "true mm",
            "if(1, 2, 3)",
            "sin(100000000000 deg)",
        ] {
            assert_eq!(code_of(text), "PHOENIX-CAD-024", "{text}");
        }
        let err = ev("1 mm + 1").unwrap_err();
        assert!(err.to_string().contains("column 6"), "{err}");
        assert!(
            err.to_string().contains("cannot add a length and a number"),
            "{err}"
        );
        let err = ev("2 * (1 mm / 0)").unwrap_err();
        assert!(err.to_string().contains("column 11"), "{err}");
        let err = ev("sqrt(2)").unwrap_err();
        assert!(err.to_string().contains("is not exact"), "{err}");
    }

    #[test]
    fn an_integer_that_does_not_fit_is_an_error_and_never_a_wrong_number() {
        for text in [
            "999999999999999999 * 999999999999999999 * 999999999999999999",
            "999999999999999999 m * 999999999999999999 * 999999999999999999",
            "10 ^ 40",
            "2 ^ 63 * 2 ^ 63 * 2",
            "(0 - 2 ^ 63 * 2 ^ 63) - 2 ^ 63 * 2 ^ 63 - 2 ^ 63 * 2 ^ 63",
            "1 / 10 ^ 39",
            "1 / 3 ^ 60 + 1 / 5 ^ 60",
        ] {
            assert_eq!(code_of(text), "PHOENIX-CAD-026", "{text}");
        }
        assert_eq!(
            ev("2 ^ 63 * 2 ^ 63"),
            Ok(Value::Number(Rat::int(1i128 << 126)))
        );
        assert_eq!(ev("2 ^ -63").map(|v| v.kind_name()), Ok("number"));
        let err = ev("10 ^ 40").unwrap_err();
        assert!(err.to_string().contains("column 4"), "{err}");
    }

    #[test]
    fn the_limits_of_length_depth_steps_and_exponent_are_hard() {
        let lim = limits();
        let long = format!("1{}", " + 1".repeat(200));
        assert!(long.len() > lim.max_length as usize);
        assert_eq!(code_of(&long), "PHOENIX-CAD-025");
        // as long as the limit is allowed
        let exact = String::from("1") + &" ".repeat(lim.max_length as usize - 1);
        assert_eq!(exact.len(), lim.max_length as usize);
        assert!(parse(&exact, &lim).is_ok());
        let more = format!("{exact} ");
        assert_eq!(parse(&more, &lim).unwrap_err().code(), "PHOENIX-CAD-025");
        // deep signs, deep parentheses and a long chain all stop at the depth
        let signs = format!("{}1", "-".repeat(lim.max_depth as usize + 1));
        assert_eq!(code_of(&signs), "PHOENIX-CAD-025");
        let parens = format!(
            "{}1{}",
            "(".repeat(lim.max_depth as usize + 1),
            ")".repeat(lim.max_depth as usize + 1)
        );
        assert_eq!(code_of(&parens), "PHOENIX-CAD-025");
        let chain = format!("1{}", "+1".repeat(lim.max_depth as usize + 1));
        assert_eq!(code_of(&chain), "PHOENIX-CAD-025");
        let nots = format!("{}true", "not ".repeat(lim.max_depth as usize + 1));
        assert_eq!(code_of(&nots), "PHOENIX-CAD-025");
        // below the depth it is fine
        let ok_chain = format!("1{}", "+1".repeat(lim.max_depth as usize - 2));
        assert!(ev(&ok_chain).is_ok());
        // the steps
        let tight = ExprLimits::new(512, 64, 6, 64).unwrap();
        let e = parse("1 + 2 + 3 + 4", &tight).unwrap();
        let err = e.eval(&Constants::new(), &tight).unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-025");
        let e = parse("1 + 2 + 3", &tight).unwrap();
        assert!(e.eval(&Constants::new(), &tight).is_ok());
        // a budget is shared by the evaluations that use it
        let mut budget = Budget::new(&tight);
        let e = parse("1 + 2", &tight).unwrap();
        assert!(e.eval_with(&Constants::new(), &tight, &mut budget).is_ok());
        assert_eq!(budget.left(), 3);
        assert!(e.eval_with(&Constants::new(), &tight, &mut budget).is_ok());
        assert_eq!(
            e.eval_with(&Constants::new(), &tight, &mut budget)
                .unwrap_err()
                .code(),
            "PHOENIX-CAD-025"
        );
        // the exponent
        assert_eq!(code_of("2 ^ 65"), "PHOENIX-CAD-025");
        assert_eq!(code_of("2 ^ -65"), "PHOENIX-CAD-025");
        assert_eq!(ev("2 ^ 64"), Ok(Value::Number(Rat::int(1i128 << 64))));
    }

    #[test]
    fn the_limits_have_hard_caps() {
        assert!(ExprLimits::new(1, 1, 1, 1).is_ok());
        assert!(ExprLimits::new(
            HARD_MAX_LENGTH,
            HARD_MAX_DEPTH,
            HARD_MAX_STEPS,
            HARD_MAX_EXPONENT
        )
        .is_ok());
        for bad in [
            ExprLimits::new(0, 1, 1, 1),
            ExprLimits::new(1, 0, 1, 1),
            ExprLimits::new(1, 1, 0, 1),
            ExprLimits::new(1, 1, 1, 0),
            ExprLimits::new(HARD_MAX_LENGTH + 1, 1, 1, 1),
            ExprLimits::new(1, HARD_MAX_DEPTH + 1, 1, 1),
            ExprLimits::new(1, 1, HARD_MAX_STEPS + 1, 1),
            ExprLimits::new(1, 1, 1, HARD_MAX_EXPONENT + 1),
        ] {
            assert_eq!(bad.unwrap_err().code(), "PHOENIX-CAD-011");
        }
    }

    #[test]
    fn the_branch_that_is_not_taken_is_not_evaluated() {
        assert_eq!(ev("if(true, 1, 1 / 0)"), Ok(num(1, 1)));
        assert_eq!(ev("if(false, 1 / 0, 2)"), Ok(num(2, 1)));
        assert_eq!(code_of("if(false, 1, 1 / 0)"), "PHOENIX-CAD-024");
        // the names of the branch not taken are still listed, so the graph knows the dependency
        let e = parse("if(a > 1, b, c)", &limits()).unwrap();
        let names: Vec<String> = e.names().into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, vec!["a", "b", "c"]);
    }

    fn printed_and_read_back(text: &str) {
        let e = parse(text, &limits()).unwrap();
        let printed = e.to_text();
        let again = parse(&printed, &limits()).unwrap();
        assert_eq!(e, again, "{text} -> {printed}");
        assert_eq!(printed, again.to_text(), "{printed}");
    }

    #[test]
    fn writing_an_expression_and_reading_it_back_gives_the_same_expression() {
        for text in [
            "1",
            "007",
            "1.50 mm",
            "0.05",
            "a + b * (c - d) / 2 mm",
            "a - (b - c)",
            "a - b - c",
            "a / (b / c)",
            "a * (b + c)",
            "(a + b) * c",
            "-a ^ 2",
            "(-a) ^ 2",
            "a ^ b ^ c",
            "(a ^ b) ^ c",
            "a ^ -b",
            "not (a and b) or c",
            "not a and b",
            "not not a",
            "a or b and c",
            "(a or b) and c",
            "a < b",
            "(a < b) == (c >= d)",
            "min(a, 2 * b) ^ 2",
            "-(2 mm)",
            "(-2) mm",
            "2 mm ^ 2",
            "(2 mm) mm",
            "if(a > 1 and b < 2, 3 mm, 4 mm) + 1 in",
            "round(a / 3, 0.1 mm)",
            "sin(30 deg) * 10 mm",
            "a - -b",
            "- - a",
            "a * -b",
            "(a + b) mm",
            "true == (a < b)",
        ] {
            printed_and_read_back(text);
        }
        // the canonical text keeps only the parentheses that are needed
        let e = parse("((a)) + (b * c)", &limits()).unwrap();
        assert_eq!(e.to_text(), "a + b * c");
        assert_eq!(e.to_string(), "a + b * c");
        let e = parse("2mm+1.50mm", &limits()).unwrap();
        assert_eq!(e.to_text(), "2 mm + 1.50 mm");
    }

    /// A small deterministic generator of numbers for the property tests.
    struct Rng(u64);

    impl Rng {
        fn below(&mut self, bound: u32) -> u32 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((self.0 >> 33) as u32) % bound
        }
    }

    fn wide_limits() -> ExprLimits {
        ExprLimits::new(65_536, 128, 1_000_000, 64).unwrap()
    }

    fn node(kind: Kind) -> Expr {
        Expr {
            kind,
            pos: 0,
            depth: 1,
        }
    }

    fn generate(rng: &mut Rng, depth: u32) -> Expr {
        const OPS: [Op; 13] = [
            Op::Or,
            Op::And,
            Op::Lt,
            Op::Le,
            Op::Gt,
            Op::Ge,
            Op::Eq,
            Op::Ne,
            Op::Add,
            Op::Sub,
            Op::Mul,
            Op::Div,
            Op::Pow,
        ];
        const NAMES: [&str; 4] = ["a", "b", "wall", "x1"];
        let units = [
            UnitWord::Len(Unit::Millimetre),
            UnitWord::Len(Unit::Inch),
            UnitWord::Deg,
            UnitWord::Turn,
        ];
        let leaf = depth == 0 || rng.below(4) == 0;
        if leaf {
            return match rng.below(3) {
                0 => node(Kind::Literal {
                    mantissa: i128::from(rng.below(1000)),
                    scale: rng.below(4),
                }),
                1 => node(Kind::Bool(rng.below(2) == 0)),
                _ => node(Kind::Name(NAMES[rng.below(4) as usize].to_string())),
            };
        }
        match rng.below(6) {
            0 => node(Kind::Neg(Box::new(generate(rng, depth - 1)))),
            1 => node(Kind::Not(Box::new(generate(rng, depth - 1)))),
            2 => {
                let func = Func::ALL[rng.below(12) as usize];
                let (lo, hi) = func.arity();
                let count = if hi == usize::MAX {
                    lo + rng.below(3) as usize
                } else {
                    lo + rng.below((hi - lo + 1) as u32) as usize
                };
                let args = (0..count).map(|_| generate(rng, depth - 1)).collect();
                node(Kind::Call(func, args))
            }
            3 => node(Kind::Unit(
                Box::new(generate(rng, depth - 1)),
                units[rng.below(4) as usize],
            )),
            _ => node(Kind::Binary(
                OPS[rng.below(13) as usize],
                Box::new(generate(rng, depth - 1)),
                Box::new(generate(rng, depth - 1)),
            )),
        }
    }

    #[test]
    fn property_every_generated_expression_reads_back_equal_to_what_was_written() {
        let wide = wide_limits();
        let mut rng = Rng(0x5eed);
        for round in 0..1500 {
            let depth = 1 + rng.below(5);
            let e = generate(&mut rng, depth);
            let printed = e.to_text();
            let again = parse(&printed, &wide)
                .unwrap_or_else(|err| panic!("round {round}: {printed}: {err}"));
            assert_eq!(e, again, "round {round}: {printed}");
            assert_eq!(printed, again.to_text(), "round {round}");
        }
    }

    #[test]
    fn property_the_evaluation_is_deterministic_and_never_panics() {
        let wide = wide_limits();
        let mut rng = Rng(77);
        let mut env = BTreeMap::new();
        env.insert("a".to_string(), num(3, 2));
        env.insert("b".to_string(), len(5_000_000));
        env.insert("wall".to_string(), len(2_000_000));
        env.insert("x1".to_string(), Value::Bool(true));
        for _ in 0..1500 {
            let depth = 1 + rng.below(5);
            let e = generate(&mut rng, depth);
            let first = e.eval(&env, &wide);
            let second = e.eval(&env, &wide);
            assert_eq!(first, second, "{e}");
            // the same text read again gives the same result (an error is the same error, though
            // it points at another column)
            let reread = parse(&e.to_text(), &wide).unwrap().eval(&env, &wide);
            match (&first, &reread) {
                (Ok(a), Ok(b)) => assert_eq!(a, b, "{e}"),
                (Err(a), Err(b)) => assert_eq!(a.code(), b.code(), "{e}"),
                _ => panic!("{e}: {first:?} against {reread:?}"),
            }
        }
    }

    #[test]
    fn property_a_sum_does_not_depend_on_the_order_of_its_terms() {
        let mut rng = Rng(5);
        for _ in 0..300 {
            let terms: Vec<i64> = (0..6).map(|_| i64::from(rng.below(2000)) - 1000).collect();
            let text = |order: &[usize]| {
                let mut s = format!("{} um", terms[order[0]]);
                for i in &order[1..] {
                    s.push_str(&format!(" + ({} um)", terms[*i]));
                }
                s
            };
            let forward = ev(&text(&[0, 1, 2, 3, 4, 5])).unwrap();
            let backward = ev(&text(&[5, 4, 3, 2, 1, 0])).unwrap();
            let shuffled = ev(&text(&[2, 5, 0, 3, 1, 4])).unwrap();
            let sum: i64 = terms.iter().sum();
            assert_eq!(forward, len(i128::from(sum) * 1000));
            assert_eq!(forward, backward);
            assert_eq!(forward, shuffled);
        }
    }

    #[test]
    fn property_rounding_keeps_the_value_within_half_a_step() {
        let mut rng = Rng(9);
        for _ in 0..300 {
            let value = i64::from(rng.below(2_000_000));
            let step = 1 + i64::from(rng.below(5000));
            let text = format!("round({value} nm, {step} nm)");
            let Value::Length(r) = ev(&text).unwrap() else {
                panic!("not a length");
            };
            let got = r.numer();
            assert_eq!(got % i128::from(step), 0, "{text}");
            assert!(
                (got - i128::from(value)).abs() * 2 <= i128::from(step),
                "{text}"
            );
            let down = ev(&format!("floor({value} nm, {step} nm)")).unwrap();
            let up = ev(&format!("ceil({value} nm, {step} nm)")).unwrap();
            let Value::Length(d) = down else { panic!() };
            let Value::Length(u) = up else { panic!() };
            assert!(d.numer() <= i128::from(value) && i128::from(value) <= u.numer());
            assert!(u.numer() - d.numer() <= i128::from(step));
        }
    }

    #[test]
    fn the_units_agree_with_the_reading_of_lengths() {
        use crate::units::parse_length;
        for text in [
            "1.6 mm", "62 mil", "1 in", "2 cm", "3 um", "0.25 mm", "7 nm", "1 m",
        ] {
            let expected = parse_length(text).unwrap().get();
            let got = parse(text, &limits())
                .unwrap()
                .eval_length(&Constants::new(), &limits())
                .unwrap()
                .get();
            assert_eq!(got, expected, "{text}");
        }
    }

    #[test]
    fn the_words_of_the_language_cannot_be_names() {
        for word in [
            "and", "or", "not", "true", "false", "deg", "turn", "mm", "um", "cm", "m", "in", "mil",
            "nm",
        ] {
            assert!(is_reserved(word), "{word}");
            let mut c = Constants::new();
            assert_eq!(
                c.set(word, num(1, 1)).unwrap_err().code(),
                "PHOENIX-CAD-017",
                "{word}"
            );
        }
        assert!(!is_reserved("wall"));
        assert!(!is_reserved("min"));
        let mut c = Constants::new();
        c.set("nozzle", len(400_000)).unwrap();
        assert_eq!(c.get("nozzle"), Some(len(400_000)));
        assert_eq!(c.names(), vec!["nozzle"]);
        assert_eq!(c.len(), 1);
        assert!(!c.is_empty());
        assert!(Constants::new().is_empty());
    }

    #[test]
    fn a_variable_called_like_a_function_is_still_a_variable() {
        let mut env = BTreeMap::new();
        env.insert("min".to_string(), len(1_000_000));
        assert_eq!(ev_in("min + 1 mm", &env), Ok(len(2_000_000)));
        assert_eq!(ev_in("min(min, 3 mm)", &env), Ok(len(1_000_000)));
    }

    #[test]
    fn the_guide_lists_every_function_and_every_code_of_the_language() {
        let guide = include_str!("../../../docs/EXPRESSIONS.md");
        for name in function_names() {
            assert!(
                guide.contains(&format!("`{name}(")),
                "{name} is not in the guide"
            );
        }
        for code in [
            "PHOENIX-CAD-023",
            "PHOENIX-CAD-024",
            "PHOENIX-CAD-025",
            "PHOENIX-CAD-026",
        ] {
            assert!(guide.contains(code), "{code}");
        }
        for word in [
            "mm", "um", "cm", "nm", "in", "mil", "deg", "turn", "and", "or", "not",
        ] {
            assert!(guide.contains(&format!("`{word}`")), "{word}");
        }
        assert_eq!(function_names().len(), 12);
    }
}
