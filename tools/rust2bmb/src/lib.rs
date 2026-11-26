//! Rewrites one Rust source file into the shape of a BMB unit.
//!
//! BMB's surface is a subset of Rust, so porting is mostly rewriting what
//! falls outside the subset. This crate does the rewrites that are mechanical
//! and reports every construct it cannot rewrite, with its position, instead
//! of guessing. It never writes contracts or effects: it leaves
//! `#[effects(TODO)]` where an effect set must be chosen, and the output is
//! the input of hand work, not a finished port.

mod infer;

use std::collections::{BTreeMap, HashMap, HashSet};

use proc_macro2::Span;
use quote::ToTokens;
use syn::spanned::Spanned;
use syn::visit_mut::{self, VisitMut};
use syn::{
    parse_quote, Attribute, Block, Expr, ExprLit, Fields, FnArg, GenericParam, ImplItem, Item, Lit,
    Pat, Stmt, TraitItem, Type, TypeParamBound,
};

/// A construct the tool did not rewrite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub line: usize,
    pub column: usize,
    pub kind: &'static str,
    pub reason: String,
}

/// The result of converting one file.
#[derive(Debug, Default)]
pub struct Output {
    /// The BMB-shaped source.
    pub text: String,
    /// What could not be rewritten, in source order.
    pub findings: Vec<Finding>,
    /// The mechanical rewrites that were applied, counted by kind.
    pub rewrites: BTreeMap<&'static str, usize>,
    /// Top-level items of the input (including those inside an inline module).
    pub items: usize,
    /// Top-level items written to the output.
    pub items_emitted: usize,
    /// Items written to the output with no finding inside them.
    pub items_clean: usize,
}

impl Output {
    /// The findings counted by kind.
    pub fn kinds(&self) -> BTreeMap<&'static str, usize> {
        let mut m = BTreeMap::new();
        for f in &self.findings {
            *m.entry(f.kind).or_insert(0) += 1;
        }
        m
    }
}

/// Lines that are neither blank nor comments (`//`, `///`, `//!`).
pub fn code_lines(src: &str) -> usize {
    src.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("//"))
        .count()
}

/// Standard-library traits: BMB has none of them.
const STD_TRAITS: &[&str] = &[
    "AsRef",
    "Clone",
    "Copy",
    "Debug",
    "Default",
    "Display",
    "Eq",
    "Error",
    "ExactSizeIterator",
    "From",
    "FromIterator",
    "Hash",
    "Into",
    "IntoIterator",
    "Iterator",
    "Ord",
    "PartialEq",
    "PartialOrd",
];

/// Types BMB has built in under the same name.
const BUILT_IN: &[&str] = &["Box", "Option", "Result", "String", "Vec"];

/// Iterator methods: BMB has range `for` loops and no iterators.
const ITERATOR_METHODS: &[&str] = &[
    "chunks",
    "collect",
    "enumerate",
    "filter",
    "fold",
    "into_iter",
    "iter",
    "iter_mut",
    "map",
    "next",
    "rev",
    "size_hint",
    "skip",
    "step_by",
    "take_while",
    "zip",
];

/// Methods of `Option`, `Result` and text that BMB does not have.
const MISSING_METHODS: &[(&str, &str)] = &[
    ("as_ref", "`as_ref` (no `AsRef` trait)"),
    ("expect", "`expect` (E0213: write the cases with `match`)"),
    (
        "into_bytes",
        "`into_bytes` (a `String` is read with `as_bytes`)",
    ),
    ("is_none", "`is_none` (write the cases with `match`)"),
    ("is_some", "`is_some` (write the cases with `match`)"),
    ("take", "`Option::take` (use `std::mem::replace`)"),
    ("to_owned", "`to_owned` (E0404)"),
    ("to_string", "`to_string` (E0404; no `Display` trait)"),
    ("unwrap", "`unwrap` (E0229: write the cases with `match`)"),
    ("unwrap_err", "`unwrap_err` (write the cases with `match`)"),
];

fn pos(span: Span) -> (usize, usize) {
    let s = span.start();
    (s.line, s.column + 1)
}

fn path_text<T: ToTokens>(t: &T) -> String {
    t.to_token_stream()
        .to_string()
        .replace(" :: ", "::")
        .replace(" < ", "<")
        .replace(" > ", ">")
        .replace(" >", ">")
        .replace("& ", "&")
}

fn last_ident(path: &syn::Path) -> String {
    path.segments
        .last()
        .map(|s| s.ident.to_string())
        .unwrap_or_default()
}

fn is_attr(a: &Attribute, name: &str) -> bool {
    a.path().is_ident(name)
}

/// The first paragraph of the doc comments in `attrs`, joined with spaces.
fn doc_summary(attrs: &[Attribute]) -> Option<String> {
    let mut words: Vec<String> = Vec::new();
    for a in attrs {
        if !is_attr(a, "doc") {
            continue;
        }
        if let syn::Meta::NameValue(nv) = &a.meta {
            if let Expr::Lit(ExprLit {
                lit: Lit::Str(s), ..
            }) = &nv.value
            {
                let line = s.value();
                let line = line.trim();
                if line.is_empty() {
                    if words.is_empty() {
                        continue;
                    }
                    break;
                }
                words.push(line.to_string());
            }
        }
    }
    if words.is_empty() {
        None
    } else {
        Some(words.join(" "))
    }
}

fn summary_attr(text: &str) -> Attribute {
    parse_quote!(#[summary(#text)])
}

/// Per item: the enum variants with named fields, by enum and variant name.
type Variants = HashMap<(String, String), Vec<String>>;

struct Converter {
    out: Output,
    variants: Variants,
    /// Names brought in by `use crate::a::Name;`, with the path to write.
    imports: HashMap<String, syn::Path>,
    /// Findings in the item being converted.
    item_findings: usize,
    /// The `AsRef<[u8]>` parameters of the item being converted.
    as_ref_args: HashSet<String>,
    /// What the crate's files declare, for the types of `let`s.
    decls: infer::Decls,
    /// Names brought in by `use` from another crate (the standard library
    /// included), with the path they stand for.
    external: HashMap<String, String>,
}

impl Converter {
    fn report(&mut self, span: Span, kind: &'static str, reason: impl Into<String>) {
        let (line, column) = pos(span);
        self.out.findings.push(Finding {
            line,
            column,
            kind,
            reason: reason.into(),
        });
        self.item_findings += 1;
    }

    fn rewrote(&mut self, kind: &'static str) {
        *self.out.rewrites.entry(kind).or_insert(0) += 1;
    }

    /// Drops the attributes BMB has no use for, turns doc comments into
    /// `#[summary]` and reports `derive`.
    fn attrs(&mut self, attrs: &mut Vec<Attribute>) {
        let summary = doc_summary(attrs);
        let mut kept: Vec<Attribute> = Vec::new();
        for a in attrs.drain(..) {
            if is_attr(&a, "doc") {
                continue;
            }
            if ["inline", "must_use", "cfg", "cfg_attr", "allow"]
                .iter()
                .any(|n| is_attr(&a, n))
            {
                self.rewrote("attribute dropped (inline, must_use, cfg, cfg_attr, allow)");
                continue;
            }
            if is_attr(&a, "derive") {
                let what = path_text(&a.meta);
                self.report(
                    a.span(),
                    "derive",
                    format!("`{what}`: BMB has no derives (values are copied as in Rust's Copy; `==` on data types exists only in specifications, E0425)"),
                );
                continue;
            }
            kept.push(a);
        }
        if let Some(s) = summary {
            self.rewrote("doc comment to #[summary] (first paragraph)");
            kept.insert(0, summary_attr(&s));
        }
        *attrs = kept;
    }

    fn has_cfg_test(attrs: &[Attribute]) -> bool {
        attrs
            .iter()
            .any(|a| is_attr(a, "cfg") && path_text(&a.meta).contains("test"))
    }

    fn collect_variants(items: &[Item], variants: &mut Variants) {
        for item in items {
            match item {
                Item::Enum(e) => {
                    for v in &e.variants {
                        if let Fields::Named(f) = &v.fields {
                            let names = f
                                .named
                                .iter()
                                .filter_map(|f| f.ident.as_ref().map(|i| i.to_string()))
                                .collect();
                            variants.insert((e.ident.to_string(), v.ident.to_string()), names);
                        }
                    }
                }
                Item::Mod(m) => {
                    if let Some((_, inner)) = &m.content {
                        Self::collect_variants(inner, variants);
                    }
                }
                _ => {}
            }
        }
    }

    /// One top-level item: `Some` to write it, `None` when it was dropped
    /// (and reported).
    fn item(&mut self, mut item: Item) -> Option<Item> {
        self.out.items += 1;
        self.item_findings = 0;
        let span = item.span();
        let kept = match &mut item {
            Item::Use(u) => {
                self.use_item(u);
                false
            }
            Item::ExternCrate(e) => {
                self.report(
                    e.span(),
                    "extern crate",
                    format!("`extern crate {}`: BMB has no crates; the standard library is `std::NAME` units", e.ident),
                );
                false
            }
            Item::Mod(m) => {
                if Self::has_cfg_test(&m.attrs) {
                    self.report(
                        span,
                        "test module",
                        format!("`mod {}` under `#[cfg(test)]`: tests are ported as data with a driver program, not as code", m.ident),
                    );
                } else if m.content.is_none() {
                    self.report(
                        span,
                        "module declaration",
                        format!("`mod {};`: a file module becomes its own unit; run the tool on its file", m.ident),
                    );
                } else {
                    self.report(
                        span,
                        "inline module",
                        format!("`mod {} {{ .. }}`: BMB has no nested modules; move its items to a unit of their own", m.ident),
                    );
                }
                false
            }
            Item::Macro(m) => {
                let name = last_ident(&m.mac.path);
                if name == "macro_rules" {
                    let what = m.ident.as_ref().map(|i| i.to_string()).unwrap_or_default();
                    self.report(
                        span,
                        "macro definition",
                        format!("`macro_rules! {what}`: BMB has no macros"),
                    );
                } else {
                    self.report(
                        span,
                        "macro invocation",
                        format!("`{name}!` at item level: BMB has no macros; the items it would expand to are not written"),
                    );
                }
                false
            }
            Item::Impl(i) => {
                if let Some((_, path, _)) = &i.trait_ {
                    let name = last_ident(path);
                    if STD_TRAITS.contains(&name.as_str()) {
                        let ty = path_text(&i.self_ty);
                        self.report(
                            span,
                            "impl of a standard-library trait",
                            format!(
                                "`impl {} for {ty}`: BMB has no standard-library traits",
                                path_text(path)
                            ),
                        );
                        false
                    } else {
                        self.impl_item(i);
                        true
                    }
                } else {
                    self.impl_item(i);
                    true
                }
            }
            Item::Fn(f) => {
                self.attrs(&mut f.attrs);
                self.vis(&mut f.vis);
                self.unsafety(&f.sig);
                self.signature(&mut f.sig, None);
                f.attrs.push(parse_quote!(#[effects(TODO)]));
                self.rewrote("#[effects(TODO)] placed");
                let sig = f.sig.clone();
                self.block(&mut f.block, &sig, None);
                true
            }
            Item::Const(c) => {
                self.attrs(&mut c.attrs);
                self.vis(&mut c.vis);
                self.const_item(c);
                true
            }
            Item::Static(s) => {
                self.attrs(&mut s.attrs);
                self.vis(&mut s.vis);
                self.report(s.span(), "static", "`static`: BMB has only `const` (E0261)");
                true
            }
            Item::Struct(s) => {
                self.attrs(&mut s.attrs);
                self.vis(&mut s.vis);
                self.generics(&s.generics);
                for f in s.fields.iter_mut() {
                    if f.attrs.iter().any(|a| is_attr(a, "doc")) {
                        self.rewrote("doc comment on a field dropped (no #[summary] there)");
                    }
                    f.attrs.retain(|a| !is_attr(a, "doc"));
                    self.vis(&mut f.vis);
                    self.ty(&f.ty, "a field");
                }
                if !matches!(s.fields, Fields::Named(_)) {
                    self.report(s.span(), "tuple struct", "BMB has no tuple structs (E0233)");
                }
                true
            }
            Item::Enum(e) => {
                self.attrs(&mut e.attrs);
                self.vis(&mut e.vis);
                self.generics(&e.generics);
                for v in e.variants.iter_mut() {
                    if let Fields::Named(f) = &v.fields {
                        let types: Vec<Type> = f.named.iter().map(|f| f.ty.clone()).collect();
                        v.fields = Fields::Unnamed(parse_quote!((#(#types),*)));
                        self.rewrote("enum variant with named fields to positional fields");
                    }
                    if v.attrs.iter().any(|a| is_attr(a, "doc")) {
                        self.rewrote("doc comment on a variant dropped (no #[summary] there)");
                    }
                    v.attrs.retain(|a| !is_attr(a, "doc"));
                    self.attrs(&mut v.attrs);
                    for f in v.fields.iter() {
                        self.ty(&f.ty, "a variant payload");
                    }
                }
                true
            }
            Item::Trait(t) => {
                self.attrs(&mut t.attrs);
                self.vis(&mut t.vis);
                self.generics(&t.generics);
                for b in &t.supertraits {
                    self.report(
                        b.span(),
                        "supertrait",
                        format!("`{}`: BMB traits have no supertraits", path_text(b)),
                    );
                }
                for ti in t.items.iter_mut() {
                    match ti {
                        TraitItem::Fn(f) => {
                            self.attrs(&mut f.attrs);
                            self.signature(&mut f.sig, Some("Self"));
                            f.attrs.push(parse_quote!(#[effects(TODO)]));
                            self.rewrote("#[effects(TODO)] placed");
                        }
                        TraitItem::Type(ty) => {
                            self.report(
                                ty.span(),
                                "associated type",
                                format!("`type {}`: BMB traits have no associated types", ty.ident),
                            );
                        }
                        other => {
                            self.report(
                                other.span(),
                                "trait item",
                                "only methods are accepted in a BMB trait",
                            );
                        }
                    }
                }
                true
            }
            Item::Type(t) => {
                self.report(
                    t.span(),
                    "type alias",
                    format!("`type {} = ..`: BMB has no type aliases; write the type where the alias is used", t.ident),
                );
                false
            }
            other => {
                self.report(other.span(), "item", "an item kind BMB does not have");
                false
            }
        };
        if kept {
            self.out.items_emitted += 1;
            Some(item)
        } else {
            None
        }
    }

    fn vis(&mut self, vis: &mut syn::Visibility) {
        if !matches!(vis, syn::Visibility::Inherited) {
            *vis = syn::Visibility::Inherited;
            self.rewrote("visibility dropped");
        }
    }

    fn use_item(&mut self, u: &syn::ItemUse) {
        // `use crate::a::b::Name;` names an item of another unit `b` of this
        // crate: references to `Name` are written `b::Name`.
        fn walk(
            t: &syn::UseTree,
            prefix: &mut Vec<syn::Ident>,
            out: &mut Vec<(Vec<syn::Ident>, Option<syn::Ident>)>,
        ) {
            match t {
                syn::UseTree::Path(p) => {
                    prefix.push(p.ident.clone());
                    walk(&p.tree, prefix, out);
                    prefix.pop();
                }
                syn::UseTree::Name(n) => {
                    // `use a::b::{self, ..}` brings in `b` itself.
                    let mut v = prefix.clone();
                    if n.ident != "self" || v.is_empty() {
                        v.push(n.ident.clone());
                    }
                    out.push((v, None));
                }
                syn::UseTree::Rename(r) => {
                    let mut v = prefix.clone();
                    v.push(r.ident.clone());
                    out.push((v, Some(r.rename.clone())));
                }
                syn::UseTree::Glob(_) => {
                    let mut v = prefix.clone();
                    v.push(syn::Ident::new("_glob", Span::call_site()));
                    out.push((v, None));
                }
                syn::UseTree::Group(g) => {
                    for t in &g.items {
                        walk(t, prefix, out);
                    }
                }
            }
        }
        let mut names = Vec::new();
        walk(&u.tree, &mut Vec::new(), &mut names);
        for (path, rename) in names {
            let text = path
                .iter()
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join("::");
            let first = path[0].to_string();
            let last = path[path.len() - 1].to_string();
            if first == "crate" && path.len() >= 3 && last != "_glob" && rename.is_none() {
                let unit: Vec<&syn::Ident> = path[1..].iter().collect();
                let p: syn::Path = parse_quote!(#(#unit)::*);
                self.imports.insert(last, p);
                self.rewrote("`use crate::..::Name` to full paths `unit::Name`");
            } else if last == "_glob" {
                self.report(
                    u.span(),
                    "use",
                    format!(
                        "`use {}::*`: a glob import has no BMB counterpart (call by full path)",
                        text.trim_end_matches("::_glob")
                    ),
                );
            } else if ["core", "alloc", "std"].contains(&first.as_str())
                && BUILT_IN.contains(&last.as_str())
            {
                self.rewrote(
                    "`use` of a type BMB has built in (String, Vec, Box, Option, Result) dropped",
                );
            } else if ["core", "alloc", "std"].contains(&first.as_str()) {
                self.report(
                    u.span(),
                    "use",
                    format!("`use {text}`: no BMB counterpart of this standard-library path; its uses are reported where they occur"),
                );
                let name = rename
                    .as_ref()
                    .map(|r| r.to_string())
                    .unwrap_or(last.clone());
                self.external.insert(name, text.clone());
            } else if path.len() == 1 {
                self.report(
                    u.span(),
                    "use",
                    format!(
                        "`use {text}`: a re-export of an item of this file (BMB has no re-exports)"
                    ),
                );
            } else {
                self.report(
                    u.span(),
                    "use",
                    format!("`use {text}`: a path outside this crate's units; its uses are reported where they occur"),
                );
                let name = rename
                    .as_ref()
                    .map(|r| r.to_string())
                    .unwrap_or(last.clone());
                if !["self", "super", "crate"].contains(&first.as_str()) {
                    self.external.insert(name, text.clone());
                }
            }
        }
    }

    fn generics(&mut self, g: &syn::Generics) {
        for p in &g.params {
            match p {
                GenericParam::Lifetime(l) => {
                    self.report(
                        l.span(),
                        "lifetime parameter",
                        format!(
                            "`{}`: BMB has no lifetimes (references only as parameters, ADR-0015)",
                            l.lifetime
                        ),
                    );
                }
                GenericParam::Type(t) => {
                    for b in &t.bounds {
                        self.bound(b);
                    }
                }
                GenericParam::Const(c) => {
                    self.report(c.span(), "const generic", "BMB has no const generics");
                }
            }
        }
        if let Some(w) = &g.where_clause {
            self.report(
                w.span(),
                "where clause",
                "bounds are written next to the parameter (E0240)",
            );
        }
    }

    fn bound(&mut self, b: &TypeParamBound) {
        if let TypeParamBound::Trait(t) = b {
            let name = last_ident(&t.path);
            if STD_TRAITS.contains(&name.as_str())
                && !["Copy", "Eq", "Ord"].contains(&name.as_str())
            {
                self.report(
                    t.span(),
                    "bound on a standard-library trait",
                    format!(
                        "`{}`: BMB's built-in bounds are Copy, Eq and Ord",
                        path_text(&t.path)
                    ),
                );
            }
        }
    }

    /// Reports what a type has outside the BMB surface; `place` says where it is.
    fn ty(&mut self, ty: &Type, place: &str) {
        struct Walk<'a> {
            c: &'a mut Converter,
            place: String,
        }
        impl<'v> syn::visit::Visit<'v> for Walk<'_> {
            fn visit_type(&mut self, t: &'v Type) {
                match t {
                    Type::Tuple(tt) if !tt.elems.is_empty() => {
                        self.c.report(
                            t.span(),
                            "tuple type",
                            format!(
                                "`{}` in {}: BMB has no tuples (E0233)",
                                path_text(t),
                                self.place
                            ),
                        );
                    }
                    Type::ImplTrait(_) => {
                        self.c.report(
                            t.span(),
                            "impl Trait type",
                            format!(
                                "`{}` in {}: no `impl Trait` types (E0264, E0265)",
                                path_text(t),
                                self.place
                            ),
                        );
                        return;
                    }
                    Type::Reference(r) => {
                        if let Some(l) = &r.lifetime {
                            self.c.report(
                                l.span(),
                                "lifetime",
                                format!("`{l}` in {}: BMB has no lifetimes", self.place),
                            );
                        }
                        if self.place != "a parameter" {
                            self.c.report(
                                t.span(),
                                "reference outside a parameter",
                                format!(
                                    "`{}` in {}: references come only as parameters (ADR-0015)",
                                    path_text(t),
                                    self.place
                                ),
                            );
                        }
                    }
                    Type::Path(p) => {
                        let text = path_text(&p.path);
                        if text.starts_with("core::")
                            || text.starts_with("::core::")
                            || text.starts_with("std::")
                            || text.starts_with("alloc::")
                        {
                            self.c.report(
                                t.span(),
                                "standard-library type",
                                format!("`{text}` in {}: no BMB counterpart", self.place),
                            );
                            return;
                        }
                    }
                    _ => {}
                }
                syn::visit::visit_type(self, t);
            }
        }
        let mut w = Walk {
            c: self,
            place: place.to_string(),
        };
        syn::visit::Visit::visit_type(&mut w, ty);
    }

    /// The signature: `-> ()` when there is no return type, `&[u8]` for a type
    /// parameter bounded only by `AsRef<[u8]>`, `self: &T` receivers.
    fn signature(&mut self, sig: &mut syn::Signature, self_ty: Option<&str>) {
        // Type parameters bounded by exactly `AsRef<[u8]>` become `&[u8]`.
        let mut as_ref: HashSet<String> = HashSet::new();
        let mut params = syn::punctuated::Punctuated::<GenericParam, syn::Token![,]>::new();
        for p in sig.generics.params.iter() {
            if let GenericParam::Type(t) = p {
                let only_as_ref = t.bounds.len() == 1
                    && matches!(&t.bounds[0], TypeParamBound::Trait(b) if path_text(&b.path).replace(' ', "") == "AsRef<[u8]>");
                if only_as_ref {
                    as_ref.insert(t.ident.to_string());
                    self.rewrote("`T: AsRef<[u8]>` parameter to `&[u8]`");
                    continue;
                }
            }
            params.push(p.clone());
        }
        sig.generics.params = params;
        if sig.generics.params.is_empty() {
            sig.generics.lt_token = None;
            sig.generics.gt_token = None;
        }
        self.generics(&sig.generics.clone());
        for arg in sig.inputs.iter_mut() {
            match arg {
                FnArg::Typed(pt) => {
                    if let Type::Path(p) = &*pt.ty {
                        if p.path.segments.len() == 1
                            && as_ref.contains(&p.path.segments[0].ident.to_string())
                        {
                            *pt.ty = parse_quote!(&[u8]);
                        }
                    }
                    self.ty(&pt.ty.clone(), "a parameter");
                    if !matches!(&*pt.pat, Pat::Ident(_)) {
                        self.report(pt.pat.span(), "parameter pattern", "parameters are names");
                    }
                }
                FnArg::Receiver(r) => {
                    let name = self_ty.unwrap_or("Self");
                    let ident = syn::Ident::new(name, Span::call_site());
                    let ty: Type = if r.reference.is_none() {
                        parse_quote!(#ident)
                    } else if r.mutability.is_some() {
                        parse_quote!(&mut #ident)
                    } else {
                        parse_quote!(&#ident)
                    };
                    if r.reference.as_ref().and_then(|(_, l)| l.as_ref()).is_some() {
                        self.report(r.span(), "lifetime", "BMB has no lifetimes");
                    }
                    *arg = parse_quote!(self: #ty);
                    self.rewrote("receiver to `self: T`, `self: &T` or `self: &mut T`");
                }
            }
        }
        match &sig.output {
            syn::ReturnType::Default => {
                sig.output = parse_quote!(-> ());
                self.rewrote("missing return type to `-> ()`");
            }
            syn::ReturnType::Type(_, t) => {
                let t = (**t).clone();
                self.ty(&t, "a return type");
            }
        }
    }

    fn impl_item(&mut self, i: &mut syn::ItemImpl) {
        self.attrs(&mut i.attrs);
        self.generics(&i.generics);
        let self_ty = match &*i.self_ty {
            Type::Path(p) => last_ident(&p.path),
            other => {
                self.report(
                    other.span(),
                    "impl for a non-path type",
                    format!(
                        "`impl .. for {}`: BMB impls are for named types",
                        path_text(other)
                    ),
                );
                "Self".to_string()
            }
        };
        if let Some((_, path, _)) = &i.trait_ {
            if let Type::Path(p) = &*i.self_ty {
                if p.path.segments.len() == 1
                    && i.generics.params.iter().any(|g| matches!(g, GenericParam::Type(t) if t.ident == p.path.segments[0].ident))
                {
                    self.report(i.span(), "blanket impl", format!("`impl {} for T`: an impl for every type is E0235", path_text(path)));
                }
            }
        }
        for ii in i.items.iter_mut() {
            match ii {
                ImplItem::Fn(f) => {
                    self.attrs(&mut f.attrs);
                    self.vis(&mut f.vis);
                    self.unsafety(&f.sig);
                    self.signature(&mut f.sig, Some(&self_ty));
                    f.attrs.push(parse_quote!(#[effects(TODO)]));
                    self.rewrote("#[effects(TODO)] placed");
                    let sig = f.sig.clone();
                    self.block(&mut f.block, &sig, Some(&self_ty));
                }
                ImplItem::Type(t) => {
                    self.report(
                        t.span(),
                        "associated type",
                        format!("`type {}`: BMB has no associated types", t.ident),
                    );
                }
                ImplItem::Const(c) => {
                    self.report(
                        c.span(),
                        "associated constant",
                        format!("`const {}` in an impl: BMB impls hold only functions; make it a `const` of the unit", c.ident),
                    );
                }
                other => {
                    self.report(
                        other.span(),
                        "impl item",
                        "only functions are accepted in a BMB impl",
                    );
                }
            }
        }
    }

    fn const_item(&mut self, c: &mut syn::ItemConst) {
        // `const X: &[u8; N] = b"..";` becomes `const X: [u8; N] = [..];`.
        if let (
            Type::Reference(r),
            Expr::Lit(ExprLit {
                lit: Lit::ByteStr(s),
                ..
            }),
        ) = (&*c.ty, &*c.expr)
        {
            if let Type::Array(_) = &*r.elem {
                let bytes = s.value();
                let elem = (*r.elem).clone();
                *c.ty = elem;
                let lits = bytes
                    .iter()
                    .map(|b| syn::LitInt::new(&b.to_string(), s.span()));
                *c.expr = parse_quote!([#(#lits),*]);
                self.rewrote("byte-string constant to an array constant");
                return;
            }
        }
        self.ty(&c.ty.clone(), "a constant");
    }

    fn block(&mut self, block: &mut Block, sig: &syn::Signature, self_ty: Option<&str>) {
        let scope = infer::Scope::new(self.decls.clone(), self_ty.map(str::to_string), sig);
        let mut body = Body { c: self, scope };
        body.visit_block_mut(block);
    }

    fn unsafety(&mut self, sig: &syn::Signature) {
        if let Some(u) = &sig.unsafety {
            self.report(
                u.span(),
                "unsafe",
                format!("`unsafe fn {}`: BMB has no unsafe code; what the function relies on becomes its `requires`", sig.ident),
            );
        }
    }
}

/// The rewrites and reports inside function bodies.
struct Body<'a> {
    c: &'a mut Converter,
    scope: infer::Scope,
}

impl Body<'_> {
    /// `match x { lit => a, lo..=hi => b, _ => c }` on integers becomes an
    /// `if` chain; `None` when the arms are not of that form.
    fn match_to_if(&mut self, m: &syn::ExprMatch) -> Option<Expr> {
        if !matches!(&*m.expr, Expr::Path(_) | Expr::Field(_)) {
            return None;
        }
        let scrut = &*m.expr;
        fn cond(p: &Pat, x: &Expr) -> Option<Option<Expr>> {
            match p {
                Pat::Wild(_) => Some(None),
                Pat::Lit(l) => {
                    let l = &l.lit;
                    Some(Some(parse_quote!(#x == #l)))
                }
                Pat::Range(r) => {
                    let lo = r.start.as_ref()?;
                    let hi = r.end.as_ref()?;
                    let e: Expr = match r.limits {
                        syn::RangeLimits::Closed(_) => parse_quote!(#x >= #lo && #x <= #hi),
                        syn::RangeLimits::HalfOpen(_) => parse_quote!(#x >= #lo && #x < #hi),
                    };
                    Some(Some(e))
                }
                Pat::Or(o) => {
                    let mut parts: Vec<Expr> = Vec::new();
                    for c in &o.cases {
                        parts.push(cond(c, x)??);
                    }
                    let e: Expr = parts
                        .into_iter()
                        .reduce(|a, b| parse_quote!((#a) || (#b)))?;
                    Some(Some(e))
                }
                _ => None,
            }
        }
        let mut arms: Vec<(Option<Expr>, Expr)> = Vec::new();
        for arm in &m.arms {
            if arm.guard.is_some() {
                return None;
            }
            arms.push((cond(&arm.pat, scrut)?, (*arm.body).clone()));
        }
        let (last_cond, last_body) = arms.pop()?;
        if last_cond.is_some() || arms.iter().any(|(c, _)| c.is_none()) {
            return None;
        }
        let as_block = |e: Expr| -> Block {
            match e {
                Expr::Block(b) if b.label.is_none() => b.block,
                e => parse_quote!({ #e }),
            }
        };
        let mut chain: Expr = Expr::Block(syn::ExprBlock {
            attrs: vec![],
            label: None,
            block: as_block(last_body),
        });
        for (c, body) in arms.into_iter().rev() {
            let c = c.expect("checked above");
            let then = as_block(body);
            chain = parse_quote!(if #c #then else #chain);
        }
        Some(chain)
    }
}

impl VisitMut for Body<'_> {
    fn visit_expr_mut(&mut self, e: &mut Expr) {
        // Rewrites first, then report on what is left, then the children.
        if let Expr::Match(m) = e {
            if let Some(chain) = self.match_to_if(m) {
                *e = chain;
                self.c
                    .rewrote("integer `match` with literal and range patterns to an `if` chain");
            }
        }
        if let Expr::MethodCall(mc) = e {
            if mc.method == "as_ref" && mc.args.is_empty() {
                if let Expr::Path(p) = &*mc.receiver {
                    if p.path.segments.len() == 1
                        && self
                            .c
                            .is_as_ref_param(&p.path.segments[0].ident.to_string())
                    {
                        *e = (*mc.receiver).clone();
                        self.c
                            .rewrote("`x.as_ref()` on an `AsRef<[u8]>` parameter to `x`");
                    }
                }
            }
        }
        if let Expr::Struct(s) = e {
            let segs: Vec<String> = s
                .path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect();
            if segs.len() >= 2 {
                let key = (segs[segs.len() - 2].clone(), segs[segs.len() - 1].clone());
                if let Some(order) = self.c.variants.get(&key).cloned() {
                    let mut args: Vec<Expr> = Vec::new();
                    for name in &order {
                        match s
                            .fields
                            .iter()
                            .find(|f| matches!(&f.member, syn::Member::Named(i) if i == name))
                        {
                            Some(f) => args.push(f.expr.clone()),
                            None => {
                                self.c.report(
                                    s.span(),
                                    "struct expression",
                                    format!("field `{name}` missing"),
                                );
                                return;
                            }
                        }
                    }
                    let path = s.path.clone();
                    *e = parse_quote!(#path(#(#args),*));
                    self.c.rewrote(
                        "variant with named fields to a positional variant (construction)",
                    );
                }
            }
        }
        if let Expr::Struct(s) = e {
            for f in s.fields.iter_mut() {
                if f.colon_token.is_none() {
                    f.colon_token = Some(Default::default());
                    self.c.rewrote("field init shorthand to `name: name`");
                }
            }
            if s.rest.is_some() {
                self.c.report(
                    s.span(),
                    "struct update",
                    "`..base` in a struct expression: BMB writes every field (E0422)",
                );
            }
        }
        match e {
            Expr::If(f) if matches!(&*f.cond, Expr::Let(_)) => {
                self.c.report(
                    f.if_token.span(),
                    "if let",
                    "BMB has no `if let`: write the whole `match` (`bmb fix` rewrites it, E0225)",
                );
            }
            Expr::While(w) if matches!(&*w.cond, Expr::Let(_)) => {
                self.c.report(
                    w.while_token.span(),
                    "while let",
                    "BMB has no `while let`: write a loop with a `match`",
                );
            }
            Expr::Unsafe(u) => {
                self.c.report(u.unsafe_token.span(), "unsafe", "an `unsafe` block: BMB has no unsafe code; write the safe operation it stands for");
            }
            Expr::Closure(c) => {
                self.c.report(c.span(), "closure", "closures come only as `Fn`-bound arguments with typed parameters (E0263, E0264)");
            }
            Expr::Tuple(t) if !t.elems.is_empty() => {
                self.c.report(
                    t.span(),
                    "tuple expression",
                    format!("`{}`: BMB has no tuples (E0233)", path_text(t)),
                );
            }
            Expr::Macro(m) => {
                let name = last_ident(&m.mac.path);
                self.c.report(
                    m.span(),
                    "macro invocation",
                    format!("`{name}!`: BMB has no macros"),
                );
            }
            Expr::MethodCall(mc) => {
                let name = mc.method.to_string();
                if ITERATOR_METHODS.contains(&name.as_str()) {
                    self.c.report(mc.method.span(), "iterator method", format!("`.{name}()`: BMB has no iterators; write a range `for` or `while` loop"));
                } else if let Some((_, why)) = MISSING_METHODS.iter().find(|(m, _)| *m == name) {
                    self.c
                        .report(mc.method.span(), "missing method", (*why).to_string());
                }
                if mc.turbofish.is_some() {
                    self.c.report(
                        mc.span(),
                        "method turbofish",
                        format!("`.{name}::<..>()`: type arguments on a method call"),
                    );
                }
            }
            Expr::ForLoop(f) => {
                if !matches!(&*f.expr, Expr::Range(_)) {
                    self.c.report(
                        f.expr.span(),
                        "for over an iterator",
                        format!("`for .. in {}`: only range `for` loops", path_text(&f.expr)),
                    );
                }
                if !matches!(&*f.pat, Pat::Ident(_)) {
                    self.c.report(
                        f.pat.span(),
                        "for pattern",
                        "the loop variable of a range `for` is a name",
                    );
                }
            }
            Expr::Cast(c) => match &*c.ty {
                Type::Path(p) if p.path.is_ident("char") => {
                    self.c.report(c.span(), "cast to char", format!("`{}`: `as` converts only between integers (E0205); a char is made with `char::from_u32`", path_text(c)));
                }
                Type::Reference(_) => {
                    self.c.report(
                        c.span(),
                        "cast to a reference",
                        format!("`{}`: `as` converts only between integers", path_text(c)),
                    );
                }
                _ => {}
            },
            Expr::Match(m) => {
                for arm in &m.arms {
                    if arm.guard.is_some() {
                        self.c
                            .report(arm.span(), "match guard", "BMB match arms have no guards");
                    }
                }
                if !matches!(
                    &*m.expr,
                    Expr::Path(_)
                        | Expr::Field(_)
                        | Expr::Call(_)
                        | Expr::MethodCall(_)
                        | Expr::Unary(_)
                ) {
                    self.c.report(
                        m.expr.span(),
                        "match scrutinee",
                        "a match is on an enum value",
                    );
                }
            }
            Expr::Lit(l) => {
                if let Lit::Int(i) = &l.lit {
                    if !i.suffix().is_empty() {
                        let digits = i.base10_digits().to_string();
                        l.lit = Lit::Int(syn::LitInt::new(&digits, i.span()));
                        self.c.rewrote("literal suffix dropped");
                    }
                }
            }
            Expr::Repeat(r) => {
                if let Expr::Lit(ExprLit {
                    lit: Lit::Int(i), ..
                }) = &mut *r.expr
                {
                    if !i.suffix().is_empty() {
                        let digits = i.base10_digits().to_string();
                        *i = syn::LitInt::new(&digits, i.span());
                        self.c.rewrote("literal suffix dropped");
                    }
                }
            }
            _ => {}
        }
        visit_mut::visit_expr_mut(self, e);
    }

    fn visit_pat_mut(&mut self, p: &mut Pat) {
        if let Pat::Struct(s) = p {
            let segs: Vec<String> = s
                .path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect();
            if segs.len() >= 2 {
                let key = (segs[segs.len() - 2].clone(), segs[segs.len() - 1].clone());
                if let Some(order) = self.c.variants.get(&key).cloned() {
                    let mut elems: Vec<Pat> = Vec::new();
                    for name in &order {
                        match s
                            .fields
                            .iter()
                            .find(|f| matches!(&f.member, syn::Member::Named(i) if i == name))
                        {
                            Some(f) => elems.push((*f.pat).clone()),
                            None if s.rest.is_some() => elems.push(parse_quote!(_)),
                            None => {
                                self.c.report(
                                    s.span(),
                                    "struct pattern",
                                    format!("field `{name}` missing"),
                                );
                                return;
                            }
                        }
                    }
                    let path = s.path.clone();
                    *p = parse_quote!(#path(#(#elems),*));
                    self.c
                        .rewrote("variant with named fields to a positional variant (pattern)");
                }
            }
        }
        match p {
            Pat::Tuple(t) if !t.elems.is_empty() => {
                self.c.report(
                    t.span(),
                    "tuple pattern",
                    format!("`{}`: BMB has no tuples (E0233)", path_text(t)),
                );
            }
            Pat::Range(r) => {
                self.c.report(
                    r.span(),
                    "range pattern",
                    "a match is on enum variants; a range is a comparison",
                );
            }
            Pat::Lit(l) => {
                self.c.report(
                    l.span(),
                    "literal pattern",
                    "a match is on enum variants; a literal is a comparison",
                );
            }
            _ => {}
        }
        visit_mut::visit_pat_mut(self, p);
    }

    fn visit_block_mut(&mut self, b: &mut Block) {
        // `let x = x.as_ref();` on an `AsRef<[u8]>` parameter is dropped: the
        // parameter is already the slice.
        let before = b.stmts.len();
        let c = &*self.c;
        b.stmts.retain(|s| {
            if let Stmt::Local(l) = s {
                if let (Pat::Ident(pi), Some(init)) = (&l.pat, &l.init) {
                    if let Expr::MethodCall(mc) = &*init.expr {
                        if mc.method == "as_ref" {
                            if let Expr::Path(p) = &*mc.receiver {
                                return !(p.path.is_ident(&pi.ident)
                                    && c.is_as_ref_param(&pi.ident.to_string()));
                            }
                        }
                    }
                }
            }
            true
        });
        for _ in b.stmts.len()..before {
            self.c
                .rewrote("`let x = x.as_ref();` on an `AsRef<[u8]>` parameter dropped");
        }
        visit_mut::visit_block_mut(self, b);
    }

    fn visit_stmt_mut(&mut self, st: &mut Stmt) {
        if let Stmt::Macro(m) = st {
            let name = last_ident(&m.mac.path);
            self.c.report(
                m.span(),
                "macro invocation",
                format!("`{name}!`: BMB has no macros"),
            );
        }
        visit_mut::visit_stmt_mut(self, st);
    }

    fn visit_local_mut(&mut self, l: &mut syn::Local) {
        match &l.pat {
            Pat::Type(pt) => {
                if let Pat::Ident(pi) = &*pt.pat {
                    self.scope.bind(pi.ident.to_string(), (*pt.ty).clone());
                }
            }
            Pat::Ident(pi) => {
                let inferred = l.init.as_ref().and_then(|i| self.scope.infer(&i.expr));
                match inferred {
                    Some(ty) => {
                        self.scope.bind(pi.ident.to_string(), ty.clone());
                        let pat = l.pat.clone();
                        l.pat = Pat::Type(syn::PatType {
                            attrs: vec![],
                            pat: Box::new(pat),
                            colon_token: Default::default(),
                            ty: Box::new(ty),
                        });
                        self.c
                            .rewrote("type of a `let` written from its initializer");
                    }
                    None => {
                        self.c.report(l.pat.span(), "let without a type", format!("`let {}`: BMB writes the type of every `let`, and the initializer does not name it", path_text(&l.pat)));
                    }
                }
            }
            other => {
                self.c.report(other.span(), "let without a type", format!("`let {}`: BMB writes the type of every `let`, and the tool does not infer types", path_text(other)));
            }
        }
        if let Some(init) = &l.init {
            if init.diverge.is_some() {
                self.c
                    .report(l.span(), "let else", "BMB has no `let .. else`");
            }
        }
        visit_mut::visit_local_mut(self, l);
    }
}

/// Writes the names brought in by `use crate::unit::Name;` as `unit::Name`.
struct Qualify<'a> {
    c: &'a mut Converter,
}

impl VisitMut for Qualify<'_> {
    fn visit_path_mut(&mut self, p: &mut syn::Path) {
        self.c.qualify(p);
        if p.leading_colon.is_none() {
            if let Some(first) = p.segments.first() {
                if let Some(from) = self.c.external.get(&first.ident.to_string()).cloned() {
                    self.c.report(
                        p.span(),
                        "path from another crate",
                        format!(
                            "`{}` (`{from}`): no BMB counterpart is known to the tool",
                            path_text(p)
                        ),
                    );
                }
            }
        }
        visit_mut::visit_path_mut(self, p);
    }
}

impl Converter {
    fn is_as_ref_param(&self, name: &str) -> bool {
        self.as_ref_args.contains(name)
    }

    /// `Name` brought in by `use crate::unit::Name;` becomes `unit::Name`.
    fn qualify(&mut self, path: &mut syn::Path) {
        if path.leading_colon.is_some() {
            return;
        }
        let first = match path.segments.first() {
            Some(s) => s.ident.to_string(),
            None => return,
        };
        if let Some(full) = self.imports.get(&first).cloned() {
            let rest: Vec<syn::PathSegment> = path.segments.iter().skip(1).cloned().collect();
            let mut segs = full.segments.clone();
            if let Some(last) = segs.last_mut() {
                last.arguments = path.segments[0].arguments.clone();
            }
            for r in rest {
                segs.push(r);
            }
            path.segments = segs;
        }
    }
}

/// Converts the Rust source `src` into a BMB unit named `unit`.
pub fn convert(src: &str, unit: &str) -> Result<Output, syn::Error> {
    convert_all(&[(src, unit)])
        .map(|mut v| v.remove(0))
        .map_err(|(_, e)| e)
}

/// Converts several files of one crate, each into its own unit: the enum
/// variants with named fields are collected from all of them, so a variant
/// declared in one file is rewritten where another file uses it. The error
/// carries the index of the file that does not parse.
pub fn convert_all(inputs: &[(&str, &str)]) -> Result<Vec<Output>, (usize, syn::Error)> {
    let mut files = Vec::new();
    for (k, (src, _)) in inputs.iter().enumerate() {
        files.push(syn::parse_file(src).map_err(|e| (k, e))?);
    }
    let mut variants = Variants::new();
    let mut decls = infer::Decls::default();
    for f in &files {
        Converter::collect_variants(&f.items, &mut variants);
        infer::collect(&f.items, &mut decls);
    }
    let mut outs = Vec::new();
    for (file, (_, unit)) in files.into_iter().zip(inputs) {
        outs.push(convert_file(file, unit, variants.clone(), decls.clone()));
    }
    Ok(outs)
}

fn convert_file(file: syn::File, unit: &str, variants: Variants, decls: infer::Decls) -> Output {
    let mut c = Converter {
        out: Output::default(),
        variants,
        imports: HashMap::new(),
        item_findings: 0,
        as_ref_args: HashSet::new(),
        decls,
        external: HashMap::new(),
    };
    let summary = doc_summary(&file.attrs).unwrap_or_else(|| "TODO".to_string());
    if file.attrs.iter().any(|a| !is_attr(a, "doc")) {
        c.rewrote("inner attributes `#![..]` dropped");
    }
    // `use` items first, so that paths in earlier items are qualified too.
    let (uses, rest): (Vec<Item>, Vec<Item>) = file
        .items
        .into_iter()
        .partition(|i| matches!(i, Item::Use(_)));
    let mut items: Vec<Item> = Vec::new();
    for item in uses.into_iter().chain(rest) {
        // Functions see their own `AsRef<[u8]>` parameters while their
        // bodies are rewritten.
        c.as_ref_args = as_ref_params_of(&item);
        if let Some(mut i) = c.item(item) {
            Qualify { c: &mut c }.visit_item_mut(&mut i);
            if c.item_findings == 0 {
                c.out.items_clean += 1;
            }
            items.push(i);
        }
    }
    c.out.findings.sort_by_key(|f| (f.line, f.column));
    let ident = syn::Ident::new(unit, Span::call_site());
    let summary = summary_attr(&summary);
    let module: Item = parse_quote! {
        #summary
        mod #ident {
            #(#items)*
        }
    };
    let out_file = syn::File {
        shebang: None,
        attrs: vec![],
        items: vec![module],
    };
    c.out.text = prettyplease::unparse(&out_file);
    c.out
}

/// The parameters of a function item (or of the functions of an impl) whose
/// type is a type parameter bounded only by `AsRef<[u8]>`.
fn as_ref_params_of(item: &Item) -> HashSet<String> {
    fn of_sig(sig: &syn::Signature, out: &mut HashSet<String>) {
        let mut tys: HashSet<String> = HashSet::new();
        for p in &sig.generics.params {
            if let GenericParam::Type(t) = p {
                if t.bounds.len() == 1 {
                    if let TypeParamBound::Trait(b) = &t.bounds[0] {
                        if path_text(&b.path).replace(' ', "") == "AsRef<[u8]>" {
                            tys.insert(t.ident.to_string());
                        }
                    }
                }
            }
        }
        for a in &sig.inputs {
            if let FnArg::Typed(pt) = a {
                if let (Pat::Ident(pi), Type::Path(tp)) = (&*pt.pat, &*pt.ty) {
                    if tp.path.segments.len() == 1
                        && tys.contains(&tp.path.segments[0].ident.to_string())
                    {
                        out.insert(pi.ident.to_string());
                    }
                }
            }
        }
    }
    let mut out = HashSet::new();
    match item {
        Item::Fn(f) => of_sig(&f.sig, &mut out),
        Item::Impl(i) => {
            for ii in &i.items {
                if let ImplItem::Fn(f) = ii {
                    of_sig(&f.sig, &mut out);
                }
            }
        }
        _ => {}
    }
    out
}
