//! Derive macros for `spark-reflect`.
//!
//! `#[derive(Reflect)]` on a struct with named fields and `#[derive(ReflectEnum)]` on an enum whose variants are
//! unit or have named fields. Field attributes (all inside `#[reflect(...)]`):
//!
//! | attribute | meaning |
//! |---|---|
//! | `range = a..=b` | inclusive numeric range (per component for vectors); violations are errors |
//! | `clamp` | clamp into `range` instead of failing (needs `range`) |
//! | `step = x` | inspector drag step |
//! | `unit = "kg"` | one of `m s kg rad deg m/s rad/s Hz %` |
//! | `default = expr` | default value; otherwise `Default::default()` of the struct (of the field type in enum variants) |
//! | `transient` / `readonly` / `hidden` | flags |
//! | `lua = "name"` / `no_lua` | flat Luau name / no Luau accessor |
//! | `color` / `asset = "texture"` / `node_ref` | reinterpret a `[f32; 4]` / `String` field (also inside `Option` / `Vec`) |
//!
//! Variants accept `#[reflect(name = "dynamic")]`. Documentation comes from `///`.
#![forbid(unsafe_code)]

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use syn::parse::ParseStream;
use syn::spanned::Spanned;
use syn::{
    Attribute, Data, DeriveInput, Expr, ExprLit, ExprRange, Fields, GenericArgument, Ident, Lit, LitStr, Meta, PathArguments, RangeLimits,
    Type, parse_macro_input,
};

const FIELD_KEYS: &[&str] =
    &["range", "clamp", "step", "unit", "default", "transient", "readonly", "hidden", "lua", "no_lua", "color", "asset", "node_ref"];

const UNITS: &[(&str, &str)] = &[
    ("m", "Meters"),
    ("s", "Seconds"),
    ("kg", "Kilograms"),
    ("rad", "Radians"),
    ("deg", "Degrees"),
    ("m/s", "MetersPerSecond"),
    ("rad/s", "RadiansPerSecond"),
    ("Hz", "Hertz"),
    ("%", "Percent"),
];

const ASSETS: &[(&str, &str)] =
    &[("mesh", "Mesh"), ("texture", "Texture"), ("sound", "Sound"), ("script", "Script"), ("prefab", "Prefab"), ("model", "Model")];

/// Derives `spark_reflect::Reflect` (and `ReflectField`) for a struct with named fields.
#[proc_macro_derive(Reflect, attributes(reflect))]
pub fn derive_reflect(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand_struct(&input).unwrap_or_else(syn::Error::into_compile_error).into()
}

/// Derives `spark_reflect::ReflectEnum` (and `ReflectField`) for an enum with unit / named-field variants.
#[proc_macro_derive(ReflectEnum, attributes(reflect))]
pub fn derive_reflect_enum(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand_enum(&input).unwrap_or_else(syn::Error::into_compile_error).into()
}

// ---------------------------------------------------------------------------------------------------------
// Attribute parsing

enum Override {
    Color,
    NodeRef,
    Asset(&'static str),
}

#[derive(Default)]
struct FieldAttrs {
    range: Option<(Expr, Expr)>,
    step: Option<Expr>,
    unit: Option<&'static str>,
    default: Option<Expr>,
    transient: bool,
    readonly: bool,
    hidden: bool,
    no_lua: bool,
    clamp: bool,
    lua: Option<LitStr>,
    over: Option<Override>,
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur.push((prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

fn suggest(name: &str, options: &[&str]) -> String {
    let best = options
        .iter()
        .map(|o| (edit_distance(name, o), o))
        .filter(|(d, _)| *d <= (name.chars().count() / 3).clamp(1, 3))
        .min_by_key(|(d, _)| *d);
    let list = options.join(", ");
    match best {
        Some((_, c)) => format!("did you mean `{c}`? valid: {list}"),
        None => format!("valid: {list}"),
    }
}
fn parse_range(input: ParseStream) -> syn::Result<(Expr, Expr)> {
    let e: Expr = input.parse()?;
    let span = e.span();
    match e {
        Expr::Range(ExprRange { start: Some(lo), limits: RangeLimits::Closed(_), end: Some(hi), .. }) => Ok((*lo, *hi)),
        Expr::Range(ExprRange { limits: RangeLimits::HalfOpen(_), .. }) => {
            Err(syn::Error::new(span, "range must be inclusive: write `a..=b`"))
        }
        _ => Err(syn::Error::new(span, "expected an inclusive range with both bounds, like `0.0..=1.0`")),
    }
}

fn parse_lit_str(meta: &syn::meta::ParseNestedMeta, what: &str) -> syn::Result<LitStr> {
    let v: Expr = meta.value()?.parse()?;
    match v {
        Expr::Lit(ExprLit { lit: Lit::Str(s), .. }) => Ok(s),
        other => Err(syn::Error::new(other.span(), format!("`{what}` expects a string literal"))),
    }
}

fn parse_field_attrs(attrs: &[Attribute]) -> syn::Result<FieldAttrs> {
    let mut out = FieldAttrs::default();
    let mut seen: Vec<String> = Vec::new();
    for attr in attrs.iter().filter(|a| a.path().is_ident("reflect")) {
        attr.parse_nested_meta(|meta| {
            let key = meta.path.get_ident().map(ToString::to_string).unwrap_or_default();
            if !FIELD_KEYS.contains(&key.as_str()) {
                return Err(meta.error(format!("unknown reflect attribute `{key}`; {}", suggest(&key, FIELD_KEYS))));
            }
            if seen.contains(&key) {
                return Err(meta.error(format!("duplicate reflect attribute `{key}`")));
            }
            seen.push(key.clone());
            match key.as_str() {
                "range" => out.range = Some(parse_range(meta.value()?)?),
                "step" => out.step = Some(meta.value()?.parse()?),
                "default" => out.default = Some(meta.value()?.parse()?),
                "unit" => {
                    let s = parse_lit_str(&meta, "unit")?;
                    let found = UNITS.iter().find(|(sym, _)| *sym == s.value());
                    match found {
                        Some((_, variant)) => out.unit = Some(variant),
                        None => {
                            let syms: Vec<&str> = UNITS.iter().map(|(s, _)| *s).collect();
                            return Err(syn::Error::new(s.span(), format!("unknown unit `{}`; valid: {}", s.value(), syms.join(", "))));
                        }
                    }
                }
                "lua" => {
                    let s = parse_lit_str(&meta, "lua")?;
                    let v = s.value();
                    let ok = !v.is_empty()
                        && !v.starts_with(|c: char| c.is_ascii_digit())
                        && v.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
                    if !ok {
                        return Err(syn::Error::new(s.span(), "`lua` must be a valid Luau identifier"));
                    }
                    out.lua = Some(s);
                }
                "asset" => {
                    let s = parse_lit_str(&meta, "asset")?;
                    match ASSETS.iter().find(|(n, _)| *n == s.value()) {
                        Some((_, variant)) => out.over = set_override(out.over.take(), Override::Asset(variant), &meta)?,
                        None => {
                            let names: Vec<&str> = ASSETS.iter().map(|(n, _)| *n).collect();
                            return Err(syn::Error::new(
                                s.span(),
                                format!("unknown asset kind `{}`; valid: {}", s.value(), names.join(", ")),
                            ));
                        }
                    }
                }
                "color" => out.over = set_override(out.over.take(), Override::Color, &meta)?,
                "node_ref" => out.over = set_override(out.over.take(), Override::NodeRef, &meta)?,
                "transient" => out.transient = true,
                "readonly" => out.readonly = true,
                "hidden" => out.hidden = true,
                "no_lua" => out.no_lua = true,
                "clamp" => out.clamp = true,
                _ => {}
            }
            Ok(())
        })?;
    }
    if out.clamp && out.range.is_none() {
        return Err(syn::Error::new(
            attrs.iter().find(|a| a.path().is_ident("reflect")).map_or_else(Span::call_site, |a| a.span()),
            "`clamp` needs a `range = a..=b`",
        ));
    }
    if out.lua.is_some() && out.no_lua {
        return Err(syn::Error::new(
            out.lua.as_ref().map_or_else(Span::call_site, |l| l.span()),
            "`lua` and `no_lua` are mutually exclusive",
        ));
    }
    Ok(out)
}

fn set_override(old: Option<Override>, new: Override, meta: &syn::meta::ParseNestedMeta) -> syn::Result<Option<Override>> {
    if old.is_some() {
        return Err(meta.error("only one of `color`, `asset`, `node_ref` is allowed"));
    }
    Ok(Some(new))
}

/// `///` comments joined with newlines.
fn doc_of(attrs: &[Attribute]) -> String {
    let mut lines: Vec<String> = Vec::new();
    for a in attrs {
        if let Meta::NameValue(nv) = &a.meta
            && nv.path.is_ident("doc")
            && let Expr::Lit(ExprLit { lit: Lit::Str(s), .. }) = &nv.value
        {
            let v = s.value();
            lines.push(v.strip_prefix(' ').unwrap_or(&v).trim_end().to_string());
        }
    }
    lines.join("\n").trim().to_string()
}

fn reject_reflect_attrs(attrs: &[Attribute], what: &str) -> syn::Result<()> {
    match attrs.iter().find(|a| a.path().is_ident("reflect")) {
        Some(a) => Err(syn::Error::new(a.span(), format!("`#[reflect(...)]` is not supported on {what}"))),
        None => Ok(()),
    }
}

fn reject_generics(input: &DeriveInput) -> syn::Result<()> {
    if input.generics.params.is_empty() { Ok(()) } else { Err(syn::Error::new(input.generics.span(), "reflected types cannot be generic")) }
}

// ---------------------------------------------------------------------------------------------------------
// FieldInfo generation

/// `Option<T>` / `Vec<T>` layers of a type, outermost first, and the innermost type.
fn peel(ty: &Type) -> Vec<&'static str> {
    let mut layers = Vec::new();
    let mut cur = ty;
    while let Type::Path(p) = cur {
        let Some(last) = p.path.segments.last() else { break };
        let kind = match last.ident.to_string().as_str() {
            "Option" => "Option",
            "Vec" => "List",
            _ => break,
        };
        let PathArguments::AngleBracketed(args) = &last.arguments else { break };
        let Some(GenericArgument::Type(inner)) = args.args.first() else { break };
        layers.push(kind);
        cur = inner;
    }
    layers
}

fn field_type_tokens(ty: &Type, over: Option<&Override>) -> TokenStream2 {
    let Some(over) = over else {
        return quote!(<#ty as ::spark_reflect::ReflectField>::FIELD_TYPE);
    };
    let mut t = match over {
        Override::Color => quote!(::spark_reflect::FieldType::Color),
        Override::NodeRef => quote!(::spark_reflect::FieldType::NodeRef),
        Override::Asset(kind) => {
            let k = Ident::new(kind, Span::call_site());
            quote!(::spark_reflect::FieldType::Asset(::spark_reflect::AssetKind::#k))
        }
    };
    for layer in peel(ty).into_iter().rev() {
        let l = Ident::new(layer, Span::call_site());
        t = quote!(::spark_reflect::FieldType::#l(&#t));
    }
    t
}

/// `default_expr`: the expression (of the field's Rust type) used when the attribute gives no `default`.
fn field_info_tokens(name: &str, ty: &Type, attrs: &[Attribute], default_fallback: TokenStream2) -> syn::Result<TokenStream2> {
    let a = parse_field_attrs(attrs)?;
    let doc = doc_of(attrs);
    let ty_tokens = field_type_tokens(ty, a.over.as_ref());
    let default_expr = match &a.default {
        Some(e) => quote!(#e),
        None => default_fallback,
    };
    let range = match &a.range {
        Some((lo, hi)) => quote!(::core::option::Option::Some((((#lo) as f64), ((#hi) as f64)))),
        None => quote!(::core::option::Option::None),
    };
    let step = match &a.step {
        Some(s) => quote!(::core::option::Option::Some((#s) as f64)),
        None => quote!(::core::option::Option::None),
    };
    let unit = match a.unit {
        Some(u) => {
            let u = Ident::new(u, Span::call_site());
            quote!(::core::option::Option::Some(::spark_reflect::Unit::#u))
        }
        None => quote!(::core::option::Option::None),
    };
    let lua = match &a.lua {
        Some(l) => quote!(::core::option::Option::Some(#l)),
        None => quote!(::core::option::Option::None),
    };
    let mut flags = quote!(::spark_reflect::FieldFlags::NONE);
    for (on, flag) in
        [(a.readonly, "READ_ONLY"), (a.hidden, "HIDDEN"), (a.transient, "TRANSIENT"), (a.no_lua, "NO_LUA"), (a.clamp, "CLAMP")]
    {
        if on {
            let f = Ident::new(flag, Span::call_site());
            flags = quote!(#flags.union(::spark_reflect::FieldFlags::#f));
        }
    }
    Ok(quote! {
        ::spark_reflect::FieldInfo {
            name: #name,
            ty: #ty_tokens,
            doc: #doc,
            default: || {
                let value: #ty = #default_expr;
                ::spark_reflect::tagged(#ty_tokens, &value)
            },
            range: #range,
            step: #step,
            unit: #unit,
            flags: #flags,
            lua: #lua,
        }
    })
}

// ---------------------------------------------------------------------------------------------------------
// Struct

fn expand_struct(input: &DeriveInput) -> syn::Result<TokenStream2> {
    reject_generics(input)?;
    reject_reflect_attrs(&input.attrs, "a struct")?;
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new(
            input.span(),
            "`#[derive(Reflect)]` supports structs with named fields; use `#[derive(ReflectEnum)]` for enums",
        ));
    };
    let Fields::Named(named) = &data.fields else {
        return Err(syn::Error::new(data.fields.span(), "`#[derive(Reflect)]` needs a struct with named fields"));
    };
    let ident = &input.ident;
    let tname = ident.to_string();
    let tdoc = doc_of(&input.attrs);

    let mut infos = Vec::new();
    let mut names = Vec::new();
    let mut idents = Vec::new();
    for f in &named.named {
        let Some(fid) = &f.ident else { continue };
        let name = fid.to_string();
        let fallback = quote!(<#ident as ::core::default::Default>::default().#fid);
        infos.push(field_info_tokens(&name, &f.ty, &f.attrs, fallback)?);
        names.push(name);
        idents.push(fid.clone());
    }
    let idx: Vec<usize> = (0..idents.len()).collect();

    Ok(quote! {
        impl ::spark_reflect::Reflect for #ident {
            fn type_info() -> &'static ::spark_reflect::TypeInfo {
                static INFO: ::spark_reflect::TypeInfo = ::spark_reflect::TypeInfo {
                    name: #tname,
                    doc: #tdoc,
                    fields: &[ #(#infos),* ],
                };
                &INFO
            }

            fn to_value(&self) -> ::spark_reflect::Value {
                let info = <Self as ::spark_reflect::Reflect>::type_info();
                ::spark_reflect::Value::Map(::std::vec![
                    #( (::std::string::String::from(#names), ::spark_reflect::encode_field(&info.fields[#idx], &self.#idents)) ),*
                ])
            }

            #[allow(unused_variables)]
            fn from_value(v: &::spark_reflect::Value) -> ::core::result::Result<Self, ::spark_reflect::ReflectError> {
                let info = <Self as ::spark_reflect::Reflect>::type_info();
                let map = ::spark_reflect::expect_map(info, v)?;
                ::core::result::Result::Ok(Self {
                    #( #idents: ::spark_reflect::decode_field(&info.fields[#idx], map)? ),*
                })
            }

            #[allow(unused_variables)]
            fn get(&self, path: &str) -> ::core::result::Result<::spark_reflect::Value, ::spark_reflect::ReflectError> {
                let info = <Self as ::spark_reflect::Reflect>::type_info();
                if path.is_empty() {
                    return ::core::result::Result::Ok(<Self as ::spark_reflect::Reflect>::to_value(self));
                }
                let (head, rest) = ::spark_reflect::split_path(path);
                match head {
                    #( #names => ::spark_reflect::get_field(&info.fields[#idx], &self.#idents, rest), )*
                    _ => ::core::result::Result::Err(::spark_reflect::unknown_field(info.name, info.fields, head)),
                }
            }

            #[allow(unused_variables)]
            fn set(&mut self, path: &str, v: ::spark_reflect::Value) -> ::core::result::Result<(), ::spark_reflect::ReflectError> {
                let info = <Self as ::spark_reflect::Reflect>::type_info();
                if path.is_empty() {
                    *self = <Self as ::spark_reflect::Reflect>::from_value(&v)?;
                    return ::core::result::Result::Ok(());
                }
                let (head, rest) = ::spark_reflect::split_path(path);
                match head {
                    #( #names => ::spark_reflect::set_field(&info.fields[#idx], &mut self.#idents, rest, v), )*
                    _ => ::core::result::Result::Err(::spark_reflect::unknown_field(info.name, info.fields, head)),
                }
            }
        }

        impl ::spark_reflect::ReflectField for #ident {
            const FIELD_TYPE: ::spark_reflect::FieldType =
                ::spark_reflect::FieldType::Struct(<#ident as ::spark_reflect::Reflect>::type_info);
            fn to_val(&self) -> ::spark_reflect::Value {
                <Self as ::spark_reflect::Reflect>::to_value(self)
            }
            fn from_val(v: &::spark_reflect::Value) -> ::core::result::Result<Self, ::spark_reflect::ReflectError> {
                <Self as ::spark_reflect::Reflect>::from_value(v)
            }
            fn get_path(&self, rest: &str) -> ::core::result::Result<::spark_reflect::Value, ::spark_reflect::ReflectError> {
                <Self as ::spark_reflect::Reflect>::get(self, rest)
            }
            fn set_path(&mut self, rest: &str, v: ::spark_reflect::Value) -> ::core::result::Result<(), ::spark_reflect::ReflectError> {
                <Self as ::spark_reflect::Reflect>::set(self, rest, v)
            }
        }
    })
}

// ---------------------------------------------------------------------------------------------------------
// Enum

fn variant_name(attrs: &[Attribute], default: &str) -> syn::Result<String> {
    let mut name = None;
    for attr in attrs.iter().filter(|a| a.path().is_ident("reflect")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("name") {
                name = Some(parse_lit_str(&meta, "name")?.value());
                Ok(())
            } else {
                Err(meta.error("unknown variant attribute; valid: name"))
            }
        })?;
    }
    Ok(name.unwrap_or_else(|| default.to_string()))
}

fn expand_enum(input: &DeriveInput) -> syn::Result<TokenStream2> {
    reject_generics(input)?;
    reject_reflect_attrs(&input.attrs, "an enum")?;
    let Data::Enum(data) = &input.data else {
        return Err(syn::Error::new(input.span(), "`#[derive(ReflectEnum)]` supports enums; use `#[derive(Reflect)]` for structs"));
    };
    let ident = &input.ident;
    let ename = ident.to_string();
    let edoc = doc_of(&input.attrs);

    let mut variant_infos = Vec::new();
    let mut name_arms = Vec::new();
    let mut to_arms = Vec::new();
    let mut from_arms = Vec::new();
    let mut get_arms = Vec::new();
    let mut set_arms = Vec::new();

    for (vi, var) in data.variants.iter().enumerate() {
        let vident = &var.ident;
        let vname = variant_name(&var.attrs, &vident.to_string())?;
        let vdoc = doc_of(&var.attrs);
        let named: Vec<&syn::Field> = match &var.fields {
            Fields::Unit => Vec::new(),
            Fields::Named(n) => n.named.iter().collect(),
            Fields::Unnamed(u) => {
                return Err(syn::Error::new(
                    u.span(),
                    "tuple variants are not supported; use named fields (`V { x: f32 }`) or a unit variant",
                ));
            }
        };

        let mut finfos = Vec::new();
        let mut fnames = Vec::new();
        let mut fidents = Vec::new();
        let mut binds = Vec::new();
        for (fi, f) in named.iter().enumerate() {
            let Some(fid) = &f.ident else { continue };
            let ty = &f.ty;
            let fallback = quote!(<#ty as ::core::default::Default>::default());
            finfos.push(field_info_tokens(&fid.to_string(), ty, &f.attrs, fallback)?);
            fnames.push(fid.to_string());
            fidents.push(fid.clone());
            binds.push(format_ident!("__f{}", fi));
        }
        let fidx: Vec<usize> = (0..fidents.len()).collect();

        variant_infos.push(quote! {
            ::spark_reflect::VariantInfo { name: #vname, doc: #vdoc, fields: &[ #(#finfos),* ] }
        });

        let is_unit = matches!(var.fields, Fields::Unit);
        let pat_all = if is_unit { quote!(Self::#vident) } else { quote!(Self::#vident { .. }) };
        name_arms.push(quote!(#pat_all => #vname,));

        let pat_bind = if is_unit { quote!(Self::#vident) } else { quote!(Self::#vident { #( #fidents: #binds ),* }) };
        to_arms.push(quote! {
            #pat_bind => ::spark_reflect::Value::Enum {
                variant: ::std::string::String::from(#vname),
                fields: ::std::vec![
                    #( (::std::string::String::from(#fnames),
                        ::spark_reflect::encode_field(&info.variants[#vi].fields[#fidx], #binds)) ),*
                ],
            },
        });

        let construct = if is_unit {
            quote!(Self::#vident)
        } else {
            quote!(Self::#vident {
                #( #fidents: ::spark_reflect::decode_field(&info.variants[#vi].fields[#fidx], fields)? ),*
            })
        };
        from_arms.push(quote!(#vi => ::core::result::Result::Ok(#construct),));

        let pat_rest = if is_unit { quote!(Self::#vident) } else { quote!(Self::#vident { #( #fidents: #binds, )* .. }) };
        get_arms.push(quote! {
            #pat_rest => match head {
                #( #fnames => ::spark_reflect::get_field(&info.variants[#vi].fields[#fidx], #binds, rest), )*
                _ => ::core::result::Result::Err(::spark_reflect::unknown_field(info.name, info.variants[#vi].fields, head)),
            },
        });
        set_arms.push(quote! {
            #pat_rest => match head {
                #( #fnames => ::spark_reflect::set_field(&info.variants[#vi].fields[#fidx], #binds, rest, v), )*
                _ => ::core::result::Result::Err(::spark_reflect::unknown_field(info.name, info.variants[#vi].fields, head)),
            },
        });
    }

    if data.variants.is_empty() {
        return Err(syn::Error::new(input.span(), "an empty enum cannot be reflected"));
    }

    Ok(quote! {
        impl ::spark_reflect::ReflectEnum for #ident {
            fn enum_info() -> &'static ::spark_reflect::EnumInfo {
                static INFO: ::spark_reflect::EnumInfo = ::spark_reflect::EnumInfo {
                    name: #ename,
                    doc: #edoc,
                    variants: &[ #(#variant_infos),* ],
                };
                &INFO
            }

            fn variant_name(&self) -> &'static str {
                match self { #(#name_arms)* }
            }

            #[allow(unused_variables)]
            fn to_value(&self) -> ::spark_reflect::Value {
                let info = <Self as ::spark_reflect::ReflectEnum>::enum_info();
                match self { #(#to_arms)* }
            }

            #[allow(unused_variables)]
            fn from_value(v: &::spark_reflect::Value) -> ::core::result::Result<Self, ::spark_reflect::ReflectError> {
                let info = <Self as ::spark_reflect::ReflectEnum>::enum_info();
                let (idx, fields) = ::spark_reflect::expect_enum(info, v)?;
                match idx {
                    #(#from_arms)*
                    _ => ::core::result::Result::Err(::spark_reflect::ReflectError::UnknownField {
                        ty: info.name,
                        path: ::std::string::ToString::to_string(&idx),
                        suggestions: ::std::vec::Vec::new(),
                    }),
                }
            }

            #[allow(unused_variables)]
            fn get(&self, path: &str) -> ::core::result::Result<::spark_reflect::Value, ::spark_reflect::ReflectError> {
                let info = <Self as ::spark_reflect::ReflectEnum>::enum_info();
                if path.is_empty() {
                    return ::core::result::Result::Ok(<Self as ::spark_reflect::ReflectEnum>::to_value(self));
                }
                let (head, rest) = ::spark_reflect::split_path(path);
                match self { #(#get_arms)* }
            }

            #[allow(unused_variables)]
            fn set(&mut self, path: &str, v: ::spark_reflect::Value) -> ::core::result::Result<(), ::spark_reflect::ReflectError> {
                let info = <Self as ::spark_reflect::ReflectEnum>::enum_info();
                if path.is_empty() {
                    *self = <Self as ::spark_reflect::ReflectEnum>::from_value(&v)?;
                    return ::core::result::Result::Ok(());
                }
                let (head, rest) = ::spark_reflect::split_path(path);
                match self { #(#set_arms)* }
            }
        }

        impl ::spark_reflect::ReflectField for #ident {
            const FIELD_TYPE: ::spark_reflect::FieldType =
                ::spark_reflect::FieldType::Enum(<#ident as ::spark_reflect::ReflectEnum>::enum_info);
            fn to_val(&self) -> ::spark_reflect::Value {
                <Self as ::spark_reflect::ReflectEnum>::to_value(self)
            }
            fn from_val(v: &::spark_reflect::Value) -> ::core::result::Result<Self, ::spark_reflect::ReflectError> {
                <Self as ::spark_reflect::ReflectEnum>::from_value(v)
            }
            fn get_path(&self, rest: &str) -> ::core::result::Result<::spark_reflect::Value, ::spark_reflect::ReflectError> {
                <Self as ::spark_reflect::ReflectEnum>::get(self, rest)
            }
            fn set_path(&mut self, rest: &str, v: ::spark_reflect::Value) -> ::core::result::Result<(), ::spark_reflect::ReflectError> {
                <Self as ::spark_reflect::ReflectEnum>::set(self, rest, v)
            }
        }
    })
}
