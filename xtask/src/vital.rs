//! Writes `src/preset/vital/registry.json` from a Vital source checkout.
//!
//! The checkout is data. The task reads its text and runs nothing in it. The
//! parser is our own: it takes names, numbers and labels from the parameter
//! table and writes them in our schema, with the unit and the law that turn a
//! stored value into a physical one.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

/// Where the generated registry lives, relative to the workspace root.
const OUTPUT: &str = "src/preset/vital/registry.json";

/// Totals the real checkout must reach before the file is written.
const REAL: Expect = Expect {
    keys: 794,
    legacy: 22,
    all_rules_used: true,
};

/// Totals a parse must reach, so a changed checkout fails loudly.
struct Expect {
    keys: usize,
    legacy: usize,
    /// Every name rule and legacy rule must match a control.
    all_rules_used: bool,
}

/// Writes the registry, or with `check` compares it with the committed file.
pub(crate) fn run(root: &Path, source: &Path, check: bool) -> crate::Result<()> {
    let text = render(source, &REAL)?;
    let out = root.join(OUTPUT);
    if check {
        let committed = fs::read_to_string(&out).map_err(|err| {
            format!(
                "cannot read {}: {err}. Run `cargo xtask vital-registry --source <checkout>`",
                out.display()
            )
        })?;
        if committed != text {
            return Err(format!(
                "{OUTPUT} differs from the Vital checkout at {}. \
                 Run `cargo xtask vital-registry --source <checkout>` and commit the result",
                source.display()
            )
            .into());
        }
        return Ok(());
    }
    if let Some(dir) = out.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(&out, text)?;
    Ok(())
}

/// The registry text for the checkout at `source`.
fn render(source: &Path, expect: &Expect) -> crate::Result<String> {
    let parsed = parse(source)?;
    let doc = build(&parsed, &commit(source)?, expect)?;
    Ok(format!("{}\n", serde_json::to_string_pretty(&doc)?))
}

/// The commit a checkout sits on, read from its `.git` directory.
fn commit(source: &Path) -> crate::Result<String> {
    let git = source.join(".git");
    let head = fs::read_to_string(git.join("HEAD")).map_err(|err| {
        format!(
            "cannot read {}/HEAD: {err}. Point --source at a git checkout of Vital",
            git.display()
        )
    })?;
    let head = head.trim();
    let hash = match head.strip_prefix("ref: ") {
        None => head.to_owned(),
        Some(name) => match fs::read_to_string(git.join(name)) {
            Ok(text) => text.trim().to_owned(),
            Err(_) => fs::read_to_string(git.join("packed-refs"))?
                .lines()
                .find_map(|line| line.strip_suffix(name)?.strip_suffix(' '))
                .ok_or_else(|| format!("{name} is not in {}/packed-refs", git.display()))?
                .to_owned(),
        },
    };
    if hash.len() == 40 && hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(hash)
    } else {
        Err(format!("`{hash}` in {} is not a commit hash", git.display()).into())
    }
}

/// One token of C++ text. Comments and preprocessor lines never reach it.
#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Ident(String),
    /// A numeric literal as written, such as `0x000507` or `1.5f`.
    Num(String),
    Str(String),
    Punct(char),
}

/// Removes `//` and `/* */` comments outside string and character literals.
fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' | '\'' => {
                out.push(c);
                while let Some(d) = chars.next() {
                    out.push(d);
                    if d == c || d == '\n' {
                        break;
                    }
                    if d == '\\'
                        && let Some(e) = chars.next()
                    {
                        out.push(e);
                    }
                }
            }
            '/' if chars.peek() == Some(&'/') => {
                while chars.peek().is_some_and(|&d| d != '\n') {
                    chars.next();
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut prev = ' ';
                for d in chars.by_ref() {
                    if d == '\n' {
                        out.push('\n');
                    }
                    if prev == '*' && d == '/' {
                        break;
                    }
                    prev = d;
                }
                out.push(' ');
            }
            _ => out.push(c),
        }
    }
    out
}

/// Splits comment-free text into tokens. Preprocessor lines are skipped.
fn tokenize(text: &str) -> Vec<Tok> {
    let chars: Vec<char> = text.chars().collect();
    let mut toks = Vec::new();
    let mut i = 0;
    let mut fresh_line = true;
    while let Some(&c) = chars.get(i) {
        if c == '\n' {
            fresh_line = true;
            i += 1;
        } else if c.is_whitespace() {
            i += 1;
        } else if c == '#' && fresh_line {
            while chars
                .get(i)
                .is_some_and(|&d| d != '\n' || chars.get(i.wrapping_sub(1)) == Some(&'\\'))
            {
                i += 1;
            }
        } else {
            fresh_line = false;
            i = token_at(&chars, i, &mut toks);
        }
    }
    toks
}

/// Reads the token that starts at `chars[i]` and returns the next index.
fn token_at(chars: &[char], start: usize, toks: &mut Vec<Tok>) -> usize {
    let mut i = start + 1;
    let c = chars[start];
    let next_is_digit = chars.get(i).is_some_and(char::is_ascii_digit);
    if c == '"' {
        let mut s = String::new();
        while let Some(&d) = chars.get(i) {
            i += 1;
            match d {
                '"' | '\n' => break,
                '\\' => {
                    let e = chars.get(i).copied().unwrap_or('\\');
                    i += 1;
                    if e != '"' && e != '\\' {
                        s.push('\\');
                    }
                    s.push(e);
                }
                _ => s.push(d),
            }
        }
        toks.push(Tok::Str(s));
    } else if c == '\'' {
        while let Some(&d) = chars.get(i) {
            i += 1;
            if d == '\'' || d == '\n' {
                break;
            }
            if d == '\\' {
                i += 1;
            }
        }
        toks.push(Tok::Punct('\''));
    } else if c.is_ascii_digit() || (c == '.' && next_is_digit) {
        let hex = c == '0' && matches!(chars.get(i), Some('x' | 'X'));
        while let Some(&d) = chars.get(i) {
            let exponent_sign = matches!(d, '+' | '-') && !hex && matches!(chars[i - 1], 'e' | 'E');
            if d.is_ascii_alphanumeric() || d == '.' || exponent_sign {
                i += 1;
            } else {
                break;
            }
        }
        toks.push(Tok::Num(chars[start..i].iter().collect()));
    } else if c.is_alphabetic() || c == '_' {
        while chars
            .get(i)
            .is_some_and(|&d| d.is_alphanumeric() || d == '_')
        {
            i += 1;
        }
        toks.push(Tok::Ident(chars[start..i].iter().collect()));
    } else {
        toks.push(Tok::Punct(c));
    }
    i
}

/// The index of the bracket that closes the one at `open`.
fn matching(toks: &[Tok], open: usize) -> crate::Result<usize> {
    let mut depth = 0_usize;
    for (i, tok) in toks.iter().enumerate().skip(open) {
        match tok {
            Tok::Punct('(' | '{' | '[') => depth += 1,
            Tok::Punct(')' | '}' | ']') => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Ok(i);
                }
            }
            _ => {}
        }
    }
    Err("a bracket in the Vital source never closes. The checkout may be damaged".into())
}

/// Splits tokens at commas outside brackets. A trailing comma adds no field.
fn split_fields(toks: &[Tok]) -> Vec<&[Tok]> {
    let mut fields = Vec::new();
    let mut depth = 0_usize;
    let mut from = 0;
    for (i, tok) in toks.iter().enumerate() {
        match tok {
            Tok::Punct('(' | '{' | '[') => depth += 1,
            Tok::Punct(')' | '}' | ']') => depth = depth.saturating_sub(1),
            Tok::Punct(',') if depth == 0 => {
                fields.push(&toks[from..i]);
                from = i + 1;
            }
            _ => {}
        }
    }
    if from < toks.len() {
        fields.push(&toks[from..]);
    }
    fields
}

/// A value in a C++ constant expression. Integers stay integers, so `7 / 2`
/// is 3 as in C++.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Num {
    Int(i32),
    Float(f64),
}

impl Num {
    fn float(self) -> f64 {
        match self {
            Self::Int(i) => f64::from(i),
            Self::Float(f) => f,
        }
    }

    fn apply(self, op: char, rhs: Self) -> crate::Result<Self> {
        if let (Self::Int(a), Self::Int(b)) = (self, rhs) {
            let r = match op {
                '+' => a.checked_add(b),
                '-' => a.checked_sub(b),
                '*' => a.checked_mul(b),
                _ => a.checked_div(b),
            };
            return r
                .map(Self::Int)
                .ok_or_else(|| format!("`{a} {op} {b}` overflows or divides by zero").into());
        }
        let (a, b) = (self.float(), rhs.float());
        Ok(Self::Float(match op {
            '+' => a + b,
            '-' => a - b,
            '*' => a * b,
            _ => a / b,
        }))
    }
}

/// The value of a numeric literal: hex and decimal integers, and decimal
/// floats with an optional `f` suffix.
fn literal(text: &str) -> crate::Result<Num> {
    let bad = || format!("`{text}` is not a number this task understands");
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        return i32::from_str_radix(hex, 16)
            .map(Num::Int)
            .map_err(|_| bad().into());
    }
    let is_float = text.contains(['.', 'e', 'E']);
    let digits = if is_float {
        text.strip_suffix(['f', 'F']).unwrap_or(text)
    } else {
        text
    };
    if is_float {
        digits.parse().map(Num::Float).map_err(|_| bad().into())
    } else {
        digits.parse().map(Num::Int).map_err(|_| bad().into())
    }
}

/// What a name stands for in the headers.
#[derive(Debug)]
enum Symbol {
    Value(i32),
    Expr(Vec<Tok>),
    /// Two headers define the same scoped name. Using it is an error.
    Ambiguous,
}

/// Enumerators and `constexpr` constants of the headers, by class scope.
#[derive(Debug, Default)]
struct Symbols {
    by_scope: BTreeMap<(String, String), Symbol>,
    scopes_of: BTreeMap<String, BTreeSet<String>>,
}

/// How deep a constant may refer to other constants.
const MAX_DEPTH: u8 = 16;

impl Symbols {
    fn add(&mut self, scope: &str, name: &str, symbol: Symbol) {
        let key = (scope.to_owned(), name.to_owned());
        let symbol = if self.by_scope.contains_key(&key) {
            Symbol::Ambiguous
        } else {
            symbol
        };
        self.by_scope.insert(key, symbol);
        self.scopes_of
            .entry(name.to_owned())
            .or_default()
            .insert(scope.to_owned());
    }

    /// Collects the symbols of one header's tokens.
    fn scan(&mut self, toks: &[Tok]) {
        let mut stack: Vec<Option<String>> = Vec::new();
        let mut pending: Option<String> = None;
        let mut i = 0;
        while let Some(tok) = toks.get(i) {
            let scope = stack
                .iter()
                .rev()
                .flatten()
                .next()
                .cloned()
                .unwrap_or_default();
            match tok {
                Tok::Ident(w) if w == "enum" => {
                    i = self.scan_enum(toks, i + 1, &scope);
                    continue;
                }
                Tok::Ident(w) if w == "constexpr" => {
                    i = self.scan_constexpr(toks, i + 1, &scope);
                    continue;
                }
                Tok::Ident(w) if w == "class" || w == "struct" => {
                    if let Some(Tok::Ident(name)) = toks.get(i + 1) {
                        pending = Some(name.clone());
                        i += 2;
                        continue;
                    }
                }
                Tok::Punct('{') => stack.push(pending.take()),
                Tok::Punct('}') => {
                    stack.pop();
                }
                Tok::Punct(';' | '(' | '=') => pending = None,
                _ => {}
            }
            i += 1;
        }
    }

    /// Reads an enum that starts after the `enum` keyword. Enumerators after
    /// one with a symbolic value stay unknown, so using them fails loudly.
    fn scan_enum(&mut self, toks: &[Tok], start: usize, scope: &str) -> usize {
        let mut j = start;
        while toks
            .get(j)
            .is_some_and(|t| *t != Tok::Punct('{') && *t != Tok::Punct(';'))
        {
            j += 1;
        }
        if toks.get(j) != Some(&Tok::Punct('{')) {
            return j;
        }
        let Ok(close) = matching(toks, j) else {
            return toks.len();
        };
        let mut next = 0_i32;
        for item in split_fields(&toks[j + 1..close]) {
            let Some(Tok::Ident(name)) = item.first() else {
                break;
            };
            match &item[1..] {
                [] => {}
                [Tok::Punct('='), rest @ ..] => match plain_int(rest) {
                    Some(v) => next = v,
                    None => break,
                },
                _ => break,
            }
            self.add(scope, name, Symbol::Value(next));
            next = next.saturating_add(1);
        }
        close + 1
    }

    /// Reads `constexpr <type> name = expr;` that starts after the keyword.
    fn scan_constexpr(&mut self, toks: &[Tok], start: usize, scope: &str) -> usize {
        let mut j = start;
        while toks
            .get(j)
            .is_some_and(|t| !matches!(t, Tok::Punct('=' | '(' | ';' | '{' | '[')))
        {
            j += 1;
        }
        if toks.get(j) != Some(&Tok::Punct('=')) || j < start + 2 {
            return start;
        }
        let Some(Tok::Ident(name)) = toks.get(j - 1) else {
            return start;
        };
        let mut end = j + 1;
        while toks.get(end).is_some_and(|t| *t != Tok::Punct(';')) {
            end += 1;
        }
        self.add(scope, name, Symbol::Expr(toks[j + 1..end].to_vec()));
        end + 1
    }

    /// Resolves a possibly qualified name such as `Class::kName`.
    fn lookup(&self, path: &[String], depth: u8) -> crate::Result<Num> {
        let shown = path.join("::");
        let (name, qualifier) = match path {
            [.., q, n] => (n, Some(q)),
            [n] => (n, None),
            [] => return Err("an empty name in a constant expression".into()),
        };
        let scoped = qualifier.and_then(|q| self.by_scope.get(&(q.clone(), name.clone())));
        let symbol = if let Some(symbol) = scoped {
            symbol
        } else {
            let defining = self.scopes_of.get(name);
            match defining.map(|s| s.iter().collect::<Vec<_>>()).as_deref() {
                Some([scope]) => &self.by_scope[&((*scope).clone(), name.clone())],
                other => {
                    return Err(format!(
                        "cannot resolve `{shown}`: {} headers define it. \
                         Teach xtask/src/vital.rs where it comes from",
                        other.map_or(0, <[_]>::len)
                    )
                    .into());
                }
            }
        };
        match symbol {
            Symbol::Value(v) => Ok(Num::Int(*v)),
            Symbol::Expr(toks) => {
                if depth >= MAX_DEPTH {
                    return Err(format!("`{shown}` refers to itself or nests too deep").into());
                }
                self.eval_at(toks, depth + 1)
                    .map_err(|err| format!("in `{shown}`: {err}").into())
            }
            Symbol::Ambiguous => Err(format!(
                "`{shown}` is defined twice in one scope. Qualify it in the source table \
                 or teach xtask/src/vital.rs which definition counts"
            )
            .into()),
        }
    }

    fn eval(&self, toks: &[Tok]) -> crate::Result<Num> {
        self.eval_at(toks, 0)
    }

    fn eval_at(&self, toks: &[Tok], depth: u8) -> crate::Result<Num> {
        let mut parser = Parser {
            symbols: self,
            toks,
            pos: 0,
            depth,
        };
        let value = parser.expr()?;
        if parser.pos == toks.len() {
            Ok(value)
        } else {
            Err(format!(
                "unexpected `{:?}` after a complete expression",
                toks[parser.pos]
            )
            .into())
        }
    }

    /// An expression as a finite `f64`.
    fn eval_f64(&self, toks: &[Tok]) -> crate::Result<f64> {
        let value = self.eval(toks)?.float();
        if value.is_finite() {
            Ok(value)
        } else {
            Err("a number in the table is not finite".into())
        }
    }
}

/// `5`, `-1` or `0x10` as an integer, and nothing more complex.
fn plain_int(toks: &[Tok]) -> Option<i32> {
    match toks {
        [Tok::Num(n)] => match literal(n).ok()? {
            Num::Int(i) => Some(i),
            Num::Float(_) => None,
        },
        [Tok::Punct('-'), Tok::Num(n)] => match literal(n).ok()? {
            Num::Int(i) => i.checked_neg(),
            Num::Float(_) => None,
        },
        _ => None,
    }
}

/// Recursive-descent evaluator for `+ - * /`, unary signs, parentheses,
/// numeric literals, names and `factorial(...)`.
struct Parser<'a> {
    symbols: &'a Symbols,
    toks: &'a [Tok],
    pos: usize,
    depth: u8,
}

impl Parser<'_> {
    fn eat(&mut self, c: char) -> bool {
        let found = self.toks.get(self.pos) == Some(&Tok::Punct(c));
        self.pos += usize::from(found);
        found
    }

    fn expr(&mut self) -> crate::Result<Num> {
        let mut value = self.term()?;
        loop {
            let op = if self.eat('+') {
                '+'
            } else if self.eat('-') {
                '-'
            } else {
                return Ok(value);
            };
            value = value.apply(op, self.term()?)?;
        }
    }

    fn term(&mut self) -> crate::Result<Num> {
        let mut value = self.unary()?;
        loop {
            let op = if self.eat('*') {
                '*'
            } else if self.eat('/') {
                '/'
            } else {
                return Ok(value);
            };
            value = value.apply(op, self.unary()?)?;
        }
    }

    fn unary(&mut self) -> crate::Result<Num> {
        if self.eat('-') {
            Num::Int(0).apply('-', self.unary()?)
        } else if self.eat('+') {
            self.unary()
        } else {
            self.primary()
        }
    }

    fn primary(&mut self) -> crate::Result<Num> {
        let tok = self.toks.get(self.pos).cloned();
        self.pos += 1;
        match tok {
            Some(Tok::Num(text)) => literal(&text),
            Some(Tok::Punct('(')) => {
                let value = self.expr()?;
                if self.eat(')') {
                    Ok(value)
                } else {
                    Err("a `(` in a constant expression never closes".into())
                }
            }
            Some(Tok::Ident(first)) => {
                let mut path = vec![first];
                while self.toks.get(self.pos) == Some(&Tok::Punct(':'))
                    && self.toks.get(self.pos + 1) == Some(&Tok::Punct(':'))
                    && let Some(Tok::Ident(part)) = self.toks.get(self.pos + 2)
                {
                    path.push(part.clone());
                    self.pos += 3;
                }
                if self.eat('(') {
                    self.call(&path)
                } else {
                    self.symbols.lookup(&path, self.depth)
                }
            }
            other => Err(format!("unexpected {other:?} in a constant expression").into()),
        }
    }

    fn call(&mut self, path: &[String]) -> crate::Result<Num> {
        if path.last().map(String::as_str) != Some("factorial") {
            return Err(
                format!("`{}(...)` is not supported in a constant", path.join("::")).into(),
            );
        }
        let Num::Int(n) = self.expr()? else {
            return Err("factorial needs an integer".into());
        };
        if !self.eat(')') {
            return Err("a `factorial(` never closes".into());
        }
        (2..=n)
            .try_fold(1_i32, i32::checked_mul)
            .map(Num::Int)
            .ok_or_else(|| format!("factorial({n}) overflows").into())
    }
}

/// How the engine and the display transform a stored value.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Scale {
    Indexed,
    Linear,
    Quadratic,
    Cubic,
    Quartic,
    SquareRoot,
    Exponential,
}

impl Scale {
    /// The name in our registry.
    fn name(self) -> &'static str {
        match self {
            Self::Indexed => "indexed",
            Self::Linear => "linear",
            Self::Quadratic => "quadratic",
            Self::Cubic => "cubic",
            Self::Quartic => "quartic",
            Self::SquareRoot => "square_root",
            Self::Exponential => "exponential",
        }
    }

    fn parse(name: &str) -> crate::Result<Self> {
        Ok(match name {
            "kIndexed" => Self::Indexed,
            "kLinear" => Self::Linear,
            "kQuadratic" => Self::Quadratic,
            "kCubic" => Self::Cubic,
            "kQuartic" => Self::Quartic,
            "kSquareRoot" => Self::SquareRoot,
            "kExponential" => Self::Exponential,
            other => {
                return Err(format!("unknown value scale `{other}` in the parameter table").into());
            }
        })
    }
}

/// One row of a parameter table: a control before it is copied per instance.
#[derive(Clone, Debug)]
struct Template {
    name: String,
    /// The version packed as `0xMMmmpp`.
    version: u32,
    min: f64,
    max: f64,
    default: f64,
    offset: f64,
    multiplier: f64,
    scale: Scale,
    invert: bool,
    units: String,
    label: String,
    labels: Option<Vec<String>>,
}

/// `0x000507` as `0.5.7`.
fn version_text(v: u32) -> String {
    format!("{}.{}.{}", (v >> 16) & 255, (v >> 8) & 255, v & 255)
}

/// A hex version literal such as `0x000507`.
fn hex_version(text: &str) -> crate::Result<u32> {
    text.strip_prefix("0x")
        .and_then(|hex| u32::from_str_radix(hex, 16).ok())
        .filter(|v| *v <= 0x00ff_ffff)
        .ok_or_else(|| format!("`{text}` is not a hex version like 0x000507").into())
}

/// `name = "text"` string constants of the table file.
fn string_const(toks: &[Tok], name: &str) -> crate::Result<String> {
    toks.windows(3)
        .find_map(|w| match w {
            [Tok::Ident(n), Tok::Punct('='), Tok::Str(s)] if n == name => Some(s.clone()),
            _ => None,
        })
        .ok_or_else(|| format!("the table file has no string constant `{name}`").into())
}

/// `const std::string kName[...] = {"a", "b"};` arrays, by name.
fn string_arrays(toks: &[Tok]) -> crate::Result<BTreeMap<String, Vec<String>>> {
    let mut arrays = BTreeMap::new();
    for (i, w) in toks.windows(6).enumerate() {
        let [
            Tok::Ident(std),
            Tok::Punct(':'),
            Tok::Punct(':'),
            Tok::Ident(ty),
            Tok::Ident(name),
            Tok::Punct('['),
        ] = w
        else {
            continue;
        };
        if std != "std" || ty != "string" {
            continue;
        }
        let close = matching(toks, i + 5)?;
        if toks.get(close + 1) != Some(&Tok::Punct('='))
            || toks.get(close + 2) != Some(&Tok::Punct('{'))
        {
            continue;
        }
        let end = matching(toks, close + 2)?;
        let mut labels = Vec::new();
        for field in split_fields(&toks[close + 3..end]) {
            let [Tok::Str(label)] = field else {
                return Err(format!("`{name}` holds something other than string literals").into());
            };
            labels.push(label.clone());
        }
        arrays.insert(name.clone(), labels);
    }
    Ok(arrays)
}

/// Reads `ValueDetailsLookup::<list>[] = { {...}, ... };`.
fn parse_list(
    toks: &[Tok],
    list: &str,
    symbols: &Symbols,
    strings: &BTreeMap<String, Vec<String>>,
) -> crate::Result<Vec<Template>> {
    let head = [
        Tok::Ident("ValueDetailsLookup".into()),
        Tok::Punct(':'),
        Tok::Punct(':'),
        Tok::Ident(list.into()),
        Tok::Punct('['),
        Tok::Punct(']'),
        Tok::Punct('='),
        Tok::Punct('{'),
    ];
    let at = toks
        .windows(head.len())
        .position(|w| w == head)
        .ok_or_else(|| format!("the table file has no list `{list}`"))?;
    let open = at + head.len() - 1;
    let close = matching(toks, open)?;
    let mut out = Vec::new();
    for entry in split_fields(&toks[open + 1..close]) {
        let (Some(Tok::Punct('{')), Some(Tok::Punct('}'))) = (entry.first(), entry.last()) else {
            return Err(format!("`{list}` holds something other than {{...}} rows").into());
        };
        let row = parse_row(&entry[1..entry.len() - 1], symbols, strings)
            .map_err(|err| format!("in `{list}` row {}: {err}", out.len() + 1))?;
        out.push(row);
    }
    Ok(out)
}

fn parse_row(
    row: &[Tok],
    symbols: &Symbols,
    strings: &BTreeMap<String, Vec<String>>,
) -> crate::Result<Template> {
    let fields = split_fields(row);
    let [
        name,
        version,
        min,
        max,
        default,
        offset,
        multiplier,
        scale,
        invert,
        units,
        label,
        labels,
    ] = fields.as_slice()
    else {
        return Err(format!("expected 12 fields, found {}", fields.len()).into());
    };
    let text = |field: &[Tok]| match field {
        [Tok::Str(s)] => Ok(s.clone()),
        _ => Err(format!("expected a string literal, found {field:?}")),
    };
    let last_ident = |field: &[Tok]| match field.last() {
        Some(Tok::Ident(s)) => Ok(s.clone()),
        _ => Err(format!("expected a name, found {field:?}")),
    };
    let name = text(name)?;
    let wrap = |err: Box<dyn std::error::Error>| format!("`{name}`: {err}");
    let version = match version {
        [Tok::Num(n)] => hex_version(n).map_err(wrap)?,
        other => return Err(format!("`{name}`: expected a hex version, found {other:?}").into()),
    };
    let invert = match invert {
        [Tok::Ident(b)] if b == "true" || b == "false" => b == "true",
        other => return Err(format!("`{name}`: expected true or false, found {other:?}").into()),
    };
    let labels = match last_ident(labels)?.as_str() {
        "nullptr" => None,
        array => Some(
            strings
                .get(array)
                .cloned()
                .ok_or_else(|| format!("`{name}`: no string array `{array}`"))?,
        ),
    };
    Ok(Template {
        version,
        min: symbols.eval_f64(min).map_err(wrap)?,
        max: symbols.eval_f64(max).map_err(wrap)?,
        default: symbols.eval_f64(default).map_err(wrap)?,
        offset: symbols.eval_f64(offset).map_err(wrap)?,
        multiplier: symbols.eval_f64(multiplier).map_err(wrap)?,
        scale: Scale::parse(&last_ident(scale)?)?,
        invert,
        units: text(units)?,
        label: text(label)?,
        labels,
        name,
    })
}

/// A group of per-instance controls, such as the oscillators.
#[derive(Debug)]
struct Group {
    prefix: String,
    label: String,
    ids: Vec<String>,
    /// Ids a later release added, and the version that added them.
    later: Option<(Vec<String>, u32)>,
    templates: Vec<Template>,
}

/// Everything read from the checkout.
#[derive(Debug)]
struct Parsed {
    /// The first group holds the global controls.
    groups: Vec<Group>,
    overrides: Vec<(String, f64)>,
}

/// How a group is built from the table file and the constructor.
struct GroupSpec {
    list: &'static str,
    id_prefix: &'static str,
    name_prefix: &'static str,
    count: &'static str,
    /// Constants of the constructor: instances before the later ones, and the
    /// version that added the rest.
    later: Option<(&'static str, &'static str)>,
    extra_id: Option<&'static str>,
}

const GROUPS: [GroupSpec; 6] = [
    GroupSpec {
        list: "env_parameter_list",
        id_prefix: "kEnvIdPrefix",
        name_prefix: "kEnvNamePrefix",
        count: "kNumEnvelopes",
        later: None,
        extra_id: None,
    },
    GroupSpec {
        list: "lfo_parameter_list",
        id_prefix: "kLfoIdPrefix",
        name_prefix: "kLfoNamePrefix",
        count: "kNumLfos",
        later: None,
        extra_id: None,
    },
    GroupSpec {
        list: "random_lfo_parameter_list",
        id_prefix: "kRandomIdPrefix",
        name_prefix: "kRandomNamePrefix",
        count: "kNumRandomLfos",
        later: None,
        extra_id: None,
    },
    GroupSpec {
        list: "osc_parameter_list",
        id_prefix: "kOscIdPrefix",
        name_prefix: "kOscNamePrefix",
        count: "kNumOscillators",
        later: Some(("kNumOscillatorsOld", "kNewOscillatorVersion")),
        extra_id: None,
    },
    GroupSpec {
        list: "filter_parameter_list",
        id_prefix: "kFilterIdPrefix",
        name_prefix: "kFilterNamePrefix",
        count: "kNumFilters",
        later: None,
        extra_id: Some("fx"),
    },
    GroupSpec {
        list: "mod_parameter_list",
        id_prefix: "kModulationIdPrefix",
        name_prefix: "kModulationNamePrefix",
        count: "kMaxModulationConnections",
        later: Some(("kOldMaxModulations", "kNewModulationVersion")),
        extra_id: None,
    },
];

/// Files under `dir` with the extension `h`, in a stable order.
fn headers(dir: &Path, out: &mut Vec<PathBuf>) -> crate::Result<()> {
    let mut paths = fs::read_dir(dir)
        .map_err(|err| {
            format!(
                "cannot read {}: {err}. Point --source at the root of a Vital checkout",
                dir.display()
            )
        })?
        .map(|entry| entry.map(|e| e.path()))
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    for path in paths {
        if path.is_dir() {
            headers(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "h") {
            out.push(path);
        }
    }
    Ok(())
}

fn read_tokens(path: &Path) -> crate::Result<Vec<Tok>> {
    let bytes = fs::read(path).map_err(|err| format!("cannot read {}: {err}", path.display()))?;
    Ok(tokenize(&strip_comments(&String::from_utf8_lossy(&bytes))))
}

/// Reads the parameter tables, strings and constants of the checkout.
fn parse(source: &Path) -> crate::Result<Parsed> {
    let mut files = Vec::new();
    headers(&source.join("src/synthesis"), &mut files)?;
    headers(&source.join("src/common"), &mut files)?;
    let mut symbols = Symbols::default();
    for file in &files {
        symbols.scan(&read_tokens(file)?);
    }
    let strings = string_arrays(&read_tokens(
        &source.join("src/interface/look_and_feel/synth_strings.h"),
    )?)?;
    let table = read_tokens(&source.join("src/common/synth_parameters.cpp"))?;
    let ctor_head = [
        Tok::Ident("ValueDetailsLookup".into()),
        Tok::Punct(':'),
        Tok::Punct(':'),
        Tok::Ident("ValueDetailsLookup".into()),
        Tok::Punct('('),
        Tok::Punct(')'),
        Tok::Punct('{'),
    ];
    let ctor_open = table
        .windows(ctor_head.len())
        .position(|w| w == ctor_head)
        .ok_or("the table file has no ValueDetailsLookup constructor")?
        + ctor_head.len()
        - 1;
    let ctor = &table[ctor_open..=matching(&table, ctor_open)?];

    let globals = parse_list(&table, "parameter_list", &symbols, &strings)?;
    let mut groups = vec![Group {
        prefix: String::new(),
        label: String::new(),
        ids: vec![String::new()],
        later: None,
        templates: globals,
    }];
    for spec in &GROUPS {
        groups.push(parse_group(spec, &table, ctor, &symbols, &strings)?);
    }
    Ok(Parsed {
        groups,
        overrides: default_overrides(ctor)?,
    })
}

fn parse_group(
    spec: &GroupSpec,
    table: &[Tok],
    ctor: &[Tok],
    symbols: &Symbols,
    strings: &BTreeMap<String, Vec<String>>,
) -> crate::Result<Group> {
    if !ctor.contains(&Tok::Ident(spec.list.into())) {
        return Err(format!(
            "the constructor no longer adds `{}`. Update the group table in xtask/src/vital.rs",
            spec.list
        )
        .into());
    }
    let Num::Int(count) = symbols.lookup(&[spec.count.to_owned()], 0)? else {
        return Err(format!("`{}` is not an integer", spec.count).into());
    };
    let count = usize::try_from(count).map_err(|_| format!("`{}` is negative", spec.count))?;
    let mut ids: Vec<String> = (1..=count).map(|n| n.to_string()).collect();
    let later = spec
        .later
        .map(|(old, version)| -> crate::Result<_> {
            let old = ctor_number(ctor, old)?
                .parse::<usize>()
                .map_err(|_| format!("`{old}` is not a count"))?;
            let version = hex_version(&ctor_number(ctor, version)?)?;
            if old >= count {
                return Err(format!("`{}` has no later instances", spec.list).into());
            }
            Ok((ids[old..].to_vec(), version))
        })
        .transpose()?;
    ids.extend(spec.extra_id.map(str::to_owned));
    Ok(Group {
        prefix: string_const(table, spec.id_prefix)?,
        label: string_const(table, spec.name_prefix)?,
        ids,
        later,
        templates: parse_list(table, spec.list, symbols, strings)?,
    })
}

/// The literal assigned to `name` inside the constructor.
fn ctor_number(ctor: &[Tok], name: &str) -> crate::Result<String> {
    ctor.windows(3)
        .find_map(|w| match w {
            [Tok::Ident(n), Tok::Punct('='), Tok::Num(v)] if n == name => Some(v.clone()),
            _ => None,
        })
        .ok_or_else(|| format!("the constructor does not set `{name}`").into())
}

/// The default values the constructor sets after the groups are added.
fn default_overrides(ctor: &[Tok]) -> crate::Result<Vec<(String, f64)>> {
    let mut found = Vec::new();
    for w in ctor.windows(10) {
        let [
            Tok::Ident(table),
            Tok::Punct('['),
            Tok::Str(key),
            Tok::Punct(']'),
            Tok::Punct('.'),
            Tok::Ident(field),
            Tok::Punct('='),
            Tok::Num(value),
            Tok::Punct(';'),
            _,
        ] = w
        else {
            continue;
        };
        if table == "details_lookup_" && field == "default_value" {
            let Num::Float(v) = literal(value)? else {
                return Err(format!("the default of `{key}` is not a decimal number").into());
            };
            found.push((key.clone(), v));
        }
    }
    if found.is_empty() {
        return Err("the constructor sets no default overrides. Update xtask/src/vital.rs".into());
    }
    Ok(found)
}

/// What a control holds.
#[derive(Debug, PartialEq)]
enum Kind {
    Flag,
    Choice(Vec<String>),
    Count,
    Float,
}

/// A whole, non-negative `f64` as an index.
fn whole(x: f64) -> Option<usize> {
    (x >= 0.0 && x.fract().abs() < 1e-9)
        .then(|| x.to_string().parse().ok())
        .flatten()
}

fn kind(t: &Template) -> crate::Result<Kind> {
    if t.scale != Scale::Indexed {
        return Ok(Kind::Float);
    }
    let on_off = t.labels.as_ref().is_none_or(|l| l == &["Off", "On"]);
    if t.min.abs() < 1e-9 && (t.max - 1.0).abs() < 1e-9 && on_off {
        return Ok(Kind::Flag);
    }
    let Some(labels) = &t.labels else {
        return Ok(Kind::Count);
    };
    let (Some(min), Some(max)) = (whole(t.min), whole(t.max)) else {
        return Err(format!(
            "`{}` has labels but its range {}..{} is not whole and non-negative",
            t.name, t.min, t.max
        )
        .into());
    };
    if labels.len() > max {
        Ok(Kind::Choice(labels[min..=max].to_vec()))
    } else {
        Ok(Kind::Count)
    }
}

/// The physical unit of a control.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Unit {
    Hz,
    Decibels,
    Cents,
    Semitones,
    Seconds,
    PercentUnipolar,
    PercentBipolar,
}

impl Unit {
    fn name(self) -> &'static str {
        match self {
            Self::Hz => "hz",
            Self::Decibels => "decibels",
            Self::Cents => "cents",
            Self::Semitones => "semitones",
            Self::Seconds => "seconds",
            Self::PercentUnipolar => "percent_unipolar",
            Self::PercentBipolar => "percent_bipolar",
        }
    }
}

/// The map from the stored value to the physical one.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Law {
    Affine {
        scale: f64,
        offset: f64,
    },
    Power {
        exponent: f64,
        scale: f64,
        offset: f64,
    },
    Exp2 {
        rate: f64,
        scale: f64,
    },
    DbOfPower {
        exponent: f64,
    },
}

impl Law {
    fn to_json(self) -> Value {
        match self {
            Self::Affine { scale, offset } => json!({"affine": {"scale": scale, "offset": offset}}),
            Self::Power {
                exponent,
                scale,
                offset,
            } => json!({"power": {"exponent": exponent, "scale": scale, "offset": offset}}),
            Self::Exp2 { rate, scale } => json!({"exp2": {"rate": rate, "scale": scale}}),
            Self::DbOfPower { exponent } => json!({"db_of_power": {"exponent": exponent}}),
        }
    }
}

/// Controls whose name decides the unit and the law.
#[derive(Clone, Copy)]
enum Named {
    NoteCutoff,
    Level,
    Tune,
    Transpose,
    EqResonance,
}

impl Named {
    fn unit_law(self) -> (Unit, Law) {
        match self {
            Self::NoteCutoff => (
                Unit::Hz,
                Law::Exp2 {
                    rate: 1.0 / 12.0,
                    scale: 8.175_798_915_6,
                },
            ),
            Self::Level => (Unit::Decibels, Law::DbOfPower { exponent: 2.0 }),
            Self::Tune => (
                Unit::Cents,
                Law::Affine {
                    scale: 100.0,
                    offset: 0.0,
                },
            ),
            Self::Transpose => (
                Unit::Semitones,
                Law::Affine {
                    scale: 1.0,
                    offset: 0.0,
                },
            ),
            Self::EqResonance => (
                Unit::PercentUnipolar,
                Law::Power {
                    exponent: 2.0,
                    scale: 100.0,
                    offset: 0.0,
                },
            ),
        }
    }
}

/// Name rules as (group prefix, control name, rule). An empty prefix means a
/// global control.
const NAMED: &[(&str, &str, Named)] = &[
    ("filter", "cutoff", Named::NoteCutoff),
    ("", "chorus_cutoff", Named::NoteCutoff),
    ("", "delay_filter_cutoff", Named::NoteCutoff),
    ("", "distortion_filter_cutoff", Named::NoteCutoff),
    ("", "eq_low_cutoff", Named::NoteCutoff),
    ("", "eq_band_cutoff", Named::NoteCutoff),
    ("", "eq_high_cutoff", Named::NoteCutoff),
    ("", "phaser_center", Named::NoteCutoff),
    ("", "flanger_center", Named::NoteCutoff),
    ("", "reverb_high_shelf_cutoff", Named::NoteCutoff),
    ("", "reverb_low_shelf_cutoff", Named::NoteCutoff),
    ("", "reverb_pre_high_cutoff", Named::NoteCutoff),
    ("", "reverb_pre_low_cutoff", Named::NoteCutoff),
    ("osc", "level", Named::Level),
    ("", "sample_level", Named::Level),
    ("", "sub_level", Named::Level),
    ("osc", "tune", Named::Tune),
    ("lfo", "keytrack_tune", Named::Tune),
    ("random", "keytrack_tune", Named::Tune),
    ("", "sample_tune", Named::Tune),
    ("", "sub_tune", Named::Tune),
    ("osc", "transpose", Named::Transpose),
    ("lfo", "keytrack_transpose", Named::Transpose),
    ("random", "keytrack_transpose", Named::Transpose),
    ("", "sample_transpose", Named::Transpose),
    ("", "sub_transpose", Named::Transpose),
    ("", "voice_transpose", Named::Transpose),
    ("", "eq_low_resonance", Named::EqResonance),
    ("", "eq_band_resonance", Named::EqResonance),
    ("", "eq_high_resonance", Named::EqResonance),
];

/// Legacy controls as (group prefix, name, instances). No instances means
/// every instance.
const LEGACY: &[(&str, &str, &[&str])] = &[
    ("", "sub_direct_out", &[]),
    ("", "sub_level", &[]),
    ("", "sub_on", &[]),
    ("", "sub_pan", &[]),
    ("", "sub_transpose", &[]),
    ("", "sub_transpose_quantize", &[]),
    ("", "sub_tune", &[]),
    ("", "sub_waveform", &[]),
    ("", "compressor_low_band_unused", &[]),
    ("filter", "osc1_input", &[]),
    ("filter", "osc2_input", &[]),
    ("filter", "osc3_input", &[]),
    ("filter", "sample_input", &[]),
    ("filter", "filter_input", &["fx"]),
];

/// The unit and law of a control, or none.
fn unit_law(prefix: &str, t: &Template) -> crate::Result<Option<(Unit, Law)>> {
    if let Some((_, _, rule)) = NAMED.iter().find(|(p, n, _)| *p == prefix && *n == t.name) {
        return Ok(Some(rule.unit_law()));
    }
    let (m, o) = (t.multiplier, t.offset);
    let units = t.units.trim();
    let unsupported = || -> crate::Result<_> {
        Err(format!(
            "control `{}` in group `{prefix}` has unit `{units}`, scale {:?}, invert {}: \
             no rule covers it. Add a rule to xtask/src/vital.rs",
            t.name, t.scale, t.invert
        )
        .into())
    };
    if t.invert && !(units == "secs" && t.scale == Scale::Exponential) {
        return unsupported();
    }
    let percent = if t.min < 0.0 {
        Unit::PercentBipolar
    } else {
        Unit::PercentUnipolar
    };
    let affine = Law::Affine {
        scale: m,
        offset: o,
    };
    let power = |exponent| Law::Power {
        exponent,
        scale: m,
        offset: o,
    };
    let exp2 = |scale| Law::Exp2 { rate: 1.0, scale };
    Ok(match (units, t.scale) {
        ("%", Scale::Linear) => Some((percent, affine)),
        ("%", Scale::Quadratic) => Some((percent, power(2.0))),
        ("dB", Scale::Linear) => Some((Unit::Decibels, affine)),
        ("dB", Scale::SquareRoot) => Some((Unit::Decibels, power(0.5))),
        ("secs", Scale::Quartic) => Some((Unit::Seconds, power(4.0))),
        ("secs", Scale::Linear) => Some((Unit::Seconds, affine)),
        ("secs", Scale::Exponential) if t.invert => Some((Unit::Hz, exp2(m))),
        ("secs", Scale::Exponential) => Some((Unit::Seconds, exp2(m))),
        ("ms", Scale::Exponential) => Some((Unit::Seconds, exp2(m / 1000.0))),
        ("Hz", Scale::Exponential) => Some((Unit::Hz, exp2(m))),
        ("semitones", Scale::Linear | Scale::Indexed) => Some((Unit::Semitones, affine)),
        ("cents", Scale::Linear) => Some((Unit::Cents, affine)),
        ("" | "v" | "voices", Scale::Indexed | Scale::Linear) => None,
        _ => return unsupported(),
    })
}

/// The `legacy` value of a control, if any, and how many keys it marks.
fn legacy(prefix: &str, name: &str, ids: &[String]) -> crate::Result<Option<(Value, usize)>> {
    let Some((_, _, only)) = LEGACY.iter().find(|(p, n, _)| *p == prefix && *n == name) else {
        return Ok(None);
    };
    if only.is_empty() {
        return Ok(Some((json!(true), ids.len())));
    }
    if let Some(missing) = only.iter().find(|id| !ids.iter().any(|i| i == *id)) {
        return Err(format!(
            "legacy control `{name}` names instance `{missing}`, which is missing"
        )
        .into());
    }
    Ok(Some((json!(only), only.len())))
}

/// Running totals while the registry is built.
#[derive(Default)]
struct Tally {
    /// Key to (min, max, default).
    keys: BTreeMap<String, (f64, f64, f64)>,
    legacy: usize,
    named_used: BTreeSet<(String, String)>,
}

fn control(group: &Group, t: &Template, tally: &mut Tally) -> crate::Result<Value> {
    let mut c = Map::new();
    c.insert("name".into(), json!(t.name));
    c.insert("label".into(), json!(t.label));
    c.insert("since".into(), json!(version_text(t.version)));
    let (kind_name, choices) = match kind(t)? {
        Kind::Flag => ("flag", None),
        Kind::Choice(choices) => ("choice", Some(choices)),
        Kind::Count => ("count", None),
        Kind::Float => ("float", None),
    };
    c.insert("kind".into(), json!(kind_name));
    c.insert("min".into(), json!(t.min));
    c.insert("max".into(), json!(t.max));
    c.insert("default".into(), json!(t.default));
    c.insert("scale".into(), json!(t.scale.name()));
    if let Some(choices) = choices {
        c.insert("choices".into(), json!(choices));
    }
    if let Some((unit, law)) = unit_law(&group.prefix, t)? {
        c.insert("unit".into(), json!(unit.name()));
        c.insert("law".into(), law.to_json());
        if NAMED
            .iter()
            .any(|(p, n, _)| *p == group.prefix && *n == t.name)
        {
            tally
                .named_used
                .insert((group.prefix.clone(), t.name.clone()));
        }
    }
    if let Some((value, count)) = legacy(&group.prefix, &t.name, &group.ids)? {
        c.insert("legacy".into(), value);
        tally.legacy += count;
    }
    Ok(Value::Object(c))
}

/// The registry document for parsed tables.
fn build(parsed: &Parsed, commit: &str, expect: &Expect) -> crate::Result<Value> {
    let mut tally = Tally::default();
    let mut groups = Vec::new();
    for group in &parsed.groups {
        let mut templates = group.templates.clone();
        templates.sort_by(|a, b| a.name.cmp(&b.name));
        let mut controls = Vec::new();
        for t in &templates {
            controls.push(control(group, t, &mut tally)?);
            for id in &group.ids {
                let key = if group.prefix.is_empty() {
                    t.name.clone()
                } else {
                    format!("{}_{id}_{}", group.prefix, t.name)
                };
                if tally
                    .keys
                    .insert(key.clone(), (t.min, t.max, t.default))
                    .is_some()
                {
                    return Err(format!("the control key `{key}` appears twice").into());
                }
            }
        }
        let mut entry = Map::new();
        entry.insert("prefix".into(), json!(group.prefix));
        entry.insert("label".into(), json!(group.label));
        entry.insert("ids".into(), json!(group.ids));
        if let Some((ids, since)) = &group.later {
            entry.insert(
                "later".into(),
                json!({"ids": ids, "since": version_text(*since)}),
            );
        }
        entry.insert("controls".into(), Value::Array(controls));
        groups.push(Value::Object(entry));
    }
    let mut defaults = BTreeMap::new();
    for (key, value) in &parsed.overrides {
        let slot = tally
            .keys
            .get_mut(key)
            .ok_or_else(|| format!("the constructor overrides `{key}`, which no group creates"))?;
        slot.2 = *value;
        defaults.insert(key.clone(), *value);
    }
    check_totals(&tally, expect)?;
    Ok(json!({
        "source": format!("Vital open source, commit {commit}"),
        "groups": groups,
        "defaults": defaults,
    }))
}

/// Fails when the totals or the default ranges are not what the rules expect.
fn check_totals(tally: &Tally, expect: &Expect) -> crate::Result<()> {
    if tally.keys.len() != expect.keys {
        return Err(format!(
            "expected {} controls, found {}. The checkout is not the pinned Vital commit",
            expect.keys,
            tally.keys.len()
        )
        .into());
    }
    if tally.legacy != expect.legacy {
        return Err(format!(
            "expected {} legacy controls, found {}. Update the legacy list in xtask/src/vital.rs",
            expect.legacy, tally.legacy
        )
        .into());
    }
    let outside: Vec<_> = tally
        .keys
        .iter()
        .filter(|(_, (min, max, default))| !(min <= default && default <= max))
        .map(|(key, _)| key.as_str())
        .take(5)
        .collect();
    if !outside.is_empty() {
        return Err(format!(
            "the default lies outside min..max for: {}",
            outside.join(", ")
        )
        .into());
    }
    if expect.all_rules_used {
        let unused: Vec<_> = NAMED
            .iter()
            .filter(|(p, n, _)| {
                !tally
                    .named_used
                    .contains(&((*p).to_owned(), (*n).to_owned()))
            })
            .map(|(p, n, _)| format!("{p}/{n}"))
            .collect();
        if !unused.is_empty() {
            return Err(format!(
                "these name rules match no control: {}. Rename them in xtask/src/vital.rs",
                unused.join(", ")
            )
            .into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(text: &str) -> Vec<Tok> {
        tokenize(&strip_comments(text))
    }

    fn symbols(header: &str) -> Symbols {
        let mut s = Symbols::default();
        s.scan(&toks(header));
        s
    }

    fn eval(s: &Symbols, expr: &str) -> crate::Result<Num> {
        s.eval(&toks(expr))
    }

    fn tpl(name: &str, units: &str, scale: Scale, min: f64, max: f64) -> Template {
        Template {
            name: name.into(),
            version: 0,
            min,
            max,
            default: min,
            offset: 0.0,
            multiplier: 1.0,
            scale,
            invert: false,
            units: units.into(),
            label: name.into(),
            labels: None,
        }
    }

    #[test]
    fn comments_go_but_strings_stay() {
        let text = "a // gone\nb /* gone\n too */ c \"// kept\" d '\"' e";
        let clean = strip_comments(text);
        assert_eq!(clean, "a \nb \n  c \"// kept\" d '\"' e");
    }

    #[test]
    fn tokens_cover_numbers_strings_and_preprocessor_lines() {
        let t = toks("#define X { 1\nint a = 0x10 + 1.5f * 2e-3; \"q\\\"s\" -4");
        assert_eq!(
            t[2..8],
            [
                Tok::Punct('='),
                Tok::Num("0x10".into()),
                Tok::Punct('+'),
                Tok::Num("1.5f".into()),
                Tok::Punct('*'),
                Tok::Num("2e-3".into()),
            ]
        );
        assert_eq!(t[9], Tok::Str("q\"s".into()));
        assert_eq!(t[t.len() - 1], Tok::Num("4".into()));
        assert!(!t.contains(&Tok::Ident("define".into())));
    }

    const HEADER: &str = "
        namespace ns {
          constexpr int kTop = 4;
          enum { kFlat = 7, kNext };
          struct Alpha {
            enum Kind { kOne, kTwo = 5, kThree, kCount };
            static constexpr int kTwice = 2 * kTwo;
          };
          class Beta : public Alpha {
            enum class Mode : int { kA = -2, kB, kCount };
            static constexpr float kLimit = 1.5f;
            constexpr int helper(int v) { return v; }
          };
          enum Mixed { kBase = kTop + 1, kLost };
        }";

    #[test]
    fn enumerators_and_constants_resolve_by_scope() {
        let s = symbols(HEADER);
        let int = |name: &str| eval(&s, name).unwrap();
        assert_eq!(int("Alpha::kThree"), Num::Int(6));
        assert_eq!(int("Alpha::kCount"), Num::Int(7));
        assert_eq!(int("Beta::kCount"), Num::Int(0));
        assert_eq!(int("kB"), Num::Int(-1));
        assert_eq!(int("Alpha::kTwice"), Num::Int(10));
        assert_eq!(int("ns::kTop"), Num::Int(4));
        assert_eq!(int("kNext"), Num::Int(8));
        assert_eq!(int("Beta::kLimit"), Num::Float(1.5));
    }

    #[test]
    fn unknown_ambiguous_and_unresolved_names_fail() {
        let s = symbols(HEADER);
        for bad in ["kCount", "kMissing", "kLost", "helper"] {
            let err = eval(&s, bad).unwrap_err().to_string();
            assert!(err.contains(bad), "{err}");
        }
        let twice = symbols("enum A { kX }; enum B { kX };");
        assert!(eval(&twice, "kX").is_err());
    }

    #[test]
    fn expressions_follow_cpp_arithmetic() {
        let s = symbols("");
        let cases = [
            ("1 + 2 * 3", Num::Int(7)),
            ("(1 + 2) * 3", Num::Int(9)),
            ("-4 + 1", Num::Int(-3)),
            ("7 / 2", Num::Int(3)),
            ("-7 / 2", Num::Int(-3)),
            ("7 / 2.0", Num::Float(3.5)),
            ("8.0f - 1", Num::Float(7.0)),
            ("vital::utils::factorial(4) - 1", Num::Int(23)),
            ("0x000507", Num::Int(0x507)),
        ];
        for (text, want) in cases {
            assert_eq!(eval(&s, text).unwrap(), want, "{text}");
        }
        for bad in [
            "1 / 0",
            "1 +",
            "(1",
            "2 << 1",
            "sqrt(4)",
            "factorial(2.0)",
            "1 2",
        ] {
            assert!(eval(&s, bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn versions_print_as_three_numbers() {
        assert_eq!(version_text(0x0000_0507), "0.5.7");
        assert_eq!(version_text(0x0002_0304), "2.3.4");
        assert_eq!(hex_version("0x000601").unwrap(), 0x601);
        assert!(hex_version("601").is_err());
        assert!(hex_version("0x1000000").is_err());
    }

    const TABLE: &str = r#"
        static const std::string kEnvIdPrefix = "env";
        const ValueDetails ValueDetailsLookup::parameter_list[] = {
          { "gain", 0x000507, 0.0, vital::utils::factorial(3) - 1, 0.25f, -1.0, 100.0,
            ValueDetails::kLinear, false, " dB", "Gain", nullptr },  // trailing note
          { "mode", 0x000000, 0.0, kLast - 1.0, 0.0, 0.0, 1.0,
            ValueDetails::kIndexed, true, "", "Mode", strings::kModeNames },
        };
        ValueDetailsLookup::ValueDetailsLookup() {
          kOld = 2;
          details_lookup_["gain"].default_value = 1.5f;
        }"#;

    #[test]
    fn a_twelve_field_row_reads_every_field() {
        let s = symbols("enum { kFirst, kLast = 4 };");
        let strings = string_arrays(&toks(
            r#"const std::string kModeNames[3 + 1] = { "A", "B\\", "C", };"#,
        ))
        .unwrap();
        assert_eq!(strings["kModeNames"], ["A", "B\\", "C"]);
        let rows = parse_list(&toks(TABLE), "parameter_list", &s, &strings).unwrap();
        let [gain, mode] = rows.as_slice() else {
            panic!("two rows expected, got {rows:?}");
        };
        assert_eq!(gain.name, "gain");
        assert_eq!(version_text(gain.version), "0.5.7");
        assert_eq!((gain.min, gain.max, gain.default), (0.0, 5.0, 0.25));
        assert_eq!((gain.offset, gain.multiplier), (-1.0, 100.0));
        assert_eq!((gain.scale, gain.invert), (Scale::Linear, false));
        assert_eq!((gain.units.as_str(), gain.label.as_str()), (" dB", "Gain"));
        assert!(gain.labels.is_none());
        assert_eq!(mode.max, 3.0);
        assert!(mode.invert);
        assert_eq!(
            mode.labels.as_deref(),
            Some(&["A".into(), "B\\".into(), "C".into()][..])
        );
        assert_eq!(string_const(&toks(TABLE), "kEnvIdPrefix").unwrap(), "env");
    }

    #[test]
    fn a_row_with_the_wrong_field_count_or_scale_fails() {
        let s = symbols("");
        let none = BTreeMap::new();
        let short = r#"const ValueDetails ValueDetailsLookup::l[] = { { "a", 0x0, 0.0, 1.0 }, };"#;
        let err = parse_list(&toks(short), "l", &s, &none)
            .unwrap_err()
            .to_string();
        assert!(err.contains("expected 12 fields, found 4"), "{err}");
        let odd = r#"const ValueDetails ValueDetailsLookup::l[] = {
            { "a", 0x0, 0.0, 1.0, 0.0, 0.0, 1.0, ValueDetails::kWeird, false, "", "A", nullptr } };"#;
        let err = parse_list(&toks(odd), "l", &s, &none)
            .unwrap_err()
            .to_string();
        assert!(err.contains("kWeird"), "{err}");
        assert!(parse_list(&toks(odd), "missing", &s, &none).is_err());
    }

    #[test]
    fn the_constructor_gives_counts_and_default_overrides() {
        let t = toks(TABLE);
        assert_eq!(ctor_number(&t, "kOld").unwrap(), "2");
        assert!(ctor_number(&t, "kNone").is_err());
        assert_eq!(default_overrides(&t).unwrap(), [("gain".to_owned(), 1.5)]);
    }

    #[test]
    fn kinds_follow_scale_range_and_labels() {
        let labelled = |min, max, labels: &[&str]| {
            let mut t = tpl("x", "", Scale::Indexed, min, max);
            t.labels = Some(labels.iter().map(|s| (*s).to_owned()).collect());
            t
        };
        assert_eq!(
            kind(&tpl("a", "", Scale::Indexed, 0.0, 1.0)).unwrap(),
            Kind::Flag
        );
        assert_eq!(
            kind(&labelled(0.0, 1.0, &["Off", "On"])).unwrap(),
            Kind::Flag
        );
        assert_eq!(
            kind(&labelled(0.0, 1.0, &["Low", "High"])).unwrap(),
            Kind::Choice(vec!["Low".into(), "High".into()])
        );
        assert_eq!(
            kind(&labelled(1.0, 2.0, &["A", "B", "C"])).unwrap(),
            Kind::Choice(vec!["B".into(), "C".into()])
        );
        assert_eq!(
            kind(&labelled(0.0, 2.0, &["Off", "On"])).unwrap(),
            Kind::Count
        );
        assert_eq!(
            kind(&tpl("a", "", Scale::Indexed, -48.0, 48.0)).unwrap(),
            Kind::Count
        );
        assert_eq!(
            kind(&tpl("a", "", Scale::Linear, 0.0, 1.0)).unwrap(),
            Kind::Float
        );
        assert!(kind(&labelled(0.5, 2.0, &["A", "B", "C"])).is_err());
    }

    fn rule(
        prefix: &str,
        name: &str,
        units: &str,
        scale: Scale,
        m: f64,
        o: f64,
    ) -> Option<(Unit, Law)> {
        let mut t = tpl(name, units, scale, 0.0, 1.0);
        (t.multiplier, t.offset) = (m, o);
        unit_law(prefix, &t).unwrap()
    }

    #[test]
    fn generic_rules_pick_unit_and_law_from_the_display() {
        let affine = |scale, offset| Law::Affine { scale, offset };
        let power = |exponent, scale, offset| Law::Power {
            exponent,
            scale,
            offset,
        };
        let exp2 = |rate, scale| Law::Exp2 { rate, scale };
        let g = |units, scale, m, o| rule("env", "x", units, scale, m, o);
        assert_eq!(
            g(" %", Scale::Linear, 100.0, 0.0),
            g("%", Scale::Linear, 100.0, 0.0)
        );
        assert_eq!(
            g("%", Scale::Linear, 100.0, 0.0),
            Some((Unit::PercentUnipolar, affine(100.0, 0.0)))
        );
        assert_eq!(
            g("%", Scale::Quadratic, 1.0, 0.0),
            Some((Unit::PercentUnipolar, power(2.0, 1.0, 0.0)))
        );
        assert_eq!(
            g(" dB", Scale::Linear, 1.0, 0.0),
            Some((Unit::Decibels, affine(1.0, 0.0)))
        );
        assert_eq!(
            g("dB", Scale::SquareRoot, 1.0, -80.0),
            Some((Unit::Decibels, power(0.5, 1.0, -80.0)))
        );
        assert_eq!(
            g(" secs", Scale::Quartic, 1.0, 0.0),
            Some((Unit::Seconds, power(4.0, 1.0, 0.0)))
        );
        assert_eq!(
            g(" secs", Scale::Linear, 1.0, 0.0),
            Some((Unit::Seconds, affine(1.0, 0.0)))
        );
        assert_eq!(
            g(" secs", Scale::Exponential, 1.0, 0.0),
            Some((Unit::Seconds, exp2(1.0, 1.0)))
        );
        assert_eq!(
            g("ms", Scale::Exponential, 1000.0, 0.0),
            Some((Unit::Seconds, exp2(1.0, 1.0)))
        );
        assert_eq!(
            g(" Hz", Scale::Exponential, 1.0, 0.0),
            Some((Unit::Hz, exp2(1.0, 1.0)))
        );
        assert_eq!(
            g(" semitones", Scale::Indexed, 1.0, 0.0),
            Some((Unit::Semitones, affine(1.0, 0.0)))
        );
        assert_eq!(
            g(" cents", Scale::Linear, 100.0, 0.0),
            Some((Unit::Cents, affine(100.0, 0.0)))
        );
        for none in ["", "v", " voices"] {
            assert_eq!(g(none, Scale::Indexed, 1.0, 0.0), None, "{none}");
        }
    }

    #[test]
    fn inverted_seconds_are_a_frequency_and_percent_can_be_bipolar() {
        let mut t = tpl("rate", " secs", Scale::Exponential, -7.0, 9.0);
        t.invert = true;
        assert_eq!(
            unit_law("lfo", &t).unwrap(),
            Some((
                Unit::Hz,
                Law::Exp2 {
                    rate: 1.0,
                    scale: 1.0
                }
            ))
        );
        let pan = tpl("pan", "%", Scale::Linear, -1.0, 1.0);
        assert_eq!(
            unit_law("", &pan).unwrap().map(|(u, _)| u),
            Some(Unit::PercentBipolar)
        );
    }

    #[test]
    fn name_rules_beat_the_display_unit() {
        let hz = Some((
            Unit::Hz,
            Law::Exp2 {
                rate: 1.0 / 12.0,
                scale: 8.175_798_915_6,
            },
        ));
        assert_eq!(
            rule("filter", "cutoff", " semitones", Scale::Linear, 1.0, 0.0),
            hz
        );
        assert_eq!(
            rule(
                "",
                "reverb_pre_low_cutoff",
                " semitones",
                Scale::Linear,
                1.0,
                0.0
            ),
            hz
        );
        assert_eq!(
            rule("osc", "level", "", Scale::Quadratic, 1.0, 0.0),
            Some((Unit::Decibels, Law::DbOfPower { exponent: 2.0 }))
        );
        let affine = |scale| Law::Affine { scale, offset: 0.0 };
        assert_eq!(
            rule("osc", "tune", "", Scale::Linear, 100.0, 0.0),
            Some((Unit::Cents, affine(100.0)))
        );
        assert_eq!(
            rule("random", "keytrack_tune", "", Scale::Linear, 7.0, 0.0),
            Some((Unit::Cents, affine(100.0)))
        );
        assert_eq!(
            rule("osc", "transpose", "", Scale::Indexed, 1.0, 0.0),
            Some((Unit::Semitones, affine(1.0)))
        );
        assert_eq!(
            rule("", "voice_transpose", "", Scale::Indexed, 1.0, 0.0),
            Some((Unit::Semitones, affine(1.0)))
        );
        assert_eq!(
            rule("", "eq_band_resonance", "", Scale::Quadratic, 100.0, 0.0),
            Some((
                Unit::PercentUnipolar,
                Law::Power {
                    exponent: 2.0,
                    scale: 100.0,
                    offset: 0.0
                }
            ))
        );
        assert_eq!(rule("env", "tune", "", Scale::Linear, 1.0, 0.0), None);
    }

    #[test]
    fn an_unknown_unit_or_scale_names_the_control() {
        for (units, scale, invert) in [
            ("furlongs", Scale::Linear, false),
            ("%", Scale::Indexed, false),
            ("", Scale::Quadratic, false),
            (" secs", Scale::Cubic, false),
            ("", Scale::Linear, true),
        ] {
            let mut t = tpl("odd", units, scale, 0.0, 1.0);
            t.invert = invert;
            let err = unit_law("filter", &t).unwrap_err().to_string();
            assert!(err.contains("`odd`") && err.contains("filter"), "{err}");
        }
    }

    #[test]
    fn legacy_marks_all_instances_or_only_the_named_ones() {
        let ids: Vec<String> = ["1", "2", "fx"].map(str::to_owned).to_vec();
        assert_eq!(
            legacy("filter", "osc1_input", &ids).unwrap(),
            Some((json!(true), 3))
        );
        assert_eq!(
            legacy("filter", "filter_input", &ids).unwrap(),
            Some((json!(["fx"]), 1))
        );
        assert_eq!(
            legacy("", "sub_on", &[String::new()]).unwrap(),
            Some((json!(true), 1))
        );
        assert_eq!(legacy("osc", "osc1_input", &ids).unwrap(), None);
        assert!(legacy("filter", "filter_input", &ids[..2]).is_err());
    }

    fn mini(default: f64) -> Parsed {
        let mut gain = tpl("gain", " dB", Scale::Linear, 0.0, 1.0);
        gain.default = default;
        let group = |prefix: &str, ids: &[&str], templates| Group {
            prefix: prefix.into(),
            label: prefix.to_uppercase(),
            ids: ids.iter().map(|s| (*s).to_owned()).collect(),
            later: None,
            templates,
        };
        let mut mix = group(
            "filter",
            &["1", "fx"],
            vec![tpl("filter_input", "", Scale::Indexed, 0.0, 1.0)],
        );
        mix.later = Some((vec!["fx".into()], 0x600));
        Parsed {
            groups: vec![group("", &[""], vec![gain]), mix],
            overrides: vec![("filter_1_filter_input".into(), 1.0)],
        }
    }

    const MINI: Expect = Expect {
        keys: 3,
        legacy: 1,
        all_rules_used: false,
    };

    #[test]
    fn the_document_has_sorted_controls_defaults_and_instance_markers() {
        let doc = build(&mini(0.5), "abc", &MINI).unwrap();
        assert_eq!(doc["source"], "Vital open source, commit abc");
        assert_eq!(doc["defaults"], json!({"filter_1_filter_input": 1.0}));
        let filter = &doc["groups"][1];
        assert_eq!(filter["later"], json!({"ids": ["fx"], "since": "0.6.0"}));
        assert_eq!(filter["controls"][0]["legacy"], json!(["fx"]));
        assert_eq!(filter["controls"][0]["kind"], "flag");
        assert!(doc["groups"][0]["controls"][0].get("legacy").is_none());
        assert_eq!(doc["groups"][0]["controls"][0]["unit"], "decibels");
    }

    #[test]
    fn totals_and_default_ranges_are_asserted() {
        let wrong_keys = Expect { keys: 794, ..MINI };
        let err = build(&mini(0.5), "abc", &wrong_keys)
            .unwrap_err()
            .to_string();
        assert!(err.contains("expected 794 controls, found 3"), "{err}");
        let wrong_legacy = Expect { legacy: 22, ..MINI };
        let err = build(&mini(0.5), "abc", &wrong_legacy)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("expected 22 legacy controls, found 1"),
            "{err}"
        );
        let err = build(&mini(2.0), "abc", &MINI).unwrap_err().to_string();
        assert!(err.contains("outside min..max for: gain"), "{err}");
        let mut stray = mini(0.5);
        stray.overrides.push(("nowhere".into(), 0.0));
        assert!(build(&stray, "abc", &MINI).is_err());
        let strict = Expect {
            all_rules_used: true,
            ..MINI
        };
        let err = build(&mini(0.5), "abc", &strict).unwrap_err().to_string();
        assert!(err.contains("filter/cutoff"), "{err}");
    }

    #[test]
    fn the_commit_comes_from_head_or_the_ref_it_names() {
        let dir = tempfile::tempdir().unwrap();
        let git = dir.path().join(".git");
        fs::create_dir_all(git.join("refs/heads")).unwrap();
        let hash = "636ca0ef517a4db087a6a08a6a8a5e704e21f836";
        fs::write(git.join("HEAD"), format!("{hash}\n")).unwrap();
        assert_eq!(commit(dir.path()).unwrap(), hash);
        fs::write(git.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        assert!(commit(dir.path()).is_err());
        fs::write(
            git.join("packed-refs"),
            format!("# pack\n{hash} refs/heads/main\n"),
        )
        .unwrap();
        assert_eq!(commit(dir.path()).unwrap(), hash);
        fs::write(git.join("refs/heads/main"), "not a hash\n").unwrap();
        assert!(commit(dir.path()).is_err());
    }

    #[test]
    #[ignore = "needs a Vital source checkout in VITAL_SOURCE"]
    fn the_committed_registry_matches_the_checkout() {
        let source = std::env::var("VITAL_SOURCE").unwrap_or_else(|_| {
            panic!(
                "set VITAL_SOURCE to a checkout of https://github.com/mtytel/vital \
                 at the pinned commit, for example from `git clone`"
            )
        });
        let generated = render(Path::new(&source), &REAL).unwrap();
        let committed = fs::read_to_string(crate::root().join(OUTPUT)).unwrap();
        assert!(
            generated == committed,
            "{OUTPUT} is stale: run `cargo xtask vital-registry --source {source}`"
        );
    }
}
