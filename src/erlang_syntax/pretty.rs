use crate::{
    core::{ErlangTerm, ErlangTerms, TermId},
    lattice::PresentationContext,
};
use unicode_segmentation::UnicodeSegmentation;
#[derive(Clone, Debug)]
pub enum Doc {
    Empty,
    Char(char),
    Character(char),
    Leaf {
        length: usize,
        text: String,
    },
    Word(String),
    Atom(String),
    Singleton(String),
    String(String),
    Consecutive(Vec<Doc>),
    List(Vec<Doc>),
    First(Box<Doc>, Box<Doc>),
    Sequence {
        before: Box<Doc>,
        after: Box<Doc>,
        separator: Separator,
        items: Vec<Doc>,
    },
    PreferNl {
        separator: Separator,
        items: Vec<Doc>,
    },
    Step {
        first: Box<Doc>,
        second: Box<Doc>,
        conditional: bool,
    },
    Reserved(Box<Doc>),
}
#[derive(Clone, Debug)]
pub struct Separator {
    pub normal: Box<Doc>,
    pub last: Box<Doc>,
    pub spaced: bool,
}
impl Separator {
    pub fn new(doc: Doc, spaced: bool) -> Self {
        Self {
            normal: Box::new(doc.clone()),
            last: Box::new(doc),
            spaced,
        }
    }
}
impl Doc {
    pub fn leaf(text: impl Into<String>) -> Self {
        let text = text.into();
        Self::Leaf {
            length: text.graphemes(true).count(),
            text,
        }
    }
    pub fn parse(arena: &ErlangTerms, root: TermId) -> Result<Self, String> {
        let mut measures: Vec<(usize, usize)> = Vec::with_capacity(arena.terms.len());
        for term in &arena.terms {
            let mut depth = 1usize;
            let mut expanded = 1usize;
            let mut add = |id: TermId| -> Result<(), String> {
                let &(d, n) = measures.get(id.0).ok_or("invalid document reference")?;
                depth = depth.max(d.saturating_add(1));
                expanded = expanded.saturating_add(n);
                Ok(())
            };
            match term {
                ErlangTerm::Tuple(xs) => {
                    for &x in xs {
                        add(x)?
                    }
                }
                ErlangTerm::List { items, tail } => {
                    for &x in items {
                        add(x)?
                    }
                    add(*tail)?
                }
                ErlangTerm::Map(xs) => {
                    for &(a, b) in xs {
                        add(a)?;
                        add(b)?
                    }
                }
                _ => {}
            }
            if depth > 128 || expanded > 1_000_000 {
                return Err("document resource limit".into());
            }
            measures.push((depth, expanded));
        }
        parse(arena, root, 0)
    }
    pub fn format(&self, indent: usize) -> Result<String, String> {
        Ok(format_doc(self, indent)?.text)
    }
    fn atomic(&self) -> bool {
        matches!(
            self,
            Self::Leaf { .. } | Self::Atom(_) | Self::Singleton(_) | Self::Word(_)
        )
    }
}
fn term(a: &ErlangTerms, id: TermId) -> Result<&ErlangTerm, String> {
    a.terms.get(id.0).ok_or("invalid document reference".into())
}
fn list(a: &ErlangTerms, id: TermId) -> Result<Vec<TermId>, String> {
    let mut out = Vec::new();
    let mut next = id;
    loop {
        match term(a, next)? {
            ErlangTerm::Nil => return Ok(out),
            ErlangTerm::List { items, tail } => {
                out.extend(items);
                next = *tail
            }
            _ => return Err("expected proper document list".into()),
        }
    }
}
fn atom(a: &ErlangTerms, id: TermId) -> Result<String, String> {
    if let ErlangTerm::Atom(s) = term(a, id)? {
        Ok(s.clone())
    } else {
        Err("expected document atom".into())
    }
}
fn chars(a: &ErlangTerms, id: TermId) -> Result<String, String> {
    let mut out = String::new();
    let mut todo = vec![id];
    while let Some(id) = todo.pop() {
        match term(a, id)? {
            ErlangTerm::Integer(n) => out.push(
                char::from_u32(n.parse::<u32>().map_err(|_| "invalid character")?)
                    .ok_or("invalid character")?,
            ),
            ErlangTerm::List { items, tail } => {
                todo.push(*tail);
                todo.extend(items.iter().rev())
            }
            ErlangTerm::Nil => {}
            _ => return Err("expected document character data".into()),
        }
    }
    Ok(out)
}
fn separator(a: &ErlangTerms, id: TermId, d: usize) -> Result<Separator, String> {
    let spaced = matches!(term(a,id)?,ErlangTerm::List{items,..} if !items.is_empty());
    let pair = match term(a, id)? {
        ErlangTerm::Tuple(xs) if xs.len() == 2 => Some(xs.clone()),
        ErlangTerm::List { .. } => {
            let xs = list(a, id)?;
            if xs.len() == 1 {
                match term(a, xs[0])? {
                    ErlangTerm::Tuple(p) if p.len() == 2 => Some(p.clone()),
                    _ => None,
                }
            } else {
                None
            }
        }
        _ => None,
    };
    if let Some(xs) = pair {
        let n = parse(a, xs[0], d + 1)?;
        let l = parse(a, xs[1], d + 1)?;
        Ok(Separator {
            normal: Box::new(if spaced { Doc::Consecutive(vec![n]) } else { n }),
            last: Box::new(if spaced { Doc::Consecutive(vec![l]) } else { l }),
            spaced,
        })
    } else {
        Ok(Separator::new(parse(a, id, d + 1)?, spaced))
    }
}
fn parse(a: &ErlangTerms, id: TermId, d: usize) -> Result<Doc, String> {
    if d > 128 {
        return Err("document depth exceeds 128".into());
    }
    Ok(match term(a, id)? {
        ErlangTerm::Nil => Doc::Empty,
        ErlangTerm::Integer(n) => Doc::Char(
            char::from_u32(n.parse::<u32>().map_err(|_| "invalid document character")?)
                .ok_or("invalid document character")?,
        ),
        ErlangTerm::Atom(s) => Doc::Word(s.clone()),
        ErlangTerm::List { .. } => Doc::Consecutive(
            list(a, id)?
                .iter()
                .map(|&x| parse(a, x, d + 1))
                .collect::<Result<_, _>>()?,
        ),
        ErlangTerm::Tuple(xs) => {
            let tag = atom(a, *xs.first().ok_or("empty document tuple")?)?;
            let at = |i| xs.get(i).copied().ok_or("document arity".to_string());
            let p = |i| parse(a, at(i)?, d + 1);
            let ps = |i| {
                list(a, at(i)?)?
                    .iter()
                    .map(|&x| parse(a, x, d + 1))
                    .collect::<Result<Vec<_>, _>>()
            };
            match tag.as_str() {
                "leaf" => {
                    let ErlangTerm::Integer(n) = term(a, at(1)?)? else {
                        return Err("document leaf length".into());
                    };
                    Doc::Leaf {
                        length: n.parse().map_err(|_| "document leaf length")?,
                        text: chars(a, at(2)?)?,
                    }
                }
                "atom" => Doc::Atom(atom(a, at(1)?)?),
                "singleton_atom_type" => Doc::Singleton(atom(a, at(1)?)?),
                "string" => Doc::String(chars(a, at(1)?)?),
                "char" => {
                    let ErlangTerm::Integer(n) = term(a, at(1)?)? else {
                        return Err("character literal".into());
                    };
                    Doc::Character(
                        char::from_u32(n.parse().map_err(|_| "character literal")?)
                            .ok_or("character literal")?,
                    )
                }
                "list" => Doc::List(ps(1)?),
                "first" => Doc::First(Box::new(p(1)?), Box::new(p(2)?)),
                "seq" => Doc::Sequence {
                    before: Box::new(p(1)?),
                    after: Box::new(p(2)?),
                    separator: separator(a, at(3)?, d + 1)?,
                    items: ps(4)?,
                },
                "prefer_nl" => Doc::PreferNl {
                    separator: separator(a, at(1)?, d + 1)?,
                    items: ps(2)?,
                },
                "step" | "cstep" => Doc::Step {
                    first: Box::new(p(1)?),
                    second: Box::new(p(2)?),
                    conditional: tag == "cstep",
                },
                "reserved" => Doc::Reserved(Box::new(p(1)?)),
                _ => return Err(format!("unsupported document kind {tag}")),
            }
        }
        _ => return Err("unsupported document literal".into()),
    })
}
#[derive(Clone, Copy, Debug, PartialEq)]
enum Size {
    Flat(usize),
    Line(usize),
}
impl Size {
    fn zero(self) -> bool {
        self == Self::Flat(0)
    }
    fn indent(self, i: usize) -> usize {
        match self {
            Self::Flat(n) => i + n,
            Self::Line(n) => n,
        }
    }
    fn nsz(self, i: usize) -> Self {
        match self {
            Self::Flat(n) => Self::Line(n + i),
            s => s,
        }
    }
}
#[derive(Clone, Debug)]
struct Output {
    text: String,
    size: Size,
}
fn output(text: String, size: Size) -> Output {
    Output { text, size }
}
fn line_size(xs: impl Iterator<Item = Size>) -> Size {
    let mut n = 0;
    let mut line = false;
    for s in xs {
        match s {
            Size::Flat(x) => n += x,
            Size::Line(x) => {
                n = x;
                line = true
            }
        }
    }
    if line { Size::Line(n) } else { Size::Flat(n) }
}
fn nl(i: usize) -> String {
    format!("\n{}", " ".repeat(i))
}
fn concat(xs: &[Doc], i: usize) -> Result<Output, String> {
    let mut text = String::new();
    let mut current = i;
    let mut sizes = Vec::new();
    for d in xs {
        let p = format_doc(d, current)?;
        current = p.size.indent(current);
        if !p.size.zero() {
            text.push_str(&p.text);
            sizes.push(p.size)
        }
    }
    Ok(output(text, line_size(sizes.into_iter())))
}
fn parts(
    items: &[Doc],
    sep: &Separator,
    i: usize,
    after: &Doc,
) -> Result<Vec<[Output; 2]>, String> {
    if items.is_empty() {
        return Ok(vec![[
            format_doc(after, i)?,
            output(String::new(), Size::Flat(0)),
        ]]);
    }
    let mut out = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let suffix = if index + 1 == items.len() {
            after
        } else if index + 2 == items.len() {
            sep.last.as_ref()
        } else {
            sep.normal.as_ref()
        };
        let zero = output(String::new(), Size::Flat(0));
        out.push(match item {
            Doc::Step {
                first,
                second,
                conditional,
            } => {
                let p = format_doc(first, i)?;
                if *conditional && matches!(p.size,Size::Flat(n)if n<4) {
                    let tail = concat(
                        &[Doc::leaf(" "), second.as_ref().clone(), suffix.clone()],
                        p.size.indent(i),
                    )?;
                    [
                        output(
                            format!("{}{}", p.text, tail.text),
                            line_size([p.size, tail.size].into_iter()),
                        ),
                        zero,
                    ]
                } else {
                    [
                        p,
                        concat(&[second.as_ref().clone(), suffix.clone()], i + 4)?,
                    ]
                }
            }
            Doc::Reserved(word) => [concat(&[word.as_ref().clone(), suffix.clone()], i)?, zero],
            _ => [concat(&[item.clone(), suffix.clone()], i)?, zero],
        })
    }
    Ok(out)
}
fn handle(ps: &[[Output; 2]], i: usize) -> Vec<Output> {
    ps.iter()
        .map(|[p, q]| {
            if q.size.zero() {
                p.clone()
            } else {
                output(
                    format!("{}{}{}", p.text, nl(i + 4), q.text),
                    line_size([p.size, q.size].into_iter()),
                )
            }
        })
        .collect()
}
fn sequence(
    before: &Doc,
    after: &Doc,
    sep: &Separator,
    items: &[Doc],
    i0: usize,
) -> Result<Output, String> {
    let b = format_doc(before, i0)?;
    let i = b.size.indent(i0);
    let ps = parts(items, sep, i, after)?;
    let flat: Vec<&Output> = ps.iter().flatten().filter(|p| !p.size.zero()).collect();
    let spaces = if sep.spaced {
        flat.len().saturating_sub(1)
    } else {
        0
    };
    let mut sum = spaces;
    let mut same = true;
    for p in std::iter::once(&b).chain(flat.iter().copied()) {
        match p.size {
            Size::Flat(n) => sum += n,
            Size::Line(_) => same = false,
        }
    }
    if same && i0 + sum <= 72 {
        return Ok(output(
            format!(
                "{}{}",
                b.text,
                flat.iter()
                    .map(|p| p.text.as_str())
                    .collect::<Vec<_>>()
                    .join(if spaces > 0 { " " } else { "" })
            ),
            Size::Flat(sum),
        ));
    }
    let hs = handle(&ps, i);
    let out = if items.is_empty() {
        hs[0].clone()
    } else {
        maybe_sep(&hs, items, i, spaces)?
    };
    Ok(output(format!("{}{}", b.text, out.text), out.size.nsz(i0)))
}
fn maybe_sep(ps: &[Output], items: &[Doc], i: usize, spaces: usize) -> Result<Output, String> {
    let Some(p) = ps.first() else {
        return Err("empty sequence".into());
    };
    let mut current = if items[0].atomic() {
        if let Size::Flat(n) = p.size {
            i + n
        } else {
            return Err("atomic document unexpectedly multiline".into());
        }
    } else {
        73
    };
    let mut sz = p.size;
    let mut text = p.text.clone();
    for (p, item) in ps.iter().skip(1).zip(items.iter().skip(1)) {
        if item.atomic()
            && let Size::Flat(n) = p.size
        {
            let add = n + 1;
            if current + add <= 72 {
                if spaces > 0 {
                    text.push(' ')
                }
                text.push_str(&p.text);
                current += add;
                sz = match sz {
                    Size::Flat(old) => Size::Flat(old + add),
                    _ => return Err("atomic accumulator unexpectedly multiline".into()),
                };
            } else {
                text.push_str(&nl(i));
                text.push_str(&p.text);
                current = i + n;
                sz = Size::Flat(add)
            }
        } else {
            text.push_str(&nl(i));
            text.push_str(&p.text);
            current = 73;
            sz = Size::Flat(0)
        }
    }
    Ok(output(text, sz))
}
fn escaped(c: char, quote: Option<char>) -> String {
    if quote == Some(c) || c == '\\' {
        return format!("\\{c}");
    }
    match c {
        '\n' => "\\n".into(),
        '\r' => "\\r".into(),
        '\t' => "\\t".into(),
        '\u{b}' => "\\v".into(),
        '\u{8}' => "\\b".into(),
        '\u{c}' => "\\f".into(),
        '\u{1b}' => "\\e".into(),
        '\u{7f}' => "\\d".into(),
        c if (32..=126).contains(&(c as u32)) || c as u32 >= 160 => c.to_string(),
        c => format!("\\{:03o}", c as u32),
    }
}
fn quoted(s: &str) -> String {
    format!(
        "\"{}\"",
        s.chars().map(|c| escaped(c, Some('"'))).collect::<String>()
    )
}

fn format_doc(d: &Doc, i: usize) -> Result<Output, String> {
    match d {
        Doc::Empty => Ok(output(String::new(), Size::Flat(0))),
        Doc::Char(c) => Ok(output(c.to_string(), Size::Flat(1))),
        Doc::Character(c) => format_doc(
            &Doc::leaf(if *c == ' ' {
                "$\\s".into()
            } else {
                format!("${}", escaped(*c, None))
            }),
            i,
        ),
        Doc::Leaf { length, text } => Ok(output(text.clone(), Size::Flat(*length))),
        Doc::Word(w) => {
            if ![
                " ->", " =", "<<", ">>", "[]", "after", "begin", "case", "catch", "end", "fun",
                "if", "of", "receive", "try", "when", " ::", "..", " |", "maybe", "else", "#{",
            ]
            .contains(&w.as_str())
            {
                return Err(format!("unknown erl_pp word {w}"));
            }
            Ok(output(w.clone(), Size::Flat(w.graphemes(true).count())))
        }
        Doc::Atom(a) => format_doc(
            &Doc::leaf(PresentationContext::default().atom_to_string(a)),
            i,
        ),
        Doc::Singleton(a) => {
            format_doc(&Doc::leaf(PresentationContext::default().quoted_atom(a)), i)
        }
        Doc::String(s) => {
            let gs: Vec<_> = s.graphemes(true).collect();
            let max = 72usize.saturating_sub(i).max(5);
            let mut index = 0;
            let mut leaves = Vec::new();
            while index < gs.len() {
                let mut n = max.min(gs.len() - index);
                let mut q = quoted(&gs[index..index + n].concat());
                while q.graphemes(true).count() > max && n > 5 {
                    n -= 1;
                    q = quoted(&gs[index..index + n].concat())
                }
                leaves.push(Doc::leaf(q));
                index += n
            }
            if leaves.is_empty() {
                format_doc(&Doc::leaf("\"\""), i)
            } else {
                format_doc(&Doc::List(leaves), i)
            }
        }
        Doc::Consecutive(xs) => concat(xs, i),
        Doc::List(xs) => sequence(
            &Doc::Empty,
            &Doc::Empty,
            &Separator::new(Doc::Consecutive(vec![Doc::Empty]), true),
            xs,
            i,
        ),
        Doc::First(b, x) => sequence(
            b,
            &Doc::Empty,
            &Separator::new(Doc::Consecutive(vec![Doc::Empty]), true),
            &[x.as_ref().clone()],
            i,
        ),
        Doc::Sequence {
            before,
            after,
            separator,
            items,
        } => sequence(before, after, separator, items, i),
        Doc::PreferNl { separator, items } => {
            let ps = parts(items, separator, i, &Doc::Empty)?;
            let flat: Vec<_> = ps.iter().flatten().filter(|p| !p.size.zero()).collect();
            if flat.is_empty() {
                return Ok(output(String::new(), Size::Flat(0)));
            }
            let size = flat.last().unwrap().size.nsz(i);
            let hs = handle(&ps, i);
            Ok(output(
                hs.iter()
                    .filter(|p| !p.size.zero())
                    .map(|p| p.text.as_str())
                    .collect::<Vec<_>>()
                    .join(&nl(i)),
                size,
            ))
        }
        Doc::Reserved(r) => format_doc(r, i),
        Doc::Step { .. } => Err("step outside sequence".into()),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arena_depth_and_dag_expansion_are_checked_before_construction() {
        for (count, copies) in [(100_000, 1), (30, 2)] {
            let mut terms = vec![ErlangTerm::Nil];
            for i in 0..count {
                terms.push(ErlangTerm::Tuple(vec![TermId(i); copies]));
            }
            let arena = ErlangTerms {
                root: TermId(count),
                terms: terms.into(),
            };
            assert!(Doc::parse(&arena, arena.root).is_err());
        }
    }
}
