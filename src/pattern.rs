//! Compiled MATCH patterns and the pattern-matrix algorithms (Maranget,
//! "Warnings for pattern matching"): `useful` decides whether a row adds
//! anything to a matrix (unreachable arms, exhaustiveness), `witnesses` lists
//! the values no row matches.

use std::collections::{HashMap, HashSet};

use crate::check::{EnumDef, PartT, RecordDef, Type, subst};
use crate::eval::Value;

/// A resolved pattern. `Any` matches everything (and binds it when named);
/// `Ctor` is an enum variant with one sub-pattern per parameter slot.
#[derive(Debug, Clone, PartialEq)]
pub enum Pat {
    Any(Option<String>),
    Ctor(usize, Vec<Pat>),
}

type Enums = HashMap<String, EnumDef>;

/// The type declarations patterns are checked against. A record is a type
/// with one constructor (index 0) whose arguments are all its slots.
pub struct Sig<'a> {
    pub enums: &'a Enums,
    pub records: &'a HashMap<String, RecordDef>,
}

pub fn enum_params(enums: &Enums, en: &str, targs: &[Type]) -> HashMap<String, Type> {
    enums[en]
        .params
        .iter()
        .cloned()
        .zip(targs.iter().cloned())
        .collect()
}

/// Types of the parameter slots of variant `c` of `ty`.
fn fields(sig: &Sig, ty: &Type, c: usize) -> Vec<Type> {
    if let Type::Record(rn) = ty {
        return sig.records[rn]
            .fields
            .iter()
            .flat_map(|f| f.slots.iter().map(|s| s.ty.clone()))
            .collect();
    }
    let Type::Enum(en, targs) = ty else {
        return vec![];
    };
    let pmap = enum_params(sig.enums, en, targs);
    sig.enums[en].variants[c]
        .parts
        .iter()
        .filter_map(|p| match p {
            PartT::Param(_, t) => Some(subst(t, &pmap, false)),
            _ => None,
        })
        .collect()
}

fn variant_count(sig: &Sig, ty: &Type) -> usize {
    match ty {
        Type::Enum(en, _) => sig.enums[en].variants.len(),
        Type::Record(_) => 1,
        _ => 0,
    }
}

fn wilds(n: usize) -> Vec<Pat> {
    vec![Pat::Any(None); n]
}

fn concat<T: Clone>(a: &[T], b: &[T]) -> Vec<T> {
    a.iter().chain(b).cloned().collect()
}

fn specialize(rows: &[Vec<Pat>], c: usize, arity: usize) -> Vec<Vec<Pat>> {
    rows.iter()
        .filter_map(|row| match &row[0] {
            Pat::Ctor(d, args) if *d == c => Some(concat(args, &row[1..])),
            Pat::Ctor(..) => None,
            Pat::Any(_) => Some(concat(&wilds(arity), &row[1..])),
        })
        .collect()
}

fn default_rows(rows: &[Vec<Pat>]) -> Vec<Vec<Pat>> {
    rows.iter()
        .filter(|row| matches!(row[0], Pat::Any(_)))
        .map(|row| row[1..].to_vec())
        .collect()
}

fn used_ctors(rows: &[Vec<Pat>]) -> HashSet<usize> {
    rows.iter()
        .filter_map(|row| match &row[0] {
            Pat::Ctor(c, _) => Some(*c),
            Pat::Any(_) => None,
        })
        .collect()
}

/// Is there a value vector matched by `q` and by no row of `rows`?
pub fn useful(sig: &Sig, rows: &[Vec<Pat>], q: &[Pat], tys: &[Type]) -> bool {
    if q.is_empty() {
        return rows.is_empty();
    }
    match &q[0] {
        Pat::Ctor(c, args) => {
            let fs = fields(sig, &tys[0], *c);
            let rows = specialize(rows, *c, fs.len());
            useful(sig, &rows, &concat(args, &q[1..]), &concat(&fs, &tys[1..]))
        }
        Pat::Any(_) => {
            let n = variant_count(sig, &tys[0]);
            if n > 0 && used_ctors(rows).len() == n {
                (0..n).any(|c| {
                    let fs = fields(sig, &tys[0], c);
                    let rows = specialize(rows, c, fs.len());
                    useful(
                        sig,
                        &rows,
                        &concat(&wilds(fs.len()), &q[1..]),
                        &concat(&fs, &tys[1..]),
                    )
                })
            } else {
                useful(sig, &default_rows(rows), &q[1..], &tys[1..])
            }
        }
    }
}

/// Every value vector (as a pattern vector) that no row matches.
pub fn witnesses(sig: &Sig, rows: &[Vec<Pat>], tys: &[Type]) -> Vec<Vec<Pat>> {
    if tys.is_empty() {
        return if rows.is_empty() {
            vec![vec![]]
        } else {
            vec![]
        };
    }
    let n = variant_count(sig, &tys[0]);
    let used = used_ctors(rows);
    let mut out = Vec::new();
    if n > 0 && used.len() == n {
        for c in 0..n {
            let fs = fields(sig, &tys[0], c);
            let rows = specialize(rows, c, fs.len());
            for w in witnesses(sig, &rows, &concat(&fs, &tys[1..])) {
                let (args, rest) = w.split_at(fs.len());
                out.push(concat(&[Pat::Ctor(c, args.to_vec())], rest));
            }
        }
        return out;
    }
    let rest = witnesses(sig, &default_rows(rows), &tys[1..]);
    if n > 0 && (!used.is_empty() || rows.is_empty()) {
        for c in (0..n).filter(|c| !used.contains(c)) {
            let arity = fields(sig, &tys[0], c).len();
            for w in &rest {
                out.push(concat(&[Pat::Ctor(c, wilds(arity))], w));
            }
        }
    } else {
        for w in &rest {
            out.push(concat(&[Pat::Any(None)], w));
        }
    }
    out
}

/// A pattern as source text, e.g. `match (glob)` or `(_) by (_)`.
pub fn render(sig: &Sig, ty: &Type, pat: &Pat) -> String {
    let Pat::Ctor(c, args) = pat else {
        return "_".into();
    };
    let fs = fields(sig, ty, *c);
    let mut args = args.iter().zip(&fs);
    if let Type::Record(rn) = ty {
        let fields: Vec<String> = sig.records[rn]
            .fields
            .iter()
            .map(|f| {
                f.key
                    .split(' ')
                    .map(|w| match w {
                        "_" => args
                            .next()
                            .map_or("(_)".into(), |(a, t)| format!("({})", render(sig, t, a))),
                        w => w.to_string(),
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect();
        return format!("{{ {} }}", fields.join(", "));
    }
    let Type::Enum(en, _) = ty else {
        return "_".into();
    };
    sig.enums[en].variants[*c]
        .parts
        .iter()
        .filter_map(|p| match p {
            PartT::Word(w) => Some(w.clone()),
            PartT::Param(..) => args.next().map(|(a, t)| format!("({})", render(sig, t, a))),
            PartT::Tuple(_) => Some("(_)".into()),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Matches `v` against `pat`, appending bindings in left-to-right order.
pub fn bind(pat: &Pat, v: &Value, out: &mut Vec<(String, Value)>) -> bool {
    match pat {
        Pat::Any(name) => {
            if let Some(n) = name {
                out.push((n.clone(), v.clone()));
            }
            true
        }
        Pat::Ctor(c, subs) => match v {
            Value::Variant(_, idx, args) => {
                idx == c && subs.iter().zip(args).all(|(p, a)| bind(p, a, out))
            }
            Value::Record(_, vals) => subs.iter().zip(vals).all(|(p, a)| bind(p, a, out)),
            _ => false,
        },
    }
}
