//! Compile-time evaluation of Elixir's inline-pure calls.
//!
//! `elixir_erl_pass:rewrite_strategy/3` applies a fixed set of standard
//! library functions to literal arguments while translating, and embeds the
//! result. A call that raises is kept. These are ports of Elixir 1.19.4 and
//! OTP 28.3 (`uri_string`, `inet_parse`) for the functions in that set.
//!
//! `evaluate` returns `Ok(Some(value))` when Elixir folds the call, `Ok(None)`
//! when the call raises and stays, and `Err` for arguments not ported here.

use num_bigint::BigInt;

#[derive(Clone, Debug, PartialEq)]
pub enum Term {
    Atom(String),
    Int(BigInt),
    /// IEEE-754 bits.
    Float(u64),
    Bin(Vec<u8>),
    List(Vec<Term>),
    Tuple(Vec<Term>),
    Map(Vec<(Term, Term)>),
}

type Eval = Result<Option<Term>, String>;

fn atom(name: &str) -> Term {
    Term::Atom(name.into())
}
fn nil() -> Term {
    atom("nil")
}
fn bin(bytes: &[u8]) -> Term {
    Term::Bin(bytes.to_vec())
}
fn int(value: impl Into<BigInt>) -> Term {
    Term::Int(value.into())
}
fn ok(value: Term) -> Term {
    Term::Tuple(vec![atom("ok"), value])
}
fn opt_bin(value: Option<&[u8]>) -> Term {
    value.map_or_else(nil, bin)
}
fn structure(name: &str, fields: Vec<(&str, Term)>) -> Term {
    let mut entries = vec![(atom("__struct__"), atom(name))];
    entries.extend(fields.into_iter().map(|(k, v)| (atom(k), v)));
    Term::Map(entries)
}

pub fn evaluate(module: &str, name: &str, args: &[Term]) -> Eval {
    match (module, name, args) {
        ("Elixir.URI", "parse", [Term::Bin(s)]) => Ok(Some(uri_parse(s))),
        ("Elixir.URI", "new", [Term::Bin(s)]) => Ok(match uri_string_parse(s) {
            Ok(map) => Some(ok(uri_from_map(map))),
            Err(Fail::Throw(part)) => Some(Term::Tuple(vec![atom("error"), Term::Bin(part)])),
            Err(Fail::Crash) => None,
        }),
        ("Elixir.URI", "new!", [Term::Bin(s)]) => Ok(uri_string_parse(s).ok().map(uri_from_map)),
        ("Elixir.URI", "encode_query", [pairs]) => encode_query(pairs, &atom("www_form")),
        ("Elixir.URI", "encode_query", [pairs, encoding]) => encode_query(pairs, encoding),
        ("Elixir.URI", "encode_www_form", [Term::Bin(s)]) => Ok(Some(Term::Bin(encode_www_form(s)))),
        ("Elixir.URI", "decode", [Term::Bin(s)]) => Ok(Some(Term::Bin(unpercent(s, false)))),
        ("Elixir.Version", "parse", [Term::Bin(s)]) => Ok(Some(
            version_parse(s).map_or_else(|| atom("error"), ok),
        )),
        ("Elixir.Version", "parse!", [Term::Bin(s)]) => Ok(version_parse(s)),
        ("Elixir.Version", "parse_requirement", [Term::Bin(s)]) => Ok(match requirement(s) {
            Requirement::Ok(value) => Some(ok(value)),
            Requirement::Error => Some(atom("error")),
            Requirement::Crash => None,
        }),
        ("Elixir.Version", "parse_requirement!", [Term::Bin(s)]) => Ok(match requirement(s) {
            Requirement::Ok(value) => Some(value),
            _ => None,
        }),
        ("Elixir.Duration", "new!", [pairs]) => Ok(duration_new(pairs)),
        ("Elixir.String", "split", [Term::Bin(s)]) => Ok(Some(split_whitespace(s))),
        ("Elixir.String", "split", [Term::Bin(s), pattern]) => split(s, pattern, &Term::List(vec![])),
        ("Elixir.String", "split", [Term::Bin(s), pattern, options]) => split(s, pattern, options),
        // Every other shape raises a FunctionClauseError or
        // UndefinedFunctionError (including Elixir's own `decode_www_for`).
        ("Elixir.URI" | "Elixir.Version" | "Elixir.Duration" | "Elixir.String", _, _) => Ok(None),
        _ => Err(format!("{module}.{name}/{}", args.len())),
    }
}

// ---------------------------------------------------------------- URI.parse

const DEFAULT_PORTS: [(&str, u32); 8] = [
    ("ftp", 21),
    ("sftp", 22),
    ("tftp", 69),
    ("http", 80),
    ("https", 443),
    ("ldap", 389),
    ("ws", 80),
    ("wss", 443),
];

fn default_port(scheme: &[u8]) -> Term {
    DEFAULT_PORTS
        .iter()
        .find(|(name, _)| name.as_bytes() == scheme)
        .map_or_else(nil, |(_, port)| int(*port))
}

fn uri_struct(fields: [Term; 8]) -> Term {
    let names = ["scheme", "authority", "userinfo", "host", "port", "path", "query", "fragment"];
    structure("Elixir.URI", names.into_iter().zip(fields).collect())
}

/// `URI.parse/1`: the RFC 3986 appendix B regular expression, then
/// `split_authority/1`, matched the way `Regex.run/2` reports groups.
fn uri_parse(s: &[u8]) -> Term {
    let mut i = 0;
    let mut scheme = None;
    if s.first().is_some_and(u8::is_ascii_alphabetic) {
        let end = s
            .iter()
            .skip(1)
            .position(|c| !(c.is_ascii_alphanumeric() || b"+-.".contains(c)))
            .map_or(s.len(), |p| p + 1);
        if s.get(end) == Some(&b':') {
            scheme = Some(&s[..end]);
            i = end + 1;
        }
    }
    let mut authority = &s[i..i];
    if s[i..].starts_with(b"//") {
        let end = s[i + 2..].iter().position(|c| b"/?#".contains(c)).map_or(s.len(), |p| i + 2 + p);
        authority = &s[i..end];
        i = end;
    }
    let path_end = s[i..].iter().position(|c| b"?#".contains(c)).map_or(s.len(), |p| i + p);
    let path = &s[i..path_end];
    i = path_end;
    let mut query = None;
    if s.get(i) == Some(&b'?') {
        let end = s[i..].iter().position(|&c| c == b'#').map_or(s.len(), |p| i + p);
        query = Some(&s[i + 1..end]);
        i = end;
    }
    // `.` stops at a newline; the expression is not anchored at the end.
    let fragment = (s.get(i) == Some(&b'#')).then(|| {
        let rest = &s[i + 1..];
        &rest[..rest.iter().position(|&c| c == b'\n').unwrap_or(rest.len())]
    });

    let (authority, userinfo, host, port) = match authority {
        b"" => (nil(), nil(), nil(), None),
        b"//" => (bin(b""), nil(), bin(b""), None),
        whole => {
            let a = &whole[2..];
            let line = a.iter().position(|&c| c == b'\n').unwrap_or(a.len());
            let (userinfo, at) = match a[..line].iter().rposition(|&c| c == b'@') {
                Some(at) => (&a[..at], at + 1),
                None => (&a[..0], 0),
            };
            let bracketed = (a.get(at) == Some(&b'[')).then(|| {
                let inner = a[at + 1..]
                    .iter()
                    .position(|c| !(c.is_ascii_alphanumeric() || b":.".contains(c)))
                    .map_or(a.len(), |p| at + 1 + p);
                (a.get(inner) == Some(&b']')).then_some(inner + 1)
            });
            let host_end = match bracketed.flatten() {
                Some(end) => end,
                None => a[at..].iter().position(|&c| c == b':').map_or(a.len(), |p| at + p),
            };
            let host = &a[at..host_end];
            let port = (a.get(host_end) == Some(&b':')).then(|| {
                let digits = &a[host_end + 1..];
                &digits[..digits.iter().position(|c| !c.is_ascii_digit()).unwrap_or(digits.len())]
            });
            let host = if host.is_empty() {
                nil()
            } else {
                let start = host.iter().position(|&c| c != b'[').unwrap_or(host.len());
                let host = &host[start..];
                let end = host.iter().rposition(|&c| c != b']').map_or(0, |p| p + 1);
                bin(&host[..end])
            };
            let port = port
                .filter(|p| !p.is_empty())
                .map(|p| int(std::str::from_utf8(p).unwrap().parse::<BigInt>().unwrap()));
            (bin(a), if userinfo.is_empty() { nil() } else { bin(userinfo) }, host, port)
        }
    };
    let scheme = scheme.map(<[u8]>::to_ascii_lowercase);
    let port = port.unwrap_or_else(|| scheme.as_deref().map_or_else(nil, default_port));
    uri_struct([
        scheme.map_or_else(nil, Term::Bin),
        authority,
        userinfo,
        host,
        port,
        if path.is_empty() { nil() } else { bin(path) },
        opt_bin(query),
        opt_bin(fragment),
    ])
}

// ------------------------------------------------- URI.new via uri_string

enum Fail {
    /// `throw({error, invalid_uri, Part})`, with `Part` as `Kernel.to_string/1` gives it.
    Throw(Vec<u8>),
    /// Any other exception, which `uri_string:parse/1` does not catch.
    Crash,
}

#[derive(Clone, Default)]
struct UriMap {
    scheme: Option<Vec<u8>>,
    userinfo: Option<Vec<u8>>,
    host: Option<Vec<u8>>,
    /// `None` inside: the atom `undefined`.
    port: Option<Option<BigInt>>,
    path: Option<Vec<u8>>,
    query: Option<Vec<u8>>,
    fragment: Option<Vec<u8>>,
}

type Step<'a> = Result<(&'a [u8], UriMap), Fail>;

fn throw_char(c: u32) -> Fail {
    let c = char::from_u32(c).expect("decoded scalar");
    Fail::Throw(c.to_string().into_bytes())
}

/// `<<Char/utf8, Rest/binary>>`: `None` on an empty binary, `Err` when the
/// head is not a UTF-8 scalar (no clause matches).
fn next(s: &[u8]) -> Result<Option<(u32, &[u8])>, Fail> {
    let Some(&first) = s.first() else {
        return Ok(None);
    };
    let len = match first {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf7 => 4,
        _ => return Err(Fail::Crash),
    };
    let head = s.get(..len).ok_or(Fail::Crash)?;
    let c = std::str::from_utf8(head).map_err(|_| Fail::Crash)?.chars().next().unwrap();
    Ok(Some((c as u32, &s[len..])))
}

fn is_alpha(c: u32) -> bool {
    matches!(c, 0x41..=0x5a | 0x61..=0x7a)
}
fn is_digit(c: u32) -> bool {
    matches!(c, 0x30..=0x39)
}
fn is_hex(c: u32) -> bool {
    is_digit(c) || matches!(c, 0x41..=0x46 | 0x61..=0x66)
}
fn in_set(c: u32, set: &[u8]) -> bool {
    c < 0x80 && set.contains(&(c as u8))
}
fn is_unreserved(c: u32) -> bool {
    in_set(c, b"-._~") || is_alpha(c) || is_digit(c)
}
fn is_sub_delim(c: u32) -> bool {
    in_set(c, b"!$&'()*+,;=")
}
fn is_pchar(c: u32) -> bool {
    in_set(c, b"%:@") || is_unreserved(c) || is_sub_delim(c)
}
fn is_segment_nz_nc(c: u32) -> bool {
    in_set(c, b"%@") || is_unreserved(c) || is_sub_delim(c)
}
fn is_scheme(c: u32) -> bool {
    in_set(c, b"+-.") || is_alpha(c) || is_digit(c)
}
fn is_userinfo(c: u32) -> bool {
    in_set(c, b"%:") || is_unreserved(c) || is_sub_delim(c)
}
fn is_reg_name(c: u32) -> bool {
    c == u32::from(b'%') || is_unreserved(c) || is_sub_delim(c)
}
fn is_query(c: u32) -> bool {
    in_set(c, b"/?") || is_pchar(c)
}
fn is_ipv6(c: u32) -> bool {
    in_set(c, b":.") || is_hex(c)
}

fn strip_last_char(input: &[u8], chars: &[u8]) -> Vec<u8> {
    match input.last() {
        Some(c) if chars.contains(c) => input[..input.len() - 1].to_vec(),
        _ => input.to_vec(),
    }
}
/// `calculate_parsed_*/2`: the parsed prefix of `input` before `unparsed`
/// and the one-character delimiter that precedes it.
fn parsed(input: &[u8], unparsed: &[u8], strip: &[u8]) -> Vec<u8> {
    if unparsed.is_empty() {
        strip_last_char(input, strip)
    } else {
        input[..input.len() - unparsed.len() - 1].to_vec()
    }
}
const SCHEME: &[u8] = b":";
const PART: &[u8] = b"?#";
const USERINFO: &[u8] = b"?#@";
const HOST_PORT: &[u8] = b":?#/";
const QUERY_FRAGMENT: &[u8] = b"#";

fn prefixed(head: &[u8], tail: Vec<u8>) -> Vec<u8> {
    let mut out = head.to_vec();
    out.extend(tail);
    out
}
fn char_bytes(c: u32) -> Vec<u8> {
    char::from_u32(c).unwrap().to_string().into_bytes()
}

fn remove_brackets(host: Vec<u8>) -> Result<Vec<u8>, Fail> {
    match host.split_first() {
        Some((b'[', rest)) if rest.is_empty() => Err(Fail::Crash),
        Some((b'[', rest)) if rest.last() == Some(&b']') => Ok(rest[..rest.len() - 1].to_vec()),
        Some((b'[', rest)) => Ok(rest.to_vec()),
        _ => Ok(host),
    }
}

fn get_port(digits: Vec<u8>) -> Option<BigInt> {
    (!digits.is_empty()).then(|| std::str::from_utf8(&digits).unwrap().parse().unwrap())
}

fn maybe_add_path(mut uri: UriMap) -> UriMap {
    uri.path.get_or_insert_with(Vec::new);
    uri
}

fn uri_string_parse(s: &[u8]) -> Result<UriMap, Fail> {
    if s.is_empty() {
        return Ok(UriMap {
            path: Some(vec![]),
            ..UriMap::default()
        });
    }
    match parse_scheme_start(s) {
        Err(Fail::Throw(_)) => parse_relative_part(s),
        other => other,
    }
}

fn parse_scheme_start(s: &[u8]) -> Result<UriMap, Fail> {
    let (c, rest) = next(s)?.ok_or(Fail::Crash)?;
    if !is_alpha(c) {
        return Err(throw_char(c));
    }
    let (t, uri) = parse_scheme(rest, UriMap::default())?;
    let scheme = parsed(rest, t, SCHEME);
    let mut uri = maybe_add_path(uri);
    uri.scheme = Some(prefixed(&char_bytes(c), scheme));
    Ok(uri)
}

fn parse_scheme(s: &[u8], uri: UriMap) -> Step<'_> {
    if let Some(rest) = s.strip_prefix(b":") {
        let (_, uri) = parse_hier(rest, uri)?;
        return Ok((rest, uri));
    }
    match next(s)? {
        Some((c, rest)) if is_scheme(c) => parse_scheme(rest, uri),
        Some((c, _)) => Err(throw_char(c)),
        None => Err(Fail::Throw(vec![])),
    }
}

/// The shared `"//"` authority clause of `parse_hier/2` and `parse_relative_part/2`.
fn authority(rest: &[u8], uri: UriMap, add_path: bool) -> Result<UriMap, Fail> {
    match parse_userinfo(rest, uri.clone()) {
        Ok((t, uri)) => {
            let userinfo = parsed(rest, t, USERINFO);
            let mut uri = if add_path { maybe_add_path(uri) } else { uri };
            uri.userinfo = Some(userinfo);
            Ok(uri)
        }
        Err(Fail::Throw(_)) => {
            let (t, uri) = parse_host(rest, uri)?;
            let host = parsed(rest, t, HOST_PORT);
            let mut uri = if add_path { maybe_add_path(uri) } else { uri };
            uri.host = Some(remove_brackets(host)?);
            Ok(uri)
        }
        Err(crash) => Err(crash),
    }
}

/// The `$/`, `$?` and `$#` clauses shared by most states: they hand over to
/// the path, query or fragment parser and record that component.
fn delimiter<'a>(s: &'a [u8], uri: UriMap) -> Option<Step<'a>> {
    let (&first, rest) = s.split_first()?;
    Some(match first {
        b'/' => parse_segment(rest, uri).map(|(t, mut uri)| {
            uri.path = Some(prefixed(b"/", parsed(rest, t, PART)));
            (rest, uri)
        }),
        b'?' => parse_query(rest, uri).map(|(t, mut uri)| {
            uri.query = Some(parsed(rest, t, QUERY_FRAGMENT));
            (rest, uri)
        }),
        b'#' => parse_fragment(rest, uri).map(|(t, mut uri)| {
            uri.fragment = Some(parsed(rest, t, QUERY_FRAGMENT));
            (rest, uri)
        }),
        _ => return None,
    })
}

fn port_clause(rest: &[u8], uri: UriMap) -> Step<'_> {
    let (t, mut uri) = parse_port(rest, uri)?;
    uri.port = Some(get_port(parsed(rest, t, HOST_PORT)));
    Ok((rest, uri))
}

fn parse_hier(s: &[u8], uri: UriMap) -> Step<'_> {
    if let Some(rest) = s.strip_prefix(b"//") {
        return authority(rest, uri, false).map(|uri| (rest, uri));
    }
    if let Some(step) = delimiter(s, uri.clone()) {
        return step;
    }
    match next(s)? {
        Some((c, rest)) if is_pchar(c) => {
            let (t, mut uri) = parse_segment(rest, uri)?;
            uri.path = Some(prefixed(&char_bytes(c), parsed(rest, t, PART)));
            Ok((rest, uri))
        }
        Some((c, _)) => Err(throw_char(c)),
        None => Ok((&[], uri)),
    }
}

fn parse_relative_part(s: &[u8]) -> Result<UriMap, Fail> {
    let uri = UriMap::default();
    if let Some(rest) = s.strip_prefix(b"//") {
        return authority(rest, uri, true);
    }
    match s.first() {
        Some(b'/') => {
            let rest = &s[1..];
            let (t, mut uri) = parse_segment(rest, uri)?;
            uri.path = Some(prefixed(b"/", parsed(rest, t, PART)));
            Ok(uri)
        }
        Some(b'?' | b'#') => {
            let (_, uri) = delimiter(s, uri).unwrap()?;
            Ok(maybe_add_path(uri))
        }
        _ => match next(s)? {
            Some((c, rest)) if is_segment_nz_nc(c) => {
                let (t, mut uri) = parse_segment_nz_nc(rest, uri)?;
                uri.path = Some(prefixed(&char_bytes(c), parsed(rest, t, PART)));
                Ok(uri)
            }
            Some((c, _)) => Err(throw_char(c)),
            None => Err(Fail::Crash),
        },
    }
}

fn parse_segment(s: &[u8], uri: UriMap) -> Step<'_> {
    match s.first() {
        Some(b'/') => parse_segment(&s[1..], uri),
        Some(b'?' | b'#') => delimiter(s, uri).unwrap(),
        _ => match next(s)? {
            Some((c, rest)) if is_pchar(c) => parse_segment(rest, uri),
            Some((c, _)) => Err(throw_char(c)),
            None => Ok((&[], uri)),
        },
    }
}

fn parse_segment_nz_nc(s: &[u8], uri: UriMap) -> Step<'_> {
    match s.first() {
        Some(b'/') => parse_segment(&s[1..], uri),
        Some(b'?' | b'#') => delimiter(s, uri).unwrap(),
        _ => match next(s)? {
            Some((c, rest)) if is_segment_nz_nc(c) => parse_segment_nz_nc(rest, uri),
            Some((c, _)) => Err(throw_char(c)),
            None => Ok((&[], uri)),
        },
    }
}

fn parse_userinfo(s: &[u8], uri: UriMap) -> Step<'_> {
    if s == b"@" {
        let mut uri = uri;
        uri.host = Some(vec![]);
        return Ok((&[], uri));
    }
    if let Some(rest) = s.strip_prefix(b"@") {
        let (t, mut uri) = parse_host(rest, uri)?;
        uri.host = Some(remove_brackets(parsed(rest, t, HOST_PORT))?);
        return Ok((rest, uri));
    }
    match next(s)? {
        Some((c, rest)) if is_userinfo(c) => parse_userinfo(rest, uri),
        Some((c, _)) => Err(throw_char(c)),
        None => Err(Fail::Throw(vec![])),
    }
}

/// `parse_host/2`. Its IPv4 branch falls back to `parse_reg_name/2` on any
/// throw, and the reg-name grammar accepts every IPv4 character with the same
/// continuations, so both branches produce the same result.
fn parse_host(s: &[u8], uri: UriMap) -> Step<'_> {
    if let Some(rest) = s.strip_prefix(b":") {
        return port_clause(rest, uri);
    }
    if let Some(rest) = s.strip_prefix(b"[") {
        return parse_ipv6_bin(rest, vec![], uri);
    }
    parse_reg_name(s, uri)
}

fn parse_reg_name(s: &[u8], uri: UriMap) -> Step<'_> {
    if let Some(rest) = s.strip_prefix(b":") {
        return port_clause(rest, uri);
    }
    if let Some(step) = delimiter(s, uri.clone()) {
        return step;
    }
    match next(s)? {
        Some((c, rest)) if is_reg_name(c) => parse_reg_name(rest, uri),
        Some((c, _)) => Err(throw_char(c)),
        None => Ok((&[], uri)),
    }
}

fn parse_ipv6_bin(s: &[u8], mut acc: Vec<u8>, uri: UriMap) -> Step<'_> {
    if let Some(rest) = s.strip_prefix(b"]") {
        if !ipv6_strict(&acc) {
            return Err(Fail::Throw(acc));
        }
        return parse_ipv6_bin_end(rest, uri);
    }
    match next(s)? {
        Some((c, rest)) if is_ipv6(c) => {
            acc.push(c as u8);
            parse_ipv6_bin(rest, acc, uri)
        }
        Some((c, _)) => Err(throw_char(c)),
        None => Err(Fail::Throw(vec![])),
    }
}

fn parse_ipv6_bin_end(s: &[u8], uri: UriMap) -> Step<'_> {
    if let Some(rest) = s.strip_prefix(b":") {
        return port_clause(rest, uri);
    }
    if let Some(step) = delimiter(s, uri.clone()) {
        return step;
    }
    match next(s)? {
        Some((c, rest)) if is_ipv6(c) => parse_ipv6_bin_end(rest, uri),
        Some((c, _)) => Err(throw_char(c)),
        None => Ok((&[], uri)),
    }
}

fn parse_port(s: &[u8], uri: UriMap) -> Step<'_> {
    if let Some(step) = delimiter(s, uri.clone()) {
        return step;
    }
    match next(s)? {
        Some((c, rest)) if is_digit(c) => parse_port(rest, uri),
        Some((c, _)) => Err(throw_char(c)),
        None => Ok((&[], uri)),
    }
}

fn parse_query(s: &[u8], uri: UriMap) -> Step<'_> {
    if let Some(rest) = s.strip_prefix(b"#") {
        let (t, mut uri) = parse_fragment(rest, uri)?;
        uri.fragment = Some(parsed(rest, t, QUERY_FRAGMENT));
        return Ok((rest, uri));
    }
    match next(s)? {
        Some((c, rest)) if is_query(c) => parse_query(rest, uri),
        Some((c, _)) => Err(throw_char(c)),
        None => Ok((&[], uri)),
    }
}

fn parse_fragment(s: &[u8], uri: UriMap) -> Step<'_> {
    match next(s)? {
        Some((c, rest)) if is_query(c) => parse_fragment(rest, uri),
        Some((c, _)) => Err(throw_char(c)),
        None => Ok((&[], uri)),
    }
}

/// `inet_parse:ipv6strict_address/1` succeeds. The URI grammar only lets
/// hex digits, `:` and `.` through, so the scope-id clauses cannot apply.
fn ipv6_strict(cs: &[u8]) -> bool {
    fn hex(cs: &[u8]) -> Option<(&[u8], &[u8])> {
        let n = cs.iter().take(4).take_while(|c| c.is_ascii_hexdigit()).count();
        (n > 0).then(|| cs.split_at(n))
    }
    fn before(field: Option<(&[u8], &[u8])>, n: usize) -> Option<()> {
        let (digits, rest) = field?;
        match rest {
            [] if n == 7 => Some(()),
            b"::" if n <= 6 => Some(()),
            [b':', b':', tail @ ..] if n <= 5 => after(hex(tail), n + 1),
            [b':', tail @ ..] if n <= 6 => before(hex(tail), n + 1),
            [b'.', ..] if n == 6 => ipv4_strict(&[digits, rest].concat()),
            _ => None,
        }
    }
    fn after(field: Option<(&[u8], &[u8])>, n: usize) -> Option<()> {
        let (digits, rest) = field?;
        match rest {
            [] if n <= 6 => Some(()),
            [b':', tail @ ..] if n <= 5 => after(hex(tail), n + 1),
            [b'.', ..] if n <= 5 => ipv4_strict(&[digits, rest].concat()),
            _ => None,
        }
    }
    match cs {
        b"::" => true,
        [b':', b':', tail @ ..] => after(hex(tail), 0).is_some(),
        _ => before(hex(cs), 0).is_some(),
    }
}

/// `inet_parse:ipv4strict_address/1` succeeds: four dotted decimal fields
/// below 256 without leading zeros.
fn ipv4_strict(cs: &[u8]) -> Option<()> {
    let mut fields: Vec<u32> = vec![];
    let mut cs = cs;
    loop {
        if !cs.is_empty() && fields.len() == 4 {
            return None;
        }
        if cs == b"0" {
            fields.push(0);
            break;
        }
        if let Some(rest) = cs.strip_prefix(b"0.") {
            fields.push(0);
            cs = rest;
            continue;
        }
        if cs.first() == Some(&b'0') {
            return None;
        }
        let n = cs.iter().take(3).take_while(|&&c| c != b'.').count();
        let (digits, rest) = cs.split_at(n);
        if !rest.is_empty() && rest[0] != b'.' {
            return None;
        }
        if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
            return None;
        }
        fields.push(std::str::from_utf8(digits).unwrap().parse().unwrap());
        match rest {
            [] => break,
            [b'.', tail @ ..] if !tail.is_empty() => cs = tail,
            _ => return None,
        }
    }
    (fields.len() == 4 && fields.iter().all(|&d| d < 256)).then_some(())
}

/// `URI.uri_from_map/1` on a successful `uri_string:parse/1` result.
fn uri_from_map(map: UriMap) -> Term {
    let path = map.path.filter(|p| !p.is_empty());
    let port = match (&map.scheme, map.port) {
        (Some(_), Some(Some(port))) => int(port),
        (Some(scheme), _) => default_port(&scheme.to_ascii_lowercase()),
        (None, Some(Some(port))) => int(port),
        (None, _) => nil(),
    };
    uri_struct([
        map.scheme.map_or_else(nil, |s| Term::Bin(s.to_ascii_lowercase())),
        nil(),
        opt_bin(map.userinfo.as_deref()),
        opt_bin(map.host.as_deref()),
        port,
        opt_bin(path.as_deref()),
        opt_bin(map.query.as_deref()),
        opt_bin(map.fragment.as_deref()),
    ])
}

// ------------------------------------------------------ URI encode/decode

fn percent(out: &mut Vec<u8>, byte: u8, keep: fn(u8) -> bool) {
    if keep(byte) {
        out.push(byte);
    } else {
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        out.extend([b'%', HEX[usize::from(byte >> 4)], HEX[usize::from(byte & 15)]]);
    }
}
fn unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"~_-.".contains(&byte)
}
fn encode_www_form(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for &byte in s {
        if byte == b' ' {
            out.push(b'+');
        } else {
            percent(&mut out, byte, unreserved);
        }
    }
    out
}
fn encode_rfc3986(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for &byte in s {
        percent(&mut out, byte, unreserved);
    }
    out
}

/// `Kernel.to_string/1`; `Ok(None)` when it raises.
fn to_string(term: &Term) -> Result<Option<Vec<u8>>, String> {
    Ok(Some(match term {
        Term::Bin(b) => b.clone(),
        Term::Atom(a) if a == "nil" => vec![],
        Term::Atom(a) => a.clone().into_bytes(),
        Term::Int(i) => i.to_string().into_bytes(),
        Term::Float(_) => return Err("Float.to_string/1".into()),
        _ => return Ok(None),
    }))
}

fn encode_query(pairs: &Term, encoding: &Term) -> Eval {
    let Term::List(pairs) = pairs else {
        return Ok(None);
    };
    let mut out = Vec::new();
    for (i, pair) in pairs.iter().enumerate() {
        let Term::Tuple(kv) = pair else {
            return Ok(None);
        };
        let [key, value] = kv.as_slice() else {
            return Ok(None);
        };
        if matches!(key, Term::List(_)) || matches!(value, Term::List(_)) {
            return Ok(None);
        }
        // The encoding is only matched per pair: an empty list ignores it.
        let encode = match encoding {
            Term::Atom(e) if e == "www_form" => encode_www_form,
            Term::Atom(e) if e == "rfc3986" => encode_rfc3986,
            _ => return Ok(None),
        };
        let (Some(key), Some(value)) = (to_string(key)?, to_string(value)?) else {
            return Ok(None);
        };
        if i > 0 {
            out.push(b'&');
        }
        out.extend(encode(&key));
        out.push(b'=');
        out.extend(encode(&value));
    }
    Ok(Some(Term::Bin(out)))
}

fn unpercent(s: &[u8], spaces: bool) -> Vec<u8> {
    let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            b'+' if spaces => {
                out.push(b' ');
                i += 1;
            }
            b'%' => match (s.get(i + 1).and_then(|&c| hex(c)), s.get(i + 2).and_then(|&c| hex(c))) {
                (Some(high), Some(low)) => {
                    out.push(high << 4 | low);
                    i += 3;
                }
                _ => {
                    out.push(b'%');
                    i += 1;
                }
            },
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    out
}

// ------------------------------------------------------------------ Version

/// `Version.Parser.parse_version/2`: `{major, minor, patch, pre, build_parts}`.
type Matchable = (BigInt, BigInt, Option<BigInt>, Vec<Term>, Vec<Vec<u8>>);

fn split_once<'a>(s: &'a [u8], at: u8) -> (&'a [u8], Option<&'a [u8]>) {
    match s.iter().position(|&c| c == at) {
        Some(p) => (&s[..p], Some(&s[p + 1..])),
        None => (s, None),
    }
}
fn leading_zero(s: &[u8]) -> bool {
    s.len() >= 2 && s[0] == b'0'
}
fn digits(s: &[u8]) -> Option<BigInt> {
    (!s.is_empty() && s.iter().all(u8::is_ascii_digit))
        .then(|| std::str::from_utf8(s).unwrap().parse().unwrap())
}
fn require_digits(s: Option<&[u8]>) -> Option<BigInt> {
    let s = s?;
    if leading_zero(s) { None } else { digits(s) }
}
fn dot_separated(s: Option<&[u8]>) -> Option<Vec<&[u8]>> {
    let Some(s) = s else {
        return Some(vec![]);
    };
    let parts: Vec<&[u8]> = s.split(|&c| c == b'.').collect();
    parts
        .iter()
        .all(|p| !p.is_empty() && p.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'-'))
        .then_some(parts)
}

fn parse_version(s: &[u8], approximate: bool) -> Option<Matchable> {
    let (with_pre, build) = split_once(s, b'+');
    let (version, pre) = split_once(with_pre, b'-');
    let parts: Vec<&[u8]> = version.split(|&c| c == b'.').collect();
    if parts.len() > 3 {
        return None;
    }
    let major = require_digits(parts.first().copied())?;
    let minor = require_digits(parts.get(1).copied())?;
    let patch = match parts.get(2) {
        None if approximate => None,
        patch => Some(require_digits(patch.copied())?),
    };
    let mut pre_parts = vec![];
    for part in dot_separated(pre)? {
        match digits(part) {
            Some(_) if leading_zero(part) => return None,
            Some(n) => pre_parts.push(Term::Int(n)),
            None => pre_parts.push(bin(part)),
        }
    }
    let build = dot_separated(build)?.into_iter().map(<[u8]>::to_vec).collect();
    Some((major, minor, patch, pre_parts, build))
}

fn version_parse(s: &[u8]) -> Option<Term> {
    let (major, minor, patch, pre, build) = parse_version(s, false)?;
    let build = if build.is_empty() { nil() } else { Term::Bin(build.join(&b'.')) };
    Some(structure(
        "Elixir.Version",
        vec![
            ("major", Term::Int(major)),
            ("minor", Term::Int(minor)),
            ("patch", patch.map_or_else(nil, Term::Int)),
            ("pre", Term::List(pre)),
            ("build", build),
        ],
    ))
}

enum Requirement {
    Ok(Term),
    Error,
    Crash,
}

#[derive(Clone)]
enum Token {
    Op(&'static str),
    Version(Vec<u8>),
}

fn lex(s: &[u8]) -> Option<Vec<Token>> {
    const OPERATORS: [(&str, &str); 10] = [
        (">=", ">="),
        ("<=", "<="),
        ("~>", "~>"),
        (">", ">"),
        ("<", "<"),
        ("==", "=="),
        (" or ", "or"),
        (" and ", "and"),
        ("!=", "!="),
        ("!", "!="),
    ];
    fn flush(buffer: &mut Vec<u8>, acc: &mut Vec<Token>) {
        if buffer.is_empty() {
            return;
        }
        if !matches!(acc.last(), Some(Token::Op(op)) if *op != "and" && *op != "or") {
            acc.push(Token::Op("=="));
        }
        acc.push(Token::Version(std::mem::take(buffer)));
    }
    let (mut acc, mut buffer, mut s) = (vec![], vec![], s);
    'outer: while !s.is_empty() {
        for (text, op) in OPERATORS {
            if let Some(rest) = s.strip_prefix(text.as_bytes()) {
                flush(&mut buffer, &mut acc);
                acc.push(Token::Op(op));
                s = rest;
                continue 'outer;
            }
        }
        if let Some(rest) = s.strip_prefix(b" ") {
            flush(&mut buffer, &mut acc);
            s = rest;
            continue;
        }
        let (_, rest) = next(s).ok()??;
        buffer.extend_from_slice(&s[..s.len() - rest.len()]);
        s = rest;
    }
    flush(&mut buffer, &mut acc);
    Some(acc)
}

fn matchable((major, minor, patch, pre, build): Matchable) -> Term {
    Term::Tuple(vec![
        Term::Int(major),
        Term::Int(minor),
        patch.map_or_else(nil, Term::Int),
        Term::List(pre),
        Term::List(build.into_iter().map(Term::Bin).collect()),
    ])
}

fn requirement(source: &[u8]) -> Requirement {
    let Some(tokens) = lex(source) else {
        return Requirement::Crash;
    };
    // `revert_lexed/2` walks the lexer's reversed accumulator from its head.
    let mut lexed = vec![];
    let mut rest: Vec<Token> = tokens;
    loop {
        let n = rest.len();
        let validate = |op: &str, version: &[u8]| {
            parse_version(version, true).filter(|v| op == "~>" || v.2.is_some())
        };
        match (rest.get(n.wrapping_sub(1)), rest.get(n.wrapping_sub(2)), rest.get(n.wrapping_sub(3))) {
            (Some(Token::Version(v)), Some(Token::Op(op)), Some(Token::Op(cond)))
                if n >= 3 && matches!(*cond, "or" | "and") =>
            {
                let Some(version) = validate(op, v) else {
                    return Requirement::Error;
                };
                lexed.splice(0..0, [atom(cond), atom(op), matchable(version)]);
                rest.truncate(n - 3);
            }
            (Some(Token::Version(v)), Some(Token::Op(op)), _) if n == 2 => {
                let Some(version) = validate(op, v) else {
                    return Requirement::Error;
                };
                lexed.splice(0..0, [atom(op), matchable(version)]);
                return Requirement::Ok(structure(
                    "Elixir.Version.Requirement",
                    vec![("source", bin(source)), ("lexed", Term::List(lexed))],
                ));
            }
            _ => return Requirement::Error,
        }
    }
}

// ----------------------------------------------------------------- Duration

fn duration_new(pairs: &Term) -> Option<Term> {
    const UNITS: [&str; 7] = ["year", "month", "week", "day", "hour", "minute", "second"];
    let Term::List(pairs) = pairs else {
        return None;
    };
    let mut fields: Vec<(&str, Term)> = UNITS.iter().map(|u| (*u, int(0))).collect();
    fields.push(("microsecond", Term::Tuple(vec![int(0), int(0)])));
    for pair in pairs {
        let Term::Tuple(kv) = pair else { return None };
        let [Term::Atom(unit), value] = kv.as_slice() else {
            return None;
        };
        let valid = if unit == "microsecond" {
            matches!(value, Term::Tuple(t) if matches!(t.as_slice(),
                [Term::Int(_), Term::Int(p)] if *p >= BigInt::from(0) && *p <= BigInt::from(6)))
        } else {
            UNITS.contains(&unit.as_str()) && matches!(value, Term::Int(_))
        };
        if !valid {
            return None;
        }
        fields.iter_mut().find(|(k, _)| k == unit).unwrap().1 = value.clone();
    }
    Some(structure("Elixir.Duration", fields))
}

// ------------------------------------------------------------- String.split

/// White_Space codepoints except the non-breaking ones, as `String.Break`.
const WHITESPACE: [u32; 22] = [
    0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x20, 0x85, 0x1680, 0x2000, 0x2001, 0x2002, 0x2003, 0x2004,
    0x2005, 0x2006, 0x2008, 0x2009, 0x200a, 0x2028, 0x2029, 0x205f, 0x3000,
];

fn split_whitespace(s: &[u8]) -> Term {
    let patterns: Vec<Vec<u8>> = WHITESPACE.iter().map(|&c| char_bytes(c)).collect();
    let parts = binary_split(s, &patterns, true, true);
    Term::List(parts.into_iter().map(|p| bin(p)).collect())
}

/// `:binary.split/3`: leftmost match, longest pattern at that position.
fn binary_split<'a>(s: &'a [u8], patterns: &[Vec<u8>], global: bool, trim_all: bool) -> Vec<&'a [u8]> {
    let mut parts = vec![];
    let mut start = 0;
    let mut i = 0;
    while i < s.len() {
        let found = patterns.iter().filter(|p| s[i..].starts_with(p)).map(Vec::len).max();
        if let Some(len) = found {
            parts.push(&s[start..i]);
            i += len;
            start = i;
            if !global {
                break;
            }
        } else {
            i += 1;
        }
    }
    parts.push(&s[start..]);
    if trim_all {
        parts.retain(|p| !p.is_empty());
    }
    parts
}

/// `Keyword.get/3`; `Err` when it raises.
fn keyword_get<'a>(options: &'a [Term], key: &str, default: &'a Term) -> Result<&'a Term, ()> {
    for option in options {
        if let Term::Tuple(items) = option
            && matches!(items.first(), Some(Term::Atom(k)) if k == key)
        {
            return match items.as_slice() {
                [_, value] => Ok(value),
                _ => Err(()),
            };
        }
    }
    Ok(default)
}

fn graphemes(s: &[u8]) -> Vec<Vec<u8>> {
    use unicode_segmentation::UnicodeSegmentation;
    let mut units = vec![];
    let mut remaining = s;
    while !remaining.is_empty() {
        let valid_up_to = std::str::from_utf8(remaining).map_or_else(|e| e.valid_up_to(), |_| remaining.len());
        let valid = std::str::from_utf8(&remaining[..valid_up_to]).unwrap();
        units.extend(valid.graphemes(true).map(|g| g.as_bytes().to_vec()));
        match remaining[valid_up_to..].split_first() {
            Some((&invalid, tail)) => {
                units.push(vec![invalid]);
                remaining = tail;
            }
            None => break,
        }
    }
    units
}

fn split(s: &[u8], pattern: &Term, options: &Term) -> Eval {
    let Term::List(options) = options else {
        return Ok(None);
    };
    let infinity = atom("infinity");
    let no = atom("false");
    if matches!(pattern, Term::List(p) if p.is_empty()) {
        // `string == "" and Keyword.get(options, :trim, false)`: options are
        // only read for an empty string, and any truthy value trims.
        if !s.is_empty() {
            return Ok(Some(Term::List(vec![bin(s)])));
        }
        return Ok(match keyword_get(options, "trim", &no) {
            Ok(Term::Atom(a)) if a == "false" || a == "nil" => Some(Term::List(vec![bin(s)])),
            Ok(_) => Some(Term::List(vec![])),
            Err(()) => None,
        });
    }
    let (Ok(parts), Ok(trim)) = (keyword_get(options, "parts", &infinity), keyword_get(options, "trim", &no))
    else {
        return Ok(None);
    };
    // parts_to_index/1
    let index = |parts: &Term| -> Option<BigInt> {
        match parts {
            Term::Atom(a) if a == "infinity" => Some(BigInt::from(0)),
            Term::Int(n) if *n > BigInt::from(0) => Some(n.clone()),
            _ => None,
        }
    };
    let is_true = matches!(trim, Term::Atom(a) if a == "true");
    let one = BigInt::from(1);
    let strings = |parts: Vec<&[u8]>| Some(Term::List(parts.into_iter().map(bin).collect()));
    match pattern {
        Term::Bin(p) if p.is_empty() => {
            let Some(index) = index(parts) else {
                return Ok(None);
            };
            let units = graphemes(s);
            // split_empty/3, counting down from `count`.
            let split_empty = |count: BigInt| -> Vec<Vec<u8>> {
                let mut out = vec![];
                let mut rest = &units[..];
                let mut count = count;
                loop {
                    if count == one {
                        let tail: Vec<u8> = rest.concat();
                        if !(tail.is_empty() && is_true) {
                            out.push(tail);
                        }
                        return out;
                    }
                    match rest.split_first() {
                        Some((unit, tail)) => {
                            out.push(unit.clone());
                            rest = tail;
                            count -= 1;
                        }
                        None => count = one.clone(),
                    }
                }
            };
            let out = if matches!(trim, Term::Atom(a) if a == "false") && index != one {
                let mut out = vec![vec![]];
                out.extend(split_empty(index - 1));
                out
            } else {
                split_empty(index)
            };
            Ok(Some(Term::List(out.into_iter().map(Term::Bin).collect())))
        }
        Term::Bin(_) | Term::List(_) => {
            let patterns: Option<Vec<Vec<u8>>> = match pattern {
                Term::Bin(p) => Some(vec![p.clone()]),
                Term::List(ps) => ps
                    .iter()
                    .map(|p| match p {
                        Term::Bin(p) if !p.is_empty() => Some(p.clone()),
                        _ => None,
                    })
                    .collect(),
                _ => None,
            };
            let Some(patterns) = patterns else {
                return Ok(None);
            };
            let parts_infinity = matches!(parts, Term::Atom(a) if a == "infinity");
            match (parts_infinity, trim) {
                (true, Term::Atom(t)) if t == "false" => Ok(strings(binary_split(s, &patterns, true, false))),
                (true, Term::Atom(t)) if t == "true" => Ok(strings(binary_split(s, &patterns, true, true))),
                (false, Term::Atom(t)) if t == "false" && *parts == int(2) => {
                    Ok(strings(binary_split(s, &patterns, false, false)))
                }
                _ => {
                    let Some(count) = index(parts) else {
                        return Ok(None);
                    };
                    Ok(strings(split_each(s, &patterns, is_true, count)))
                }
            }
        }
        _ => Ok(None),
    }
}

/// `split_each/4` with `do_splitter/3`.
fn split_each<'a>(s: &'a [u8], patterns: &[Vec<u8>], trim: bool, count: BigInt) -> Vec<&'a [u8]> {
    let one = BigInt::from(1);
    let mut out = vec![];
    let mut rest = Some(s);
    let mut count = count;
    loop {
        let Some(string) = rest else { return out };
        if count == one {
            if !(string.is_empty() && trim) {
                out.push(string);
            }
            return out;
        }
        // do_splitter/3
        let mut bin = string;
        let split = loop {
            if bin.is_empty() {
                break if trim { None } else { Some((bin, None)) };
            }
            match binary_split(bin, patterns, false, false).as_slice() {
                [first, second] if first.is_empty() && trim => bin = second,
                [first, second] => break Some((*first, Some(*second))),
                [first] => break Some((*first, None)),
                _ => unreachable!(),
            }
        };
        match split {
            Some((head, tail)) => {
                out.push(head);
                rest = tail;
                count -= 1;
            }
            None => return out,
        }
    }
}
