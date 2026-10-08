use super::pretty::{Doc, Separator};
use num_bigint::BigInt;
#[derive(Debug, Clone, PartialEq)]
pub enum SyntaxError {
    Invalid(String),
    Unsupported(String),
    NativeBadMatchNil,
    Limit,
}
type Result<T> = std::result::Result<T, SyntaxError>;
#[derive(Clone, Debug, PartialEq)]
enum Token {
    Atom(String),
    QuotedAtom(String),
    Var(String),
    Number(String),
    Float(String),
    String(String),
    Char(char),
    Symbol(String),
}
impl Token {
    fn is(&self, s: &str) -> bool {
        matches!(self,Self::Symbol(x)|Self::Atom(x) if x==s)
    }
}
fn escape(cs: &[char], i: &mut usize) -> Result<char> {
    let c = *cs
        .get(*i)
        .ok_or_else(|| SyntaxError::Invalid("unterminated escape".into()))?;
    *i += 1;
    Ok(match c {
        'n' => '\n',
        'r' => '\r',
        't' => '\t',
        'v' => '\u{b}',
        'b' => '\u{8}',
        'f' => '\u{c}',
        'e' => '\u{1b}',
        's' => ' ',
        'd' => '\u{7f}',
        '^' => {
            let n = *cs
                .get(*i)
                .ok_or_else(|| SyntaxError::Invalid("control escape".into()))?;
            *i += 1;
            char::from_u32(n as u32 & 31)
                .ok_or_else(|| SyntaxError::Invalid("control escape".into()))?
        }
        'x' => {
            let mut text = String::new();
            if cs.get(*i) == Some(&'{') {
                *i += 1;
                while cs.get(*i).is_some_and(|&x| x != '}') {
                    text.push(cs[*i]);
                    *i += 1
                }
                if cs.get(*i) != Some(&'}') {
                    return Err(SyntaxError::Invalid("hex escape".into()));
                }
                *i += 1
            } else {
                for _ in 0..2 {
                    text.push(
                        *cs.get(*i)
                            .ok_or_else(|| SyntaxError::Invalid("hex escape".into()))?,
                    );
                    *i += 1
                }
            }
            char::from_u32(
                u32::from_str_radix(&text, 16)
                    .map_err(|_| SyntaxError::Invalid("hex escape".into()))?,
            )
            .ok_or_else(|| SyntaxError::Invalid("hex escape".into()))?
        }
        c if c.is_digit(8) => {
            let mut text = c.to_string();
            for _ in 0..2 {
                if cs.get(*i).is_some_and(|x| x.is_digit(8)) {
                    text.push(cs[*i]);
                    *i += 1
                } else {
                    break;
                }
            }
            char::from_u32(
                u32::from_str_radix(&text, 8)
                    .map_err(|_| SyntaxError::Invalid("octal escape".into()))?,
            )
            .ok_or_else(|| SyntaxError::Invalid("octal escape".into()))?
        }
        c => c,
    })
}
fn lex(src: &str) -> Result<Vec<Token>> {
    let cs: Vec<char> = src.chars().collect();
    if cs.len() > 1_000_000 {
        return Err(SyntaxError::Limit);
    }
    let mut i = 0;
    let mut out = Vec::new();
    while i < cs.len() {
        let c = cs[i];
        if c as u32 <= 32 || (128..=160).contains(&(c as u32)) {
            i += 1;
            continue;
        }
        if c == '%' {
            while i < cs.len() && cs[i] != '\n' {
                i += 1
            }
            continue;
        }
        if c == '\'' || c == '"' {
            let quote = c;
            i += 1;
            let mut value = String::new();
            while i < cs.len() && cs[i] != quote {
                if cs[i] == '\\' {
                    i += 1;
                    value.push(escape(&cs, &mut i)?)
                } else {
                    value.push(cs[i]);
                    i += 1
                }
            }
            if cs.get(i) != Some(&quote) {
                return Err(SyntaxError::Invalid("unterminated quote".into()));
            }
            i += 1;
            if quote == '\'' && value.chars().count() > 255 {
                return Err(SyntaxError::Invalid("atom length".into()));
            }
            let t = if quote == '\'' {
                Token::QuotedAtom(value)
            } else {
                Token::String(value)
            };
            if let Token::String(s) = &t
                && let Some(Token::String(previous)) = out.last_mut()
            {
                previous.push_str(s)
            } else {
                out.push(t)
            }
            continue;
        }
        if c == '$' {
            i += 1;
            let v = if cs.get(i) == Some(&'\\') {
                i += 1;
                escape(&cs, &mut i)?
            } else {
                let v = *cs
                    .get(i)
                    .ok_or_else(|| SyntaxError::Invalid("character".into()))?;
                i += 1;
                v
            };
            out.push(Token::Char(v));
            continue;
        }
        if c.is_ascii_digit() {
            let start = i;
            while cs.get(i).is_some_and(|x| x.is_ascii_digit() || *x == '_') {
                i += 1
            }
            let initial: String = cs[start..i].iter().filter(|&&x| x != '_').collect();
            if cs.get(i) == Some(&'#') {
                let base = initial
                    .parse::<u32>()
                    .map_err(|_| SyntaxError::Invalid("integer base".into()))?;
                if !(2..=36).contains(&base) {
                    return Err(SyntaxError::Invalid("integer base".into()));
                }
                i += 1;
                let start = i;
                while cs
                    .get(i)
                    .is_some_and(|x| x.is_ascii_alphanumeric() || *x == '_')
                {
                    i += 1
                }
                let digits: String = cs[start..i].iter().filter(|&&x| x != '_').collect();
                let n = BigInt::parse_bytes(digits.as_bytes(), base)
                    .ok_or_else(|| SyntaxError::Invalid("based integer".into()))?;
                out.push(Token::Number(n.to_string()));
                continue;
            }
            if cs.get(i) == Some(&'.') && cs.get(i + 1).is_some_and(char::is_ascii_digit) {
                i += 1;
                while cs.get(i).is_some_and(|x| x.is_ascii_digit() || *x == '_') {
                    i += 1
                }
                if cs.get(i).is_some_and(|x| *x == 'e' || *x == 'E') {
                    i += 1;
                    if cs.get(i).is_some_and(|x| *x == '+' || *x == '-') {
                        i += 1
                    }
                    let start = i;
                    while cs.get(i).is_some_and(|x| x.is_ascii_digit() || *x == '_') {
                        i += 1
                    }
                    if start == i {
                        return Err(SyntaxError::Invalid("float exponent".into()));
                    }
                }
                let text: String = cs[start..i].iter().filter(|&&x| x != '_').collect();
                let n = text
                    .parse::<f64>()
                    .map_err(|_| SyntaxError::Invalid("float".into()))?;
                if !n.is_finite() {
                    return Err(SyntaxError::Invalid("float overflow".into()));
                }
                let mut text = format!("{n:?}");
                if let Some(e) = text.find('e')
                    && !text[..e].contains('.')
                {
                    text.insert_str(e, ".0")
                }
                out.push(Token::Float(text));
            } else {
                out.push(Token::Number(initial))
            }
            continue;
        }
        if c.is_ascii_alphabetic()
            || c == '_'
            || ('ß'..='ÿ').contains(&c) && c != '÷'
            || ('À'..='Þ').contains(&c) && c != '×'
        {
            let start = i;
            i += 1;
            while cs.get(i).is_some_and(|x| {
                x.is_ascii_alphanumeric()
                    || *x == '_'
                    || *x == '@'
                    || ('ß'..='ÿ').contains(x) && *x != '÷'
                    || ('À'..='Þ').contains(x) && *x != '×'
            }) {
                i += 1
            }
            let value: String = cs[start..i].iter().collect();
            if value.chars().count() > 255 {
                return Err(SyntaxError::Invalid("atom length".into()));
            }
            out.push(if c.is_uppercase() || c == '_' {
                Token::Var(value)
            } else {
                Token::Atom(value)
            });
            continue;
        }
        let remaining: String = cs[i..].iter().take(4).collect();
        if let Some(op) = [
            "=:=", "=/=", "...", "->", "::", "..", "<<", ">>", "=>", ":=", "++", "--", "==", "/=",
            "=<", ">=", "||",
        ]
        .iter()
        .find(|s| remaining.starts_with(**s))
        {
            out.push(Token::Symbol((*op).into()));
            i += op.len()
        } else if "(){}[]#,:;|+-*/=<>!.".contains(c) {
            out.push(Token::Symbol(c.to_string()));
            i += 1
        } else {
            return Err(SyntaxError::Invalid(format!("invalid character {c}")));
        }
    }
    Ok(out)
}
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Type,
    Expression,
}
#[derive(Clone, Debug)]
enum Ast {
    Atom(String, bool),
    Var(String),
    Number(String),
    Float(String),
    String(String),
    Char(char),
    Nil,
    Tuple(Vec<Ast>),
    List(Vec<Ast>, Option<Box<Ast>>),
    ListType(Box<Ast>, bool),
    Call {
        module: Option<String>,
        name: String,
        args: Vec<Ast>,
    },
    Map(Vec<(Ast, String, Ast)>),
    Record(String, Vec<(String, String, Ast)>),
    Binary(Vec<BinarySegment>),
    BinaryType(Box<Ast>, Box<Ast>),
    Fun(Option<Vec<Ast>>, Box<Ast>),
    Unary(String, Box<Ast>),
    Op(String, Box<Ast>, Box<Ast>),
}
#[derive(Clone, Debug)]
struct BinarySegment {
    value: Ast,
    size: Option<Ast>,
    specifiers: Vec<BinarySpecifier>,
}
#[derive(Clone, Debug)]
struct BinarySpecifier {
    name: String,
    unit: Option<String>,
}
struct Parser<'a> {
    tokens: &'a [Token],
    position: usize,
    mode: Mode,
    depth: usize,
}
impl<'a> Parser<'a> {
    fn new(tokens: &'a [Token], mode: Mode) -> Self {
        Self {
            tokens,
            position: 0,
            mode,
            depth: 0,
        }
    }
    fn peek(&self, s: &str) -> bool {
        self.tokens.get(self.position).is_some_and(|t| t.is(s))
    }
    fn take(&mut self, s: &str) -> bool {
        if self.peek(s) {
            self.position += 1;
            true
        } else {
            false
        }
    }
    fn expect(&mut self, s: &str) -> Result<()> {
        if self.take(s) {
            Ok(())
        } else {
            Err(SyntaxError::Invalid(format!("expected {s}")))
        }
    }
    fn name(&mut self) -> Result<String> {
        match self.tokens.get(self.position) {
            Some(Token::Atom(a) | Token::QuotedAtom(a)) => {
                self.position += 1;
                Ok(a.clone())
            }
            _ => Err(SyntaxError::Invalid("expected atom".into())),
        }
    }
    fn expressions(&mut self, end: &str) -> Result<Vec<Ast>> {
        let mut out = Vec::new();
        if self.take(end) {
            return Ok(out);
        }
        loop {
            out.push(self.expr(0)?);
            if self.take(end) {
                break;
            }
            self.expect(",")?
        }
        Ok(out)
    }
    fn expr(&mut self, min: usize) -> Result<Ast> {
        self.depth += 1;
        if self.depth > 128 {
            return Err(SyntaxError::Limit);
        }
        let mut left = self.prefix()?;
        loop {
            let Some(op) = self.tokens.get(self.position).and_then(|t| match t {
                Token::Atom(x) | Token::Symbol(x) => Some(x.clone()),
                _ => None,
            }) else {
                break;
            };
            let Some((_, bp, r)) = prec(&op, self.mode) else {
                break;
            };
            if bp < min {
                break;
            }
            self.position += 1;
            let right = self.expr(if self.mode == Mode::Type && op == "|" {
                0
            } else {
                r
            })?;
            if self.mode == Mode::Type && op == "::" && !matches!(left, Ast::Var(_)) {
                return Err(SyntaxError::Invalid("annotation requires variable".into()));
            }
            if self.mode == Mode::Type && op == "=" {
                return Err(SyntaxError::Invalid("type match expression".into()));
            }
            if self.mode == Mode::Type
                && !["::", "|", "="].contains(&op.as_str())
                && (!type_expression_operand(&left) || !type_expression_operand(&right))
            {
                return Err(SyntaxError::Unsupported(
                    "structured type in expression pretty-printing".into(),
                ));
            }
            left = Ast::Op(op, Box::new(left), Box::new(right));
        }
        self.depth -= 1;
        Ok(left)
    }
    fn unary(&mut self, op: String) -> Result<Ast> {
        let value = self.expr(700)?;
        if self.mode == Mode::Type && !type_expression_operand(&value) {
            return Err(SyntaxError::Unsupported(
                "structured type in expression pretty-printing".into(),
            ));
        }
        Ok(Ast::Unary(op, Box::new(value)))
    }
    fn prefix(&mut self) -> Result<Ast> {
        let token = self
            .tokens
            .get(self.position)
            .cloned()
            .ok_or_else(|| SyntaxError::Invalid("missing expression".into()))?;
        self.position += 1;
        Ok(match token {
            Token::Atom(name) if ["not", "bnot", "catch"].contains(&name.as_str()) => {
                if self.mode == Mode::Type && name == "catch" {
                    return Err(SyntaxError::Invalid("type unary operator".into()));
                }
                self.unary(name)?
            }
            Token::Symbol(s) if s == "+" || s == "-" => self.unary(s)?,
            Token::Atom(name) => {
                if ["case", "if", "receive", "try", "begin", "maybe"].contains(&name.as_str()) {
                    return Err(SyntaxError::Unsupported(format!("expression form {name}")));
                }
                if name == "fun" && self.take("(") {
                    if self.take(")") {
                        Ast::Call {
                            module: None,
                            name,
                            args: vec![],
                        }
                    } else {
                        self.expect("(")?;
                        let args = if self.take("...") {
                            self.expect(")")?;
                            None
                        } else {
                            Some(self.expressions(")")?)
                        };
                        self.expect("->")?;
                        let ret = self.expr(0)?;
                        self.expect(")")?;
                        Ast::Fun(args, Box::new(ret))
                    }
                } else {
                    let (module, name) = if self.take(":") {
                        (Some(name), self.name()?)
                    } else {
                        (None, name)
                    };
                    if self.take("(") {
                        Ast::Call {
                            module,
                            name,
                            args: self.expressions(")")?,
                        }
                    } else if module.is_some() {
                        return Err(SyntaxError::Invalid(
                            "remote type needs argument list".into(),
                        ));
                    } else {
                        Ast::Atom(name, self.mode == Mode::Type)
                    }
                }
            }
            Token::QuotedAtom(name) => {
                let (module, name) = if self.take(":") {
                    (Some(name), self.name()?)
                } else {
                    (None, name)
                };
                if self.take("(") {
                    Ast::Call {
                        module,
                        name,
                        args: self.expressions(")")?,
                    }
                } else if module.is_some() {
                    return Err(SyntaxError::Invalid(
                        "remote type needs argument list".into(),
                    ));
                } else {
                    Ast::Atom(name, self.mode == Mode::Type)
                }
            }
            Token::Var(v) => Ast::Var(v),
            Token::Number(n) => Ast::Number(n),
            Token::Float(n) => {
                if self.mode == Mode::Type {
                    return Err(SyntaxError::Invalid("float type".into()));
                }
                Ast::Float(n)
            }
            Token::String(s) => {
                if self.mode == Mode::Type {
                    return Err(SyntaxError::Invalid("string type".into()));
                }
                Ast::String(s)
            }
            Token::Char(c) => Ast::Char(c),
            Token::Symbol(s) if s == "(" => {
                let e = self.expr(0)?;
                self.expect(")")?;
                e
            }
            Token::Symbol(s) if s == "{" => Ast::Tuple(self.expressions("}")?),
            Token::Symbol(s) if s == "[" => {
                if self.take("]") {
                    Ast::Nil
                } else {
                    let first = self.expr(0)?;
                    if self.mode == Mode::Type {
                        let nonempty = if self.take(",") {
                            self.expect("...")?;
                            true
                        } else {
                            false
                        };
                        self.expect("]")?;
                        Ast::ListType(Box::new(first), nonempty)
                    } else {
                        let mut items = vec![first];
                        while self.take(",") {
                            items.push(self.expr(0)?)
                        }
                        let tail = if self.take("|") {
                            Some(Box::new(self.expr(0)?))
                        } else {
                            None
                        };
                        self.expect("]")?;
                        Ast::List(items, tail)
                    }
                }
            }
            Token::Symbol(s) if s == "#" => {
                if self.take("{") {
                    let mut fields = Vec::new();
                    if !self.take("}") {
                        loop {
                            let key = self.expr(0)?;
                            let op = if self.take("=>") {
                                "=>"
                            } else {
                                self.expect(":=")?;
                                ":="
                            };
                            let value = self.expr(0)?;
                            fields.push((key, op.into(), value));
                            if self.take("}") {
                                break;
                            }
                            self.expect(",")?
                        }
                    }
                    Ast::Map(fields)
                } else {
                    let name = self.name()?;
                    self.expect("{")?;
                    let mut fields = Vec::new();
                    if !self.take("}") {
                        loop {
                            let field = self.name()?;
                            let op = if self.mode == Mode::Type {
                                self.expect("::")?;
                                "::"
                            } else {
                                self.expect("=")?;
                                "="
                            };
                            fields.push((field, op.into(), self.expr(0)?));
                            if self.take("}") {
                                break;
                            }
                            self.expect(",")?
                        }
                    }
                    Ast::Record(name, fields)
                }
            }
            Token::Symbol(s) if s == "<<" => {
                if self.mode == Mode::Type {
                    if self.take(">>") {
                        Ast::BinaryType(
                            Box::new(Ast::Number("0".into())),
                            Box::new(Ast::Number("0".into())),
                        )
                    } else {
                        let mut base = Ast::Number("0".into());
                        let mut unit = Ast::Number("0".into());
                        let mut count = 0;
                        let mut first_unit = false;
                        loop {
                            match self.tokens.get(self.position) {
                                Some(Token::Var(v)) if v == "_" => self.position += 1,
                                _ => return Err(SyntaxError::Invalid("binary type base".into())),
                            }
                            self.expect(":")?;
                            if let Some(Token::Var(v)) = self.tokens.get(self.position)
                                && v == "_"
                            {
                                self.position += 1;
                                self.expect("*")?;
                                if count > 1 {
                                    return Err(SyntaxError::Invalid("binary type fields".into()));
                                }
                                if count == 0 {
                                    first_unit = true
                                }
                                unit = self.expr(180)?
                            } else {
                                if count > 0 {
                                    return Err(SyntaxError::Invalid(
                                        "binary type field order".into(),
                                    ));
                                }
                                base = self.expr(180)?
                            }
                            count += 1;
                            if count > 2 || count > 1 && first_unit {
                                return Err(SyntaxError::Invalid("binary type field order".into()));
                            }
                            if self.take(">>") {
                                break;
                            }
                            self.expect(",")?
                        }
                        if !type_expression_operand(&base) || !type_expression_operand(&unit) {
                            return Err(SyntaxError::Unsupported(
                                "structured binary type size".into(),
                            ));
                        }
                        Ast::BinaryType(Box::new(base), Box::new(unit))
                    }
                } else {
                    let mut segments = Vec::new();
                    if !self.take(">>") {
                        loop {
                            let value = self.expr(900)?;
                            let size = if self.take(":") {
                                Some(self.expr(900)?)
                            } else {
                                None
                            };
                            let mut types = Vec::new();
                            if self.take("/") {
                                loop {
                                    let name = self.name()?;
                                    let value = if self.take(":") {
                                        match self.tokens.get(self.position).cloned() {
                                            Some(Token::Number(n)) => {
                                                self.position += 1;
                                                Some(n)
                                            }
                                            _ => {
                                                return Err(SyntaxError::Invalid(
                                                    "binary unit".into(),
                                                ));
                                            }
                                        }
                                    } else {
                                        None
                                    };
                                    types.push(BinarySpecifier { name, unit: value });
                                    if !self.take("-") {
                                        break;
                                    }
                                }
                            }
                            segments.push(BinarySegment {
                                value,
                                size,
                                specifiers: types,
                            });
                            if self.take(">>") {
                                break;
                            }
                            self.expect(",")?
                        }
                    }
                    Ast::Binary(segments)
                }
            }
            _ => return Err(SyntaxError::Invalid("unexpected token".into())),
        })
    }
}
fn type_expression_operand(e: &Ast) -> bool {
    match e {
        Ast::Atom(..) | Ast::Var(_) | Ast::Number(_) | Ast::Char(_) => true,
        Ast::Unary(_, e) => type_expression_operand(e),
        Ast::Op(op, l, r) if !["|", "::", ".."].contains(&op.as_str()) => {
            type_expression_operand(l) && type_expression_operand(r)
        }
        _ => false,
    }
}
fn prec(op: &str, mode: Mode) -> Option<(usize, usize, usize)> {
    Some(match op {
        "::" if mode == Mode::Type => (150, 150, 160),
        "|" if mode == Mode::Type => (180, 170, 170),
        ".." if mode == Mode::Type => (300, 200, 300),
        "=" | "!" => (150, 100, 100),
        "orelse" if mode == Mode::Expression => (160, 150, 150),
        "andalso" if mode == Mode::Expression => (200, 160, 160),
        "==" | "/=" | "=<" | "<" | ">=" | ">" | "=:=" | "=/=" if mode == Mode::Expression => {
            (300, 200, 300)
        }
        "++" | "--" if mode == Mode::Expression => (400, 300, 300),
        "+" | "-" | "bor" | "bxor" | "bsl" | "bsr" => (400, 400, 500),
        "or" | "xor" => (400, 400, 500),
        "*" | "/" | "div" | "rem" | "band" => (500, 500, 600),
        "and" => (500, 500, 600),
        _ => return None,
    })
}
fn parse_one(tokens: &[Token]) -> Result<Ast> {
    let mut binary_depth = 0usize;
    for (i, token) in tokens.iter().enumerate() {
        if token.is("<<") {
            binary_depth += 1;
        }
        if token.is(">>") {
            binary_depth = binary_depth.saturating_sub(1);
        }
        if token.is("#")
            && i > 0
            && matches!(
                &tokens[i - 1],
                Token::Var(_) | Token::Atom(_) | Token::QuotedAtom(_)
            )
        {
            return Err(SyntaxError::Unsupported(
                "map or record updates/access".into(),
            ));
        }
        if binary_depth == 0
            && matches!(token, Token::Var(_))
            && tokens.get(i + 1).is_some_and(|t| t.is(":"))
            && matches!(
                tokens.get(i + 2),
                Some(Token::Atom(_) | Token::QuotedAtom(_) | Token::Var(_))
            )
            && tokens.get(i + 3).is_some_and(|t| t.is("("))
        {
            return Err(SyntaxError::Unsupported("dynamic remote calls".into()));
        }

        if token.is("||") {
            return Err(SyntaxError::Unsupported("comprehensions".into()));
        }
        if matches!(token,Token::Atom(a) if a=="fun")
            && !tokens.get(i + 1).is_some_and(|t| t.is("("))
        {
            return Err(SyntaxError::Unsupported(
                "anonymous or referenced functions".into(),
            ));
        }
        if matches!(token,Token::Atom(a) if a=="catch") {
            return Err(SyntaxError::Unsupported("catch expressions".into()));
        }
        if matches!(token, Token::Var(_)) && tokens.get(i + 1).is_some_and(|t| t.is("(")) {
            return Err(SyntaxError::Unsupported("dynamic function calls".into()));
        }
    }

    for mode in [Mode::Type, Mode::Expression] {
        let mut p = Parser::new(tokens, mode);
        match p.expr(0) {
            Ok(e) if p.position == tokens.len() => return Ok(e),
            Err(e @ SyntaxError::Unsupported(_)) | Err(e @ SyntaxError::Limit) => return Err(e),
            _ => {}
        }
    }
    Err(SyntaxError::Invalid("unparsed expression".into()))
}
fn consecutive(xs: Vec<Doc>) -> Doc {
    Doc::Consecutive(xs)
}
fn first(a: Doc, b: Doc) -> Doc {
    Doc::First(Box::new(a), Box::new(b))
}
fn step(a: Doc, b: Doc, c: bool) -> Doc {
    Doc::Step {
        first: Box::new(a),
        second: Box::new(b),
        conditional: c,
    }
}
fn seq(before: Doc, after: Doc, sep: Doc, spaced: bool, items: Vec<Doc>) -> Doc {
    Doc::Sequence {
        before: Box::new(before),
        after: Box::new(after),
        separator: Separator::new(sep, spaced),
        items,
    }
}
fn paren(p: usize, required: usize, doc: Doc) -> Doc {
    if p < required {
        consecutive(vec![Doc::Char('('), doc, Doc::Char(')')])
    } else {
        doc
    }
}
fn args(items: Vec<Doc>) -> Doc {
    seq(
        Doc::Char('('),
        Doc::Char(')'),
        consecutive(vec![Doc::Char(',')]),
        true,
        items,
    )
}
fn tuple(items: Vec<Doc>) -> Doc {
    if items.is_empty() {
        Doc::leaf("{}")
    } else {
        seq(
            Doc::Char('{'),
            Doc::Char('}'),
            consecutive(vec![Doc::Char(',')]),
            true,
            items,
        )
    }
}
fn typed(name: Doc, ty: Doc) -> Doc {
    Doc::List(vec![step(
        consecutive(vec![name, Doc::Word(" ::".into())]),
        ty,
        true,
    )])
}
fn doc(e: &Ast, required: usize) -> Doc {
    match e {
        Ast::Atom(a, true) => Doc::Singleton(a.clone()),
        Ast::Atom(a, false) => Doc::Atom(a.clone()),
        Ast::Var(v) | Ast::Number(v) | Ast::Float(v) => Doc::leaf(v.clone()),
        Ast::String(s) => Doc::String(s.clone()),
        Ast::Char(c) => Doc::Character(*c),
        Ast::Nil => Doc::Word("[]".into()),
        Ast::Tuple(xs) => tuple(xs.iter().map(|x| doc(x, 0)).collect()),
        Ast::ListType(x, false) => seq(
            Doc::Char('['),
            Doc::Char(']'),
            Doc::Char(','),
            false,
            vec![doc(x, 0)],
        ),
        Ast::ListType(x, true) => seq(
            Doc::Char('['),
            Doc::Char(']'),
            consecutive(vec![Doc::Char(',')]),
            true,
            vec![doc(x, 0), Doc::leaf("...")],
        ),
        Ast::List(xs, tail) => {
            let mut items: Vec<_> = xs.iter().map(|x| doc(x, 0)).collect();
            let separator = if let Some(t) = tail {
                items.push(doc(t, 0));
                Separator {
                    normal: Box::new(consecutive(vec![Doc::Char(',')])),
                    last: Box::new(consecutive(vec![Doc::Word(" |".into())])),
                    spaced: true,
                }
            } else {
                Separator::new(consecutive(vec![Doc::Char(',')]), true)
            };
            Doc::Sequence {
                before: Box::new(Doc::Char('[')),
                after: Box::new(Doc::Char(']')),
                separator,
                items,
            }
        }
        Ast::Call {
            module,
            name,
            args: xs,
        } if module.is_none() && name == "fun" && xs.is_empty() => Doc::leaf("fun()"),
        Ast::Call {
            module,
            name,
            args: xs,
        } => {
            let name = if let Some(m) = module {
                consecutive(vec![
                    Doc::Atom(m.clone()),
                    Doc::Char(':'),
                    Doc::Atom(name.clone()),
                ])
            } else {
                Doc::Atom(name.clone())
            };
            first(name, args(xs.iter().map(|x| doc(x, 0)).collect()))
        }
        Ast::Map(xs) => paren(
            700,
            required,
            first(
                consecutive(vec![Doc::Char('#')]),
                tuple(
                    xs.iter()
                        .map(|(k, op, v)| {
                            Doc::List(vec![step(
                                consecutive(vec![doc(k, 0), Doc::leaf(format!(" {op}"))]),
                                doc(v, 0),
                                true,
                            )])
                        })
                        .collect(),
                ),
            ),
        ),
        Ast::Record(n, xs) => paren(
            700,
            required,
            first(
                consecutive(vec![Doc::Char('#'), Doc::Atom(n.clone())]),
                tuple(
                    xs.iter()
                        .map(|(n, op, v)| {
                            if op == "::" {
                                typed(Doc::Atom(n.clone()), doc(v, 0))
                            } else {
                                Doc::List(vec![step(
                                    consecutive(vec![Doc::Atom(n.clone()), Doc::Word(" =".into())]),
                                    doc(v, 100),
                                    true,
                                )])
                            }
                        })
                        .collect(),
                ),
            ),
        ),
        Ast::BinaryType(base, unit) => {
            let mut xs = Vec::new();
            if !matches!(base.as_ref(),Ast::Number(n)if n=="0") {
                xs.push(consecutive(vec![
                    Doc::leaf("_:"),
                    expression_doc(base, 900),
                ]))
            }
            if !matches!(unit.as_ref(),Ast::Number(n)if n=="0") {
                xs.push(consecutive(vec![
                    Doc::leaf("_:_*"),
                    expression_doc(unit, 900),
                ]))
            }
            if xs.is_empty() {
                Doc::leaf("<<>>")
            } else {
                seq(
                    Doc::Word("<<".into()),
                    Doc::Word(">>".into()),
                    consecutive(vec![Doc::Char(',')]),
                    true,
                    xs,
                )
            }
        }
        Ast::Binary(xs) => {
            if xs.is_empty() {
                return Doc::leaf("<<>>");
            }
            let items = xs
                .iter()
                .map(|segment| {
                    let value = &segment.value;
                    let size = &segment.size;
                    let types = &segment.specifiers;
                    let mut ds = vec![doc(value, 900)];
                    if let Some(s) = size {
                        ds.push(Doc::Char(':'));
                        ds.push(doc(s, 900))
                    }
                    if !types.is_empty() {
                        ds.push(Doc::Char('/'));
                        for (i, specifier) in types.iter().enumerate() {
                            let name = &specifier.name;
                            let value = &specifier.unit;
                            if i > 0 {
                                ds.push(Doc::Char('-'))
                            }
                            ds.push(Doc::Atom(name.clone()));
                            if let Some(v) = value {
                                ds.push(Doc::Char(':'));
                                ds.push(Doc::leaf(v.clone()))
                            }
                        }
                    }
                    consecutive(ds)
                })
                .collect();
            consecutive(vec![
                Doc::Word("<<".into()),
                seq(Doc::Empty, Doc::Empty, Doc::Char(','), false, items),
                Doc::Word(">>".into()),
            ])
        }
        Ast::Fun(domain, ret) => consecutive(vec![
            fun_doc(
                consecutive(vec![Doc::Word("fun".into()), Doc::Char('(')]),
                domain,
                ret,
            ),
            Doc::Char(')'),
        ]),
        Ast::Unary(op, e) => {
            let d = if op == "+" || op == "-" {
                consecutive(vec![
                    Doc::Reserved(Box::new(Doc::leaf(op.clone()))),
                    expression_doc(e, 700),
                ])
            } else {
                consecutive(vec![
                    Doc::Reserved(Box::new(Doc::leaf(format!("{op} ")))),
                    expression_doc(e, 700),
                ])
            };
            paren(600, required, d)
        }
        Ast::Op(op, l, r) => {
            let mode = if ["::", "|", ".."].contains(&op.as_str()) {
                Mode::Type
            } else {
                Mode::Expression
            };
            let (lp, p, rp) = prec(op, mode).unwrap();
            let d = match op.as_str() {
                "::" => typed(doc(l, lp), doc(r, rp)),
                "|" => {
                    let mut xs = vec![doc(l, rp)];
                    fn right(e: &Ast, p: usize, xs: &mut Vec<Doc>) {
                        if let Ast::Op(op, l, r) = e
                            && op == "|"
                        {
                            xs.push(doc(l, p));
                            right(r, p, xs)
                        } else {
                            xs.push(doc(e, p))
                        }
                    }
                    right(r, rp, &mut xs);
                    seq(
                        Doc::Empty,
                        Doc::Empty,
                        consecutive(vec![Doc::Word(" |".into())]),
                        true,
                        xs,
                    )
                }
                ".." => seq(
                    Doc::Empty,
                    Doc::Empty,
                    Doc::Word("..".into()),
                    false,
                    vec![expression_doc(l, rp), expression_doc(r, rp)],
                ),
                "=" => Doc::List(vec![step(
                    consecutive(vec![doc(l, lp), Doc::Word(" =".into())]),
                    doc(r, rp),
                    true,
                )]),
                "orelse" | "andalso" => Doc::PreferNl {
                    separator: Separator::new(consecutive(vec![Doc::Empty]), true),
                    items: vec![
                        expression_doc(l, lp),
                        Doc::Reserved(Box::new(Doc::leaf(op.clone()))),
                        doc(r, rp),
                    ],
                },
                _ => Doc::List(vec![
                    expression_doc(l, lp),
                    Doc::Reserved(Box::new(Doc::leaf(op.clone()))),
                    expression_doc(r, rp),
                ]),
            };
            paren(p, required, d)
        }
    }
}
fn expression_doc(e: &Ast, p: usize) -> Doc {
    let mut e = e.clone();
    fn strip(e: &mut Ast) {
        match e {
            Ast::Atom(_, singleton) => *singleton = false,
            Ast::Unary(_, x) => strip(x),
            Ast::Op(_, l, r) => {
                strip(l);
                strip(r)
            }
            _ => {}
        }
    }
    strip(&mut e);
    doc(&e, p)
}
fn fun_doc(before: Doc, domain: &Option<Vec<Ast>>, ret: &Ast) -> Doc {
    let ds = domain
        .as_ref()
        .map(|xs| args(xs.iter().map(|x| doc(x, 0)).collect()))
        .unwrap_or_else(|| Doc::leaf("(...)"));
    first(
        before,
        step(
            consecutive(vec![ds, Doc::Word(" ->".into())]),
            doc(ret, 0),
            false,
        ),
    )
}
fn pp_type(ast: &Ast) -> Result<String> {
    let full = consecutive(vec![
        first(
            Doc::leaf("-type "),
            typed(first(Doc::Atom("t".into()), args(vec![])), doc(ast, 0)),
        ),
        Doc::leaf(".\n"),
    ])
    .format(0)
    .map_err(SyntaxError::Unsupported)?;
    let (_, body) = full
        .split_once("::")
        .ok_or_else(|| SyntaxError::Unsupported("type document omitted annotation".into()))?;
    Ok(body
        .trim_start()
        .strip_suffix(".\n")
        .ok_or_else(|| SyntaxError::Unsupported("type document terminator".into()))?
        .into())
}
fn split_tokens(tokens: &[Token]) -> Result<Vec<&[Token]>> {
    let mut stack = Vec::new();
    let mut start = 0;
    let mut out = Vec::new();
    for (i, t) in tokens.iter().enumerate() {
        if t.is(",") && stack.is_empty() {
            out.push(&tokens[start..i]);
            start = i + 1
        } else if let Token::Symbol(s) = t {
            match s.as_str() {
                "(" => stack.push(")"),
                "[" => stack.push("]"),
                "{" => stack.push("}"),
                "<<" => stack.push(">>"),
                ")" | "]" | "}" | ">>" => {
                    if stack.pop() != Some(s.as_str()) {
                        return Err(SyntaxError::Invalid("unbalanced delimiter".into()));
                    }
                }
                _ => {}
            }
        }
    }
    if !stack.is_empty() {
        return Err(SyntaxError::Invalid("unbalanced delimiter".into()));
    }
    if start < tokens.len() {
        out.push(&tokens[start..])
    }
    Ok(out)
}
pub fn format_type_or_literal(source: &str) -> Result<String> {
    let tokens = match lex(source) {
        Ok(t) => Some(t),
        Err(SyntaxError::Invalid(_)) => None,
        Err(e) => return Err(e),
    };
    if !source.starts_with('(')
        && let Some(tokens) = tokens
    {
        match parse_one(&tokens) {
            Ok(ast) => return Ok(format!("\n          {}", pp_type(&ast)?)),
            Err(SyntaxError::Invalid(_)) => {}
            Err(e) => return Err(e),
        }
    }
    let chars: Vec<_> = source.chars().collect();
    if chars.len() < 2 {
        return Err(SyntaxError::NativeBadMatchNil);
    }
    let inner: String = chars[1..chars.len() - 1].iter().collect();
    let tokens = match lex(&inner) {
        Ok(t) => t,
        Err(SyntaxError::Invalid(_)) => return Ok(source.into()),
        Err(e) => return Err(e),
    };
    let chunks = match split_tokens(&tokens) {
        Ok(t) => t,
        Err(SyntaxError::Invalid(_)) => return Ok(source.into()),
        Err(e) => return Err(e),
    };
    let mut bodies = Vec::new();
    for chunk in chunks {
        match parse_one(chunk) {
            Ok(ast) => bodies.push(pp_type(&ast)?),
            Err(SyntaxError::Invalid(_)) => return Ok(source.into()),
            Err(e) => return Err(e),
        }
    }
    Ok(format!(
        "\n         {}{}{}",
        chars[0],
        bodies.join(",\n          "),
        chars[chars.len() - 1]
    ))
}
pub fn format_signature(source: &str) -> Result<String> {
    let tokens = match lex(source) {
        Ok(t) => t,
        Err(SyntaxError::Invalid(_)) => return Ok(source.into()),
        Err(e) => return Err(e),
    };
    let mut p = Parser::new(&tokens, Mode::Type);
    let parsed = (|| {
        let mut clauses = Vec::new();
        loop {
            p.expect("(")?;
            let domain = if p.take("...") {
                p.expect(")")?;
                None
            } else {
                Some(p.expressions(")")?)
            };
            p.expect("->")?;
            if domain.is_none() {
                return Err(SyntaxError::Invalid("spec requires fixed arity".into()));
            }
            let ret = p.expr(0)?;
            let mut fd = fun_doc(Doc::Empty, &domain, &ret);
            if p.take("when") {
                let mut constraints = Vec::new();
                loop {
                    let ast = p.expr(0)?;
                    let ast = match ast {
                        Ast::Op(ref op, ref left, _)
                            if op == "::" && matches!(left.as_ref(),Ast::Var(v)if v!="_") =>
                        {
                            ast
                        }
                        Ast::Call {
                            module: None,
                            name,
                            args,
                        } if name == "is_subtype"
                            && args.len() == 2
                            && matches!(&args[0],Ast::Var(v)if v!="_") =>
                        {
                            Ast::Op(
                                "::".into(),
                                Box::new(args[0].clone()),
                                Box::new(args[1].clone()),
                            )
                        }
                        _ => return Err(SyntaxError::Invalid("unsupported constraint".into())),
                    };
                    constraints.push(doc(&ast, 0));
                    if !p.take(",") {
                        break;
                    }
                }
                let ds = seq(
                    Doc::Empty,
                    Doc::Empty,
                    consecutive(vec![Doc::Char(',')]),
                    true,
                    constraints,
                );
                fd = Doc::List(vec![step(
                    fd,
                    Doc::List(vec![step(Doc::Word("when".into()), ds, false)]),
                    false,
                )])
            }
            clauses.push(fd);
            if !p.take(";") {
                break;
            }
        }
        if p.position != tokens.len() {
            return Err(SyntaxError::Invalid("signature suffix".into()));
        }
        Ok(clauses)
    })();
    let clauses = match parsed {
        Ok(c) => c,
        Err(SyntaxError::Invalid(_)) => return Ok(source.into()),
        Err(e) => return Err(e),
    };
    let full = consecutive(vec![
        first(
            Doc::leaf("-spec "),
            Doc::List(vec![first(
                consecutive(vec![
                    Doc::Atom("a".into()),
                    Doc::Char(':'),
                    Doc::Atom("b".into()),
                ]),
                Doc::PreferNl {
                    separator: Separator::new(consecutive(vec![Doc::Char(';')]), true),
                    items: clauses,
                },
            )]),
        ),
        Doc::leaf(".\n"),
    ])
    .format(0)
    .map_err(SyntaxError::Unsupported)?;
    let body = full
        .strip_prefix("-spec a:b")
        .and_then(|s| s.strip_suffix(".\n"))
        .ok_or_else(|| SyntaxError::Unsupported("signature wrapper".into()))?
        .trim_start();
    Ok(format!("\n          {body}"))
}
pub fn format_field_diffs(source: &str) -> Result<String> {
    source
        .split(" and ")
        .map(|field| {
            let mut parts = field.split("::");
            let name = parts.next().unwrap_or("");
            let ty = parts.collect::<Vec<_>>().join("::");
            Ok(format!("{name} ::{}", format_type_or_literal(&ty)?))
        })
        .collect::<Result<Vec<_>>>()
        .map(|x| x.join(" and "))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn depth_and_input_limits_are_explicit() {
        assert_eq!(
            format_type_or_literal(&format!("{}X{}", "(".repeat(100_000), ")".repeat(100_000))),
            Err(SyntaxError::Limit)
        );
        assert_eq!(
            format_type_or_literal(&"x".repeat(1_000_001)),
            Err(SyntaxError::Limit)
        );
    }
    #[test]
    fn unsupported_valid_expression_is_not_raw_fallback() {
        assert!(matches!(
            format_type_or_literal("case X of a -> b end"),
            Err(SyntaxError::Unsupported(_))
        ));
    }
    #[test]
    fn structured_type_expression_is_explicitly_unsupported() {
        assert!(matches!(
            format_type_or_literal("integer()+1"),
            Err(SyntaxError::Unsupported(_))
        ));
    }
    #[test]
    fn malformed_helper_matches_native_error() {
        assert_eq!(
            format_type_or_literal(""),
            Err(SyntaxError::NativeBadMatchNil)
        );
        assert_eq!(
            format_field_diffs("field"),
            Err(SyntaxError::NativeBadMatchNil)
        );
    }
    #[test]
    fn invalid_annotation_retains_native_source() {
        let input = "(m:t() :: atom())";
        assert_eq!(format_type_or_literal(input).unwrap(), input);
    }
}
