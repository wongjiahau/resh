//! Compiled MATCH patterns and the pattern-matrix algorithms (Maranget,
//! "Warnings for pattern matching"): `useful` decides whether a row adds
//! anything to a matrix (unreachable arms, exhaustiveness), `witnesses` lists
//! the values no row matches.

use std::collections::{HashMap, HashSet};

use crate::check::{EnumDef, PartT, Type, subst};
use crate::eval::Value;

/// A resolved pattern. `Any` matches everything (and binds it when named);
/// `Ctor` is an enum variant with one sub-pattern per parameter slot.
#[derive(Debug, Clone, PartialEq)]
pub enum Pat {
    Any(Option<String>),
    Ctor(usize, Vec<Pat>),
}

type Enums = HashMap<String, EnumDef>;

pub fn enum_params(enums: &Enums, en: &str, targs: &[Type]) -> HashMap<String, Type> {
    enums[en]
        .params
        .iter()
        .cloned()
        .zip(targs.iter().cloned())
        .collect()
}

/// Types of the parameter slots of variant `c` of `ty`.
fn fields(enums: &Enums, ty: &Type, c: usize) -> Vec<Type> {
    let Type::Enum(en, targs) = ty else {
        return vec![];
    };
    let pmap = enum_params(enums, en, targs);
    enums[en].variants[c]
        .parts
        .iter()
        .filter_map(|p| match p {
            PartT::Param(_, t) => Some(subst(t, &pmap, false)),
            _ => None,
        })
        .collect()
}

fn variant_count(enums: &Enums, ty: &Type) -> usize {
    match ty {
        Type::Enum(en, _) => enums[en].variants.len(),
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
pub fn useful(enums: &Enums, rows: &[Vec<Pat>], q: &[Pat], tys: &[Type]) -> bool {
    if q.is_empty() {
        return rows.is_empty();
    }
    match &q[0] {
        Pat::Ctor(c, args) => {
            let fs = fields(enums, &tys[0], *c);
            let rows = specialize(rows, *c, fs.len());
            useful(
                enums,
                &rows,
                &concat(args, &q[1..]),
                &concat(&fs, &tys[1..]),
            )
        }
        Pat::Any(_) => {
            let n = variant_count(enums, &tys[0]);
            if n > 0 && used_ctors(rows).len() == n {
                (0..n).any(|c| {
                    let fs = fields(enums, &tys[0], c);
                    let rows = specialize(rows, c, fs.len());
                    useful(
                        enums,
                        &rows,
                        &concat(&wilds(fs.len()), &q[1..]),
                        &concat(&fs, &tys[1..]),
                    )
                })
            } else {
                useful(enums, &default_rows(rows), &q[1..], &tys[1..])
            }
        }
    }
}

/// Every value vector (as a pattern vector) that no row matches.
pub fn witnesses(enums: &Enums, rows: &[Vec<Pat>], tys: &[Type]) -> Vec<Vec<Pat>> {
    if tys.is_empty() {
        return if rows.is_empty() { vec![vec![]] } else { vec![] };
    }
    let n = variant_count(enums, &tys[0]);
    let used = used_ctors(rows);
    let mut out = Vec::new();
    if n > 0 && used.len() == n {
        for c in 0..n {
            let fs = fields(enums, &tys[0], c);
            let rows = specialize(rows, c, fs.len());
            for w in witnesses(enums, &rows, &concat(&fs, &tys[1..])) {
                let (args, rest) = w.split_at(fs.len());
                out.push(concat(&[Pat::Ctor(c, args.to_vec())], rest));
            }
        }
        return out;
    }
    let rest = witnesses(enums, &default_rows(rows), &tys[1..]);
    if n > 0 && (!used.is_empty() || rows.is_empty()) {
        for c in (0..n).filter(|c| !used.contains(c)) {
            let arity = fields(enums, &tys[0], c).len();
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
pub fn render(enums: &Enums, ty: &Type, pat: &Pat) -> String {
    let Pat::Ctor(c, args) = pat else {
        return "_".into();
    };
    let Type::Enum(en, _) = ty else {
        return "_".into();
    };
    let fs = fields(enums, ty, *c);
    let mut args = args.iter().zip(&fs);
    enums[en].variants[*c]
        .parts
        .iter()
        .filter_map(|p| match p {
            PartT::Word(w) => Some(w.clone()),
            PartT::Param(..) => args
                .next()
                .map(|(a, t)| format!("({})", render(enums, t, a))),
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
        Pat::Ctor(c, subs) => {
            let Value::Variant(_, idx, args) = v else {
                return false;
            };
            idx == c && subs.iter().zip(args).all(|(p, a)| bind(p, a, out))
        }
    }
}
