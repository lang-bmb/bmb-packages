//! The type of a `let` initializer, when the syntax alone settles it.
//!
//! BMB writes the type of every `let`. The tool has no type checker, so it
//! gives a type only where the initializer names it: a suffixed or byte
//! literal, a cast, an array of such, `.len()`, an integer type's
//! `from_le_bytes` or `MAX`, a struct literal, a call of a function of the
//! same file with a written return type, a field of a struct of the file, an
//! element of an array or slice, arithmetic on a typed left operand, a
//! comparison. Anything else stays reported.

use std::collections::HashMap;

use syn::{parse_quote, Expr, FnArg, ImplItem, Item, Lit, Pat, ReturnType, Type};

const INTEGERS: &[&str] = &[
    "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32", "i64", "i128", "isize",
];

/// What the file declares: struct fields and function return types.
#[derive(Clone, Default)]
pub(crate) struct Decls {
    /// struct name -> field name -> type
    structs: HashMap<String, HashMap<String, Type>>,
    /// `f` for a free function, `T::f` for a function of `impl T`
    fns: HashMap<String, Type>,
}

fn simple(t: &Type) -> bool {
    // A type the tool can write as it is: no `Self` and no `impl` (the
    // functions collected have no type parameters of their own).
    let text = quote::ToTokens::to_token_stream(t).to_string();
    !text.contains("Self") && !text.contains("impl")
}

pub(crate) fn collect(items: &[Item], d: &mut Decls) {
    for item in items {
        match item {
            Item::Struct(s) if s.generics.params.is_empty() => {
                let mut fields = HashMap::new();
                for f in &s.fields {
                    if let Some(i) = &f.ident {
                        fields.insert(i.to_string(), f.ty.clone());
                    }
                }
                d.structs.insert(s.ident.to_string(), fields);
            }
            Item::Fn(f) => {
                if let ReturnType::Type(_, t) = &f.sig.output {
                    if f.sig.generics.params.is_empty() && simple(t) {
                        d.fns.insert(f.sig.ident.to_string(), (**t).clone());
                    }
                }
            }
            Item::Impl(i) if i.trait_.is_none() && i.generics.params.is_empty() => {
                if let Type::Path(p) = &*i.self_ty {
                    let ty = p
                        .path
                        .segments
                        .last()
                        .map(|s| s.ident.to_string())
                        .unwrap_or_default();
                    for ii in &i.items {
                        if let ImplItem::Fn(f) = ii {
                            if let ReturnType::Type(_, t) = &f.sig.output {
                                let t: Type = if quote::ToTokens::to_token_stream(&**t).to_string()
                                    == "Self"
                                {
                                    (*i.self_ty).clone()
                                } else {
                                    (**t).clone()
                                };
                                if f.sig.generics.params.is_empty() && simple(&t) {
                                    d.fns.insert(format!("{ty}::{}", f.sig.ident), t);
                                }
                            }
                        }
                    }
                }
            }
            Item::Mod(m) => {
                if let Some((_, inner)) = &m.content {
                    collect(inner, d);
                }
            }
            _ => {}
        }
    }
}

/// The types known inside one function body.
pub(crate) struct Scope {
    pub(crate) decls: Decls,
    pub(crate) self_ty: Option<String>,
    pub(crate) locals: HashMap<String, Type>,
}

fn strip_refs(t: &Type) -> &Type {
    match t {
        Type::Reference(r) => strip_refs(&r.elem),
        Type::Paren(p) => strip_refs(&p.elem),
        other => other,
    }
}

fn path_name(t: &Type) -> Option<String> {
    match strip_refs(t) {
        Type::Path(p) if p.qself.is_none() => Some(
            p.path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect::<Vec<_>>()
                .join("::"),
        ),
        _ => None,
    }
}

impl Scope {
    pub(crate) fn new(decls: Decls, self_ty: Option<String>, sig: &syn::Signature) -> Scope {
        let mut locals = HashMap::new();
        for a in &sig.inputs {
            match a {
                FnArg::Typed(pt) => {
                    if let Pat::Ident(pi) = &*pt.pat {
                        let t: Type = if pi.ident == "self" {
                            match &self_ty {
                                Some(s) => syn::parse_str(s).unwrap_or_else(|_| (*pt.ty).clone()),
                                None => (*pt.ty).clone(),
                            }
                        } else {
                            (*pt.ty).clone()
                        };
                        locals.insert(pi.ident.to_string(), t);
                    }
                }
                FnArg::Receiver(_) => {
                    if let Some(s) = &self_ty {
                        if let Ok(t) = syn::parse_str::<Type>(s) {
                            locals.insert("self".to_string(), t);
                        }
                    }
                }
            }
        }
        Scope {
            decls,
            self_ty,
            locals,
        }
    }

    fn integer(name: &str) -> Option<Type> {
        if INTEGERS.contains(&name) {
            syn::parse_str(name).ok()
        } else {
            None
        }
    }

    /// The type of `e`, when the syntax settles it.
    pub(crate) fn infer(&self, e: &Expr) -> Option<Type> {
        match e {
            Expr::Lit(l) => match &l.lit {
                Lit::Int(i) if !i.suffix().is_empty() => Self::integer(i.suffix()),
                Lit::Byte(_) => Some(parse_quote!(u8)),
                Lit::Bool(_) => Some(parse_quote!(bool)),
                Lit::Char(_) => Some(parse_quote!(char)),
                Lit::Float(f) if f.suffix() == "f64" => Some(parse_quote!(f64)),
                _ => None,
            },
            Expr::Cast(c) => {
                let t = (*c.ty).clone();
                path_name(&t).and_then(|n| Self::integer(&n)).or(
                    if matches!(&t, Type::Path(p) if p.path.is_ident("f64")) {
                        Some(t)
                    } else {
                        None
                    },
                )
            }
            Expr::Paren(p) => self.infer(&p.expr),
            Expr::Group(g) => self.infer(&g.expr),
            Expr::Repeat(r) => {
                let t = self.infer(&r.expr)?;
                let n = &r.len;
                Some(parse_quote!([#t; #n]))
            }
            Expr::Array(a) => {
                let t = self.infer(a.elems.first()?)?;
                let n =
                    syn::LitInt::new(&a.elems.len().to_string(), proc_macro2::Span::call_site());
                Some(parse_quote!([#t; #n]))
            }
            Expr::MethodCall(m) => {
                let name = m.method.to_string();
                if name == "len" && m.args.is_empty() {
                    return Some(parse_quote!(usize));
                }
                if name.starts_with("wrapping_")
                    || name.starts_with("rotate_")
                    || name == "swap_bytes"
                    || name == "to_le"
                    || name == "to_be"
                {
                    return self
                        .infer(&m.receiver)
                        .filter(|t| path_name(t).is_some_and(|n| INTEGERS.contains(&n.as_str())));
                }
                if name == "to_le_bytes" || name == "to_be_bytes" {
                    let t = self.infer(&m.receiver)?;
                    let bits: usize = match path_name(&t)?.as_str() {
                        "u8" | "i8" => 1,
                        "u16" | "i16" => 2,
                        "u32" | "i32" => 4,
                        "u64" | "i64" | "usize" | "isize" => 8,
                        "u128" | "i128" => 16,
                        _ => return None,
                    };
                    let n = syn::LitInt::new(&bits.to_string(), proc_macro2::Span::call_site());
                    return Some(parse_quote!([u8; #n]));
                }
                None
            }
            Expr::Path(p) => {
                let segs: Vec<String> = p
                    .path
                    .segments
                    .iter()
                    .map(|s| s.ident.to_string())
                    .collect();
                if segs.len() == 1 {
                    return self
                        .locals
                        .get(&segs[0])
                        .map(|t| strip_refs(t).clone())
                        .filter(|t| !matches!(t, Type::Slice(_)));
                }
                if segs.len() == 2 && (segs[1] == "MAX" || segs[1] == "MIN") {
                    return Self::integer(&segs[0]);
                }
                None
            }
            Expr::Call(c) => {
                let Expr::Path(p) = &*c.func else { return None };
                let segs: Vec<String> = p
                    .path
                    .segments
                    .iter()
                    .map(|s| s.ident.to_string())
                    .collect();
                match segs.len() {
                    1 => self.decls.fns.get(&segs[0]).cloned(),
                    2 => {
                        if ["from_le_bytes", "from_be_bytes", "from_ne_bytes"]
                            .contains(&segs[1].as_str())
                        {
                            if let Some(t) = Self::integer(&segs[0]) {
                                return Some(t);
                            }
                        }
                        let ty = if segs[0] == "Self" {
                            self.self_ty.clone()?
                        } else {
                            segs[0].clone()
                        };
                        self.decls.fns.get(&format!("{ty}::{}", segs[1])).cloned()
                    }
                    _ => None,
                }
            }
            Expr::Struct(s) => {
                let name = s.path.segments.last()?.ident.to_string();
                if name == "Self" {
                    syn::parse_str(self.self_ty.as_ref()?).ok()
                } else if self.decls.structs.contains_key(&name) {
                    Some(Type::Path(syn::TypePath {
                        qself: None,
                        path: s.path.clone(),
                    }))
                } else {
                    None
                }
            }
            Expr::Field(f) => {
                let base = self.infer(&f.base)?;
                let name = path_name(&base)?;
                let name = name.rsplit("::").next()?.to_string();
                let syn::Member::Named(field) = &f.member else {
                    return None;
                };
                let t = self.decls.structs.get(&name)?.get(&field.to_string())?;
                if matches!(t, Type::Reference(_)) {
                    None
                } else {
                    Some(t.clone())
                }
            }
            Expr::Index(i) => match strip_refs(&self.infer_place(&i.expr)?) {
                Type::Array(a) => Some((*a.elem).clone()),
                Type::Slice(s) => Some((*s.elem).clone()),
                _ => None,
            },
            Expr::Binary(b) => {
                use syn::BinOp::*;
                match b.op {
                    Eq(_) | Ne(_) | Lt(_) | Le(_) | Gt(_) | Ge(_) | And(_) | Or(_) => {
                        Some(parse_quote!(bool))
                    }
                    Add(_) | Sub(_) | Mul(_) | Div(_) | Rem(_) | BitAnd(_) | BitOr(_)
                    | BitXor(_) | Shl(_) | Shr(_) => self.infer(&b.left).or_else(|| match b.op {
                        Shl(_) | Shr(_) => None,
                        _ => self.infer(&b.right),
                    }),
                    _ => None,
                }
            }
            Expr::Unary(u) => match u.op {
                syn::UnOp::Not(_) | syn::UnOp::Neg(_) => self.infer(&u.expr),
                _ => None,
            },
            _ => None,
        }
    }

    /// Like `infer`, but also the type of a slice parameter (for indexing).
    fn infer_place(&self, e: &Expr) -> Option<Type> {
        if let Expr::Path(p) = e {
            if let Some(id) = p.path.get_ident() {
                return self.locals.get(&id.to_string()).cloned();
            }
        }
        self.infer(e)
    }

    pub(crate) fn bind(&mut self, name: String, t: Type) {
        self.locals.insert(name, t);
    }
}
