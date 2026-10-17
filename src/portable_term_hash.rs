use crate::{native_dict::pinned_hash_atom, native_runtime_hash::NativeHashKey as K};
use num_traits::ToPrimitive;
fn mix(mut a: u32, mut b: u32, mut c: u32) -> (u32, u32, u32) {
    a = a.wrapping_sub(b).wrapping_sub(c) ^ (c >> 13);
    b = b.wrapping_sub(c).wrapping_sub(a) ^ (a << 8);
    c = c.wrapping_sub(a).wrapping_sub(b) ^ (b >> 13);
    a = a.wrapping_sub(b).wrapping_sub(c) ^ (c >> 12);
    b = b.wrapping_sub(c).wrapping_sub(a) ^ (a << 16);
    c = c.wrapping_sub(a).wrapping_sub(b) ^ (b >> 5);
    a = a.wrapping_sub(b).wrapping_sub(c) ^ (c >> 3);
    b = b.wrapping_sub(c).wrapping_sub(a) ^ (a << 10);
    c = c.wrapping_sub(a).wrapping_sub(b) ^ (b >> 15);
    (a, b, c)
}
fn hash2(a: u32, b: u32, constant: u32, h: u32) -> u32 {
    mix(constant.wrapping_add(a), constant.wrapping_add(b), h).2
}
fn block(bytes: &[u8], initial: u32) -> u32 {
    let (mut a, mut b, mut c) = (0x9e3779b9u32, 0x9e3779b9u32, initial);
    let mut chunks = bytes.chunks_exact(12);
    for bytes in &mut chunks {
        a = a.wrapping_add(u32::from_le_bytes(bytes[0..4].try_into().unwrap()));
        b = b.wrapping_add(u32::from_le_bytes(bytes[4..8].try_into().unwrap()));
        c = c.wrapping_add(u32::from_le_bytes(bytes[8..12].try_into().unwrap()));
        (a, b, c) = mix(a, b, c);
    }
    c = c.wrapping_add(bytes.len() as u32);
    for (i, byte) in chunks.remainder().iter().enumerate() {
        match i {
            0..=3 => a = a.wrapping_add((*byte as u32) << (8 * i)),
            4..=7 => b = b.wrapping_add((*byte as u32) << (8 * (i - 4))),
            _ => c = c.wrapping_add((*byte as u32) << (8 * (i - 7))),
        }
    }
    mix(a, b, c).2
}
use crate::native_runtime_hash::{NativeAtomLayout, RuntimeHashError};

pub fn hash(key: &K) -> Result<u32, RuntimeHashError> {
    hash_in(key, None)
}

pub fn hash_with_layout(key: &K, layout: &NativeAtomLayout) -> Result<u32, RuntimeHashError> {
    hash_in(key, Some(layout))
}

fn hash_in(key: &K, layout: Option<&NativeAtomLayout>) -> Result<u32, RuntimeHashError> {
    enum Work<'a> {
        Key(&'a K),
        Pair,
        Tail(u32, u32),
    }
    let mut work = vec![Work::Key(key)];
    let mut h = 0u32;
    let mut xor = 0u32;
    while let Some(next) = work.pop() {
        match next {
            Work::Pair => {
                xor ^= h;
                h = 0;
            }
            Work::Tail(saved, outer) => {
                h = hash2(xor, 0, 0xbe1e08bb, saved);
                xor = outer;
            }
            Work::Key(key) => match key {
                #[cfg(otp_domain = "29")]
                K::Record { module,name,exported,fields } => {
                    h = hash2(fields.len() as u32, record_definition_hash(module,name,*exported,fields), 0x36fbef9f, h);
                    let layout = layout.ok_or(RuntimeHashError::UnavailableAtomLayout)?;
                    let mut fields = fields.iter().map(|(key,value)| Ok((layout.atom_identity(key)?,value)))
                        .collect::<Result<Vec<_>,RuntimeHashError>>()?;
                    fields.sort_by_key(|(identity,_)| *identity);
                    work.extend(fields.into_iter().rev().map(|(_,value)| Work::Key(value)));
                }
                K::ExternalFun {
                    module,
                    function,
                    arity,
                } => {
                    h = hash2(u32::from(*arity), pinned_hash_atom(module), 0x9e3779b9, h);
                    h = hash2(pinned_hash_atom(function), 0, 0xa708a81e, h);
                }
                K::Atom(name) => {
                    let atom = pinned_hash_atom(name);
                    h = if h == 0 {
                        atom
                    } else {
                        hash2(atom, 0, 0xdaa66d2b, h)
                    };
                }
                K::Nil => {
                    h = if h == 0 {
                        3468870702
                    } else {
                        hash2(2, 0, 0x3c6ef372, h)
                    }
                }
                K::Integer(n) => {
                    if let Some(n) = n.to_i32().filter(|n| (-134217728..134217728).contains(n)) {
                        if n < 0 {
                            h = hash2(n.unsigned_abs(), 0, 0x9e3779b9, h);
                        }
                        h = hash2(n as u32, 0, 0x9e3779b9, h);
                    } else {
                        let (sign, mut bytes) = n.to_bytes_le();
                        bytes.resize(bytes.len().max(1).div_ceil(8) * 8, 0);
                        let constant = if sign == num_bigint::Sign::Minus {
                            0x2e2ac13a
                        } else {
                            0xcc623af3
                        };
                        for part in bytes.chunks_exact(8) {
                            h = hash2(
                                u32::from_le_bytes(part[0..4].try_into().unwrap()),
                                u32::from_le_bytes(part[4..8].try_into().unwrap()),
                                constant,
                                h,
                            );
                        }
                    }
                }
                K::Float(bits) => {
                    let bits = if f64::from_bits(*bits) == 0.0 {
                        0
                    } else {
                        *bits
                    };
                    h = hash2((bits >> 32) as u32, bits as u32, 0x6a99b4ac, h);
                }
                K::Bits { bytes, bits } => {
                    let size = (*bits / 8) as usize;
                    let tail = (*bits % 8) as u32;
                    let initial = 0x08d12e65u32.wrapping_add(h);
                    h = if size == 0 && tail == 0 {
                        initial
                    } else {
                        block(&bytes[..size], initial)
                    };
                    if tail > 0 {
                        h = hash2(tail, (bytes[size] >> (8 - tail)) as u32, 0x454021d7, h);
                    }
                }
                K::Tuple(fields) => {
                    h = hash2(fields.len() as u32, 0, 0x8ff34781, h);
                    work.extend(fields.iter().rev().map(Work::Key));
                }
                K::Map(fields) => {
                    h = hash2(fields.len() as u32, 0, 0xe3779b90, h);
                    if !fields.is_empty() {
                        work.push(Work::Tail(h, xor));
                        h = 0;
                        xor = 0;
                        for (k, v) in fields.iter().rev() {
                            work.push(Work::Pair);
                            work.push(Work::Key(v));
                            work.push(Work::Key(k));
                        }
                    }
                }
                K::Cons(_, _) => {
                    let mut next = key;
                    let mut count = 0;
                    let mut packed = 0u32;
                    while let K::Cons(head, tail) = next {
                        let K::Integer(n) = head.as_ref() else { break };
                        let Some(byte) = n.to_u8() else { break };
                        packed = (packed << 8) + byte as u32;
                        count += 1;
                        if count == 4 {
                            h = hash2(packed, 0, 0x78dde6e4, h);
                            count = 0;
                            packed = 0;
                        }
                        next = tail;
                    }
                    if count > 0 {
                        h = hash2(packed, 0, 0x78dde6e4, h);
                    }
                    if let K::Cons(head, tail) = next {
                        work.push(Work::Key(tail));
                        work.push(Work::Key(head));
                    } else {
                        work.push(Work::Key(next));
                    }
                }
            },
        }
    }
    Ok(h)
}

#[cfg(otp_domain = "29")]
pub(crate) fn record_definition_hash(module: &str, name: &str, exported: bool, fields: &[(String,K)]) -> u32 {
    let mut parts = vec![K::Atom(module.into()),K::Atom(name.into()),K::Atom(exported.to_string())];
    parts.extend(fields.iter().map(|(k,_)| K::Atom(k.clone())));
    hash(&K::Tuple(parts)).expect("record definition hash contains only atoms")
}
