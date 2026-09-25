//! Implementation of `#[derive(Reactive)]` proc macro.
//!
//! Generates getters, setters (with change detection), watcher dispatch,
//! and computed field caching for fields annotated with `#[reactive]`,
//! `#[reactive(layout)]`, `#[reactive(watch)]`, `#[reactive(watch_with_app)]`,
//! `#[reactive(init = false)]`, `#[reactive(always_update)]`,
//! `#[reactive(private_watch)]`, `#[reactive(private_validate)]`, `#[var]`,
//! `#[var(watch)]`, `#[var(watch_with_app)]`, `#[var(init = false)]`,
//! `#[var(always_update)]`, `#[var(private_watch)]`, `#[var(private_validate)]`,
//! or `#[computed(depends_on = "field1, field2")]` (plus `watch`,
//! `watch_with_app`, `private_watch` on computed).

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Expr, Fields, Lit, Meta, parse2};

/// Parsed annotation on a single field.
// Each flag is a separate `#[reactive(..)]` / `#[var(..)]` option.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone)]
struct ReactiveField {
    /// The field identifier.
    ident: syn::Ident,
    /// The field type.
    ty: syn::Type,
    /// Whether `layout` was specified.
    layout: bool,
    /// The plain watcher's shape (`watch`, `watch1`, `watch0`, `watch_async`).
    watcher: WatcherShape,
    /// Whether `watch_with_app` was specified (watcher receives `&mut App`).
    watch_with_app: bool,
    /// Whether this is a `#[var]` field (no repaint, no layout).
    is_var: bool,
    /// Whether `init = false` was specified (suppress watcher on mount).
    init_false: bool,
    /// Whether `recompose` was specified (recompose owner subtree on change).
    recompose: bool,
    /// Whether `validate` was specified (call `validate_<field>` before store).
    validate: bool,
    /// Whether `private_watch` was specified (call `_watch_<field>` before
    /// `watch_<field>` on dispatch; Python `_check_watchers` order).
    private_watch: bool,
    /// Whether `private_validate` was specified (call `_validate_<field>`
    /// before `validate_<field>` in the setter; Python `_set` order).
    private_validate: bool,
    /// Whether `bindings` was specified (refresh key bindings on change;
    /// Python `reactive(bindings=True)`).
    bindings: bool,
    /// Class names from `toggle_class = "..."` (space-separated in the
    /// attribute; stored split). The setter applies `set_class(bool(value))`
    /// per class before the equality gate (Python `_set` order). Bool
    /// fields only.
    toggle_classes: Vec<String>,
    /// Whether `always_update` was specified (Python `always_update=True`):
    /// the setter records the change and fires watchers even when the new
    /// value equals the old one.
    always_update: bool,
}

/// Parsed `#[computed(depends_on = "field1, field2")]` annotation.
#[derive(Debug, Clone)]
struct ComputedField {
    /// The field identifier.
    ident: syn::Ident,
    /// The field type.
    ty: syn::Type,
    /// Names of reactive fields this computed field depends on.
    depends_on: Vec<String>,
    /// The plain watcher's shape; see [`WatcherShape`].
    watcher: WatcherShape,
    /// Whether `watch_with_app` was specified — call
    /// `watch_<field>(app, old, new, ctx)` when the recomputed value changes.
    watch_with_app: bool,
    /// Whether `private_watch` was specified — call `_watch_<field>` before
    /// `watch_<field>` (same plain signature; Python `_check_watchers` order).
    private_watch: bool,
}

/// The shape of a field's plain watcher. Python's `invoke_watcher` adapts to
/// the watcher's parameter count; static Rust declares it with one flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WatcherShape {
    /// No plain watcher.
    None,
    /// `watch`: `watch_<field>(old, new, ctx)`.
    Plain,
    /// `watch1`: `watch_<field>(new, ctx)`.
    NewOnly,
    /// `watch0`: `watch_<field>()`.
    NoArgs,
    /// `watch_async` (Python `async def watch_*`, awaited inline there). The
    /// method returns a future driven on the worker pool:
    /// `fn watch_<f>(&self, old: T, new: T) -> impl Future<Output = ()> + Send + 'static`.
    Async,
}

/// Record one watcher-shape flag; repeating the same flag is harmless.
fn add_shape(shapes: &mut Vec<WatcherShape>, shape: WatcherShape) {
    if !shapes.contains(&shape) {
        shapes.push(shape);
    }
}

/// The field's watcher shape, rejecting conflicting flags: at most one of
/// `watch`/`watch1`/`watch0`/`watch_async`, and `watch_with_app` combines
/// only with plain `watch` (the other shapes have no app-passing form).
fn watcher_shape(
    attr: &syn::Attribute,
    shapes: &[WatcherShape],
    watch_with_app: bool,
) -> syn::Result<WatcherShape> {
    let shape = match shapes {
        [] => WatcherShape::None,
        [shape] => *shape,
        _ => {
            return Err(syn::Error::new_spanned(
                attr,
                "conflicting watcher flags; use at most one of `watch`, `watch1`, `watch0`, `watch_async`",
            ));
        }
    };
    if watch_with_app && !matches!(shape, WatcherShape::None | WatcherShape::Plain) {
        return Err(syn::Error::new_spanned(
            attr,
            "`watch_with_app` combines only with plain `watch`, not `watch1`/`watch0`/`watch_async`",
        ));
    }
    Ok(shape)
}

/// Parse reactive/var/computed attributes from a field's attributes.
///
/// Returns `Ok(Some(FieldAnnotation))` for annotated fields, `Ok(None)` for
/// unannotated, or `Err(...)` for malformed attributes.
enum FieldAnnotation {
    Reactive(ReactiveField),
    Computed(ComputedField),
}

fn parse_field_annotation(field: &syn::Field) -> Result<Option<FieldAnnotation>, syn::Error> {
    let ident = match field.ident.as_ref() {
        Some(id) => id.clone(),
        None => return Ok(None),
    };
    let ty = field.ty.clone();

    for attr in &field.attrs {
        // Check for #[var] or #[var(...)]
        if attr.path().is_ident("var") {
            return parse_var_attr(attr, ident, ty).map(Some);
        }

        // Check for #[computed(depends_on = "field1, field2"[, watch | watch_with_app | private_watch])]
        if attr.path().is_ident("computed") {
            return parse_computed_attr(attr, ident, ty).map(Some);
        }

        // Check for #[reactive] or #[reactive(...)]
        if attr.path().is_ident("reactive") {
            return parse_reactive_attr(attr, ident, ty).map(Some);
        }
    }

    Ok(None)
}

/// Parse a `#[var]` / `#[var(...)]` attribute.
fn parse_var_attr(
    attr: &syn::Attribute,
    ident: syn::Ident,
    ty: syn::Type,
) -> Result<FieldAnnotation, syn::Error> {
    let mut shapes: Vec<WatcherShape> = Vec::new();
    let mut watch_with_app = false;
    let mut init_false = false;
    let mut validate = false;
    let mut always_update = false;
    let mut private_watch = false;
    let mut private_validate = false;
    let mut bindings = false;
    let mut toggle_classes: Vec<String> = Vec::new();

    // Parse optional args: watch, watch_with_app, validate, always_update, private_watch, private_validate, bindings, toggle_class = "...", init = false
    if let Meta::List(meta_list) = &attr.meta {
        let nested = meta_list.parse_args_with(
            syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated,
        )?;

        for nested_meta in &nested {
            match nested_meta {
                Meta::Path(path) => {
                    if path.is_ident("watch") {
                        add_shape(&mut shapes, WatcherShape::Plain);
                    } else if path.is_ident("watch1") {
                        add_shape(&mut shapes, WatcherShape::NewOnly);
                    } else if path.is_ident("watch0") {
                        add_shape(&mut shapes, WatcherShape::NoArgs);
                    } else if path.is_ident("watch_async") {
                        add_shape(&mut shapes, WatcherShape::Async);
                    } else if path.is_ident("watch_with_app") {
                        watch_with_app = true;
                    } else if path.is_ident("validate") {
                        validate = true;
                    } else if path.is_ident("always_update") {
                        always_update = true;
                    } else if path.is_ident("private_watch") {
                        private_watch = true;
                    } else if path.is_ident("private_validate") {
                        private_validate = true;
                    } else if path.is_ident("bindings") {
                        bindings = true;
                    } else {
                        return Err(unknown_attr(
                            path,
                            "var",
                            "`watch`, `watch1`, `watch0`, `watch_async`, `watch_with_app`, `validate`, `always_update`, `private_watch`, `private_validate`, `bindings`, `toggle_class = \"...\"`, or `init = false` (note: `recompose` is only valid on `#[reactive]`, not `#[var]`)",
                        ));
                    }
                }
                Meta::NameValue(nv) => {
                    if nv.path.is_ident("toggle_class") {
                        toggle_classes = lit_str_arg(&nv.value, "toggle_class")?
                            .split_whitespace()
                            .map(std::string::ToString::to_string)
                            .collect();
                    } else if nv.path.is_ident("init") {
                        if !lit_bool_arg(&nv.value, "init")? {
                            init_false = true;
                        }
                    } else {
                        return Err(unknown_attr(
                            &nv.path,
                            "var",
                            "`toggle_class = \"...\"` or `init`",
                        ));
                    }
                }
                Meta::List(_) => {
                    return Err(syn::Error::new_spanned(
                        nested_meta,
                        "expected a simple identifier (e.g. `watch`, `watch_with_app`) or `init = false`",
                    ));
                }
            }
        }
    }

    let watcher = watcher_shape(attr, &shapes, watch_with_app)?;
    Ok(FieldAnnotation::Reactive(ReactiveField {
        ident,
        ty,
        layout: false,
        watcher,
        watch_with_app,
        is_var: true,
        init_false,
        recompose: false,
        validate,
        always_update,
        private_watch,
        private_validate,
        bindings,
        toggle_classes,
    }))
}

/// Parse a `#[computed(depends_on = "...", ...)]` attribute.
fn parse_computed_attr(
    attr: &syn::Attribute,
    ident: syn::Ident,
    ty: syn::Type,
) -> Result<FieldAnnotation, syn::Error> {
    let mut depends_on = Vec::new();
    let mut shapes: Vec<WatcherShape> = Vec::new();
    let mut watch_with_app = false;
    let mut private_watch = false;

    if let Meta::List(meta_list) = &attr.meta {
        let nested = meta_list.parse_args_with(
            syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated,
        )?;

        for nested_meta in &nested {
            match nested_meta {
                Meta::Path(path) if path.is_ident("private_watch") => {
                    private_watch = true;
                }
                Meta::NameValue(nv) if nv.path.is_ident("depends_on") => {
                    depends_on = lit_str_arg(&nv.value, "depends_on")?
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
                Meta::Path(path) if path.is_ident("watch") => {
                    add_shape(&mut shapes, WatcherShape::Plain);
                }
                Meta::Path(path) if path.is_ident("watch1") => {
                    add_shape(&mut shapes, WatcherShape::NewOnly);
                }
                Meta::Path(path) if path.is_ident("watch0") => {
                    add_shape(&mut shapes, WatcherShape::NoArgs);
                }
                Meta::Path(path) if path.is_ident("watch_async") => {
                    add_shape(&mut shapes, WatcherShape::Async);
                }
                Meta::Path(path) if path.is_ident("watch_with_app") => {
                    watch_with_app = true;
                }
                _ => {
                    return Err(syn::Error::new_spanned(
                        nested_meta,
                        "expected `depends_on = \"field1, field2\"`, `watch`, `watch1`, `watch0`, `watch_async`, `watch_with_app`, or `private_watch`",
                    ));
                }
            }
        }
    } else {
        return Err(syn::Error::new_spanned(
            attr,
            "computed requires `depends_on` argument: #[computed(depends_on = \"field1, field2\")]",
        ));
    }

    if depends_on.is_empty() {
        return Err(syn::Error::new_spanned(
            attr,
            "computed requires at least one dependency in `depends_on`",
        ));
    }

    let watcher = watcher_shape(attr, &shapes, watch_with_app)?;
    Ok(FieldAnnotation::Computed(ComputedField {
        ident,
        ty,
        depends_on,
        watcher,
        watch_with_app,
        private_watch,
    }))
}

/// Parse a `#[reactive]` / `#[reactive(...)]` attribute.
fn parse_reactive_attr(
    attr: &syn::Attribute,
    ident: syn::Ident,
    ty: syn::Type,
) -> Result<FieldAnnotation, syn::Error> {
    let mut layout = false;
    let mut shapes: Vec<WatcherShape> = Vec::new();
    let mut watch_with_app = false;
    let mut init_false = false;
    let mut recompose = false;
    let mut validate = false;
    let mut always_update = false;
    let mut private_watch = false;
    let mut private_validate = false;
    let mut bindings = false;
    let mut toggle_classes: Vec<String> = Vec::new();

    // Parse arguments if present:
    // #[reactive(layout, watch, watch_with_app, recompose, validate, always_update, private_watch, private_validate, bindings, toggle_class = "...", init = false)]
    if let Meta::List(meta_list) = &attr.meta {
        let nested = meta_list.parse_args_with(
            syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated,
        )?;

        for nested_meta in &nested {
            match nested_meta {
                Meta::Path(path) => {
                    if path.is_ident("layout") {
                        layout = true;
                    } else if path.is_ident("watch") {
                        add_shape(&mut shapes, WatcherShape::Plain);
                    } else if path.is_ident("watch1") {
                        add_shape(&mut shapes, WatcherShape::NewOnly);
                    } else if path.is_ident("watch0") {
                        add_shape(&mut shapes, WatcherShape::NoArgs);
                    } else if path.is_ident("watch_async") {
                        add_shape(&mut shapes, WatcherShape::Async);
                    } else if path.is_ident("watch_with_app") {
                        watch_with_app = true;
                    } else if path.is_ident("recompose") {
                        recompose = true;
                    } else if path.is_ident("validate") {
                        validate = true;
                    } else if path.is_ident("always_update") {
                        always_update = true;
                    } else if path.is_ident("private_watch") {
                        private_watch = true;
                    } else if path.is_ident("private_validate") {
                        private_validate = true;
                    } else if path.is_ident("bindings") {
                        bindings = true;
                    } else {
                        return Err(unknown_attr(
                            path,
                            "reactive",
                            "`layout`, `watch`, `watch1`, `watch0`, `watch_async`, `watch_with_app`, `recompose`, `validate`, `always_update`, `private_watch`, `private_validate`, `bindings`, `toggle_class = \"...\"`, or `init = false`",
                        ));
                    }
                }
                Meta::NameValue(nv) => {
                    if nv.path.is_ident("toggle_class") {
                        // Parse `toggle_class = "active highlighted"` (space-separated).
                        toggle_classes = lit_str_arg(&nv.value, "toggle_class")?
                            .split_whitespace()
                            .map(std::string::ToString::to_string)
                            .collect();
                    } else if nv.path.is_ident("init") {
                        // Parse `init = false`
                        if !lit_bool_arg(&nv.value, "init")? {
                            init_false = true;
                        }
                        // init = true is the default, so just ignore it
                    } else {
                        return Err(unknown_attr(
                            &nv.path,
                            "reactive",
                            "`toggle_class = \"...\"` or `init`",
                        ));
                    }
                }
                Meta::List(_) => {
                    return Err(syn::Error::new_spanned(
                        nested_meta,
                        "expected a simple identifier (e.g. `layout`, `watch`, `watch_with_app`) or `init = false`",
                    ));
                }
            }
        }
    }

    let watcher = watcher_shape(attr, &shapes, watch_with_app)?;
    Ok(FieldAnnotation::Reactive(ReactiveField {
        ident,
        ty,
        layout,
        watcher,
        watch_with_app,
        is_var: false,
        init_false,
        recompose,
        validate,
        always_update,
        private_watch,
        private_validate,
        bindings,
        toggle_classes,
    }))
}

/// The value of a string-literal attribute argument (`name = "..."`).
fn lit_str_arg(value: &Expr, name: &str) -> Result<String, syn::Error> {
    if let Expr::Lit(expr_lit) = value
        && let Lit::Str(lit_str) = &expr_lit.lit
    {
        return Ok(lit_str.value());
    }
    Err(syn::Error::new_spanned(
        value,
        format!("expected string literal for `{name}`"),
    ))
}

/// "unknown {kind} attribute `name`; expected {expected}", on `path`.
fn unknown_attr(path: &syn::Path, kind: &str, expected: &str) -> syn::Error {
    syn::Error::new_spanned(
        path,
        format!(
            "unknown {kind} attribute `{}`; expected {expected}",
            path.get_ident()
                .map(std::string::ToString::to_string)
                .unwrap_or_default()
        ),
    )
}

/// The value of a boolean-literal attribute argument (`name = true|false`).
fn lit_bool_arg(value: &Expr, name: &str) -> Result<bool, syn::Error> {
    if let Expr::Lit(expr_lit) = value
        && let Lit::Bool(lit_bool) = &expr_lit.lit
    {
        return Ok(lit_bool.value);
    }
    Err(syn::Error::new_spanned(
        value,
        format!("expected boolean literal for `{name}`"),
    ))
}

/// Compute the `ReactiveFlags` constructor expression for a field.
///
/// `recompose` takes precedence over `layout` (recompose implies a layout +
/// repaint of the rebuilt subtree, mirroring Python's `refresh(recompose=True)`).
/// `recompose` is rejected on `#[var]` during parsing, so it only applies here
/// to `#[reactive]` fields.
fn flags_expr(field: &ReactiveField) -> TokenStream {
    let base = if field.recompose && field.init_false {
        quote! { rusty_textual::reactive::ReactiveFlags::reactive_recompose_no_init() }
    } else if field.recompose {
        quote! { rusty_textual::reactive::ReactiveFlags::reactive_recompose() }
    } else if field.is_var && field.init_false {
        quote! { rusty_textual::reactive::ReactiveFlags::var_no_init() }
    } else if field.is_var {
        quote! { rusty_textual::reactive::ReactiveFlags::var() }
    } else if field.layout && field.init_false {
        quote! { rusty_textual::reactive::ReactiveFlags::reactive_layout_no_init() }
    } else if field.layout {
        quote! { rusty_textual::reactive::ReactiveFlags::reactive_layout() }
    } else if field.init_false {
        quote! { rusty_textual::reactive::ReactiveFlags::reactive_no_init() }
    } else {
        quote! { rusty_textual::reactive::ReactiveFlags::reactive() }
    };
    let mut expr = base;
    if field.always_update {
        expr = quote! { #expr.with_always_update() };
    }
    if field.bindings {
        expr = quote! { #expr.with_bindings() };
    }
    expr
}

/// One watcher match arm: `_watch_<field>` first when `private_watch` is set,
/// then `watch_<field>` when `watch` is set (Python `_check_watchers` order).
/// Plain (no-app) signature; with-app dispatch reuses these arms for the
/// non-app section, matching how public plain watchers already fire there.
fn watcher_arm(
    ident: &syn::Ident,
    ty: &syn::Type,
    watcher: WatcherShape,
    private_watch: bool,
) -> TokenStream {
    let field_name_str = ident.to_string();
    let mut calls: Vec<TokenStream> = Vec::new();
    if private_watch {
        let watcher_name = format_ident!("_watch_{}", ident);
        calls.push(quote! { self.#watcher_name(old, new, ctx); });
    }
    // Public arity (Python `invoke_watcher` adapts to 0/1/2 parameters;
    // static Rust selects the shape by flag, one per field).
    let watcher_name = format_ident!("watch_{}", ident);
    match watcher {
        WatcherShape::None => {}
        WatcherShape::NoArgs => calls.push(quote! { self.#watcher_name(); }),
        WatcherShape::NewOnly => calls.push(quote! { self.#watcher_name(new, ctx); }),
        WatcherShape::Async => {
            // Python `async def watch_*` is awaited inline; static Rust has no
            // inline executor in dispatch, so the future rides the worker pool
            // (plain threads — never a nesting runtime). Owned clones: the
            // future must be 'static, so it cannot borrow the widget or the
            // context.
            calls.push(quote! {{
                let __watch_future = self.#watcher_name(old.clone(), new.clone());
                ctx.request_async_watcher_task(stringify!(#watcher_name), __watch_future);
            }});
        }
        WatcherShape::Plain => calls.push(quote! { self.#watcher_name(old, new, ctx); }),
    }
    quote! {
        #field_name_str => {
            if let (Some(old), Some(new)) = (
                change.old_value.downcast_ref::<#ty>(),
                change.new_value.downcast_ref::<#ty>(),
            ) {
                #(#calls)*
            }
        }
    }
}

pub fn derive_reactive_impl(input: TokenStream) -> TokenStream {
    let input: DeriveInput = match parse2(input) {
        Ok(input) => input,
        Err(err) => return err.to_compile_error(),
    };

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            _ => {
                return syn::Error::new_spanned(
                    name,
                    "Reactive can only be derived for structs with named fields",
                )
                .to_compile_error();
            }
        },
        _ => {
            return syn::Error::new_spanned(name, "Reactive can only be derived for structs")
                .to_compile_error();
        }
    };

    let mut reactive_fields = Vec::new();
    let mut computed_fields = Vec::new();
    for field in fields {
        match parse_field_annotation(field) {
            Ok(Some(FieldAnnotation::Reactive(rf))) => reactive_fields.push(rf),
            Ok(Some(FieldAnnotation::Computed(cf))) => computed_fields.push(cf),
            Ok(None) => {}
            Err(err) => return err.to_compile_error(),
        }
    }

    if reactive_fields.is_empty() && computed_fields.is_empty() {
        // No reactive fields — still implement the trait with a no-op.
        return quote! {
            impl #impl_generics rusty_textual::reactive::ReactiveWidget for #name #ty_generics #where_clause {}
        };
    }

    // Generate getters and setters.
    let mut accessors: Vec<TokenStream> = reactive_fields.iter().map(reactive_accessors).collect();

    // Generate computed field accessors (getter only — recomputation is in reactive_dispatch).
    for cf in &computed_fields {
        let field_ident = &cf.ident;
        let field_ty = &cf.ty;

        accessors.push(quote! {
            /// Generated getter for computed field. Returns the cached value.
            pub fn #field_ident(&self) -> &#field_ty {
                &self.#field_ident
            }
        });
    }

    // Build computed recomputation arms: for each changed dependency field,
    // recompute dependent computed fields.
    let computed_recompute_stmts: Vec<TokenStream> = computed_fields
        .iter()
        .map(computed_recompute_stmt)
        .collect();

    let sets = WatchSets::new(&reactive_fields, &computed_fields);

    // ── reactive_dispatch body (plain-watch + computed; no watch_with_app) ──
    let dispatch_body = reactive_dispatch_body(&sets, &computed_recompute_stmts);

    // ── reactive_dispatch_with_app body (all watch kinds + computed) ──
    // Only generated when at least one field has watch_with_app.
    let dispatch_with_app_impl = reactive_dispatch_with_app_impl(&sets, &computed_recompute_stmts);

    // ── reactive_record_init ──
    // For each non-computed reactive field whose effective flags have init=true,
    // emit a synthetic change old==new==current value.
    let init_fields: Vec<&ReactiveField> = reactive_fields
        .iter()
        .filter(|f| {
            // `var()` / `reactive()` / `reactive_layout()` all default to
            // init=true; their `*_no_init()` forms set `init_false`.
            !f.init_false
        })
        .collect();

    let record_init_impl = record_init_impl(&init_fields);

    // Generate the list of reactive field descriptors for `reactive_field_descriptors()`.
    let descriptor_entries: Vec<TokenStream> = reactive_fields
        .iter()
        .map(|field| {
            let field_name_str = field.ident.to_string();
            let f_flags_expr = flags_expr(field);
            quote! {
                rusty_textual::reactive::ReactiveFieldDescriptor {
                    name: #field_name_str,
                    flags: #f_flags_expr,
                }
            }
        })
        .collect();

    let expanded = quote! {
        impl #impl_generics #name #ty_generics #where_clause {
            #(#accessors)*
        }

        impl #impl_generics rusty_textual::reactive::ReactiveWidget for #name #ty_generics #where_clause {
            fn reactive_dispatch(
                &mut self,
                changes: &[rusty_textual::reactive::ReactiveChange],
                ctx: &mut rusty_textual::reactive::ReactiveCtx,
            ) {
                #dispatch_body
            }

            #dispatch_with_app_impl

            fn reactive_field_descriptors(&self) -> &'static [rusty_textual::reactive::ReactiveFieldDescriptor] {
                static DESCRIPTORS: &[rusty_textual::reactive::ReactiveFieldDescriptor] = &[
                    #(#descriptor_entries),*
                ];
                DESCRIPTORS
            }

            #record_init_impl
        }
    };

    expanded
}

/// The fields whose watchers the generated dispatch methods call.
struct WatchSets<'a> {
    plain_watch_fields: Vec<&'a ReactiveField>,
    app_watch_fields: Vec<&'a ReactiveField>,
    computed_plain_watch: Vec<&'a ComputedField>,
    computed_app_watch: Vec<&'a ComputedField>,
}

impl<'a> WatchSets<'a> {
    fn new(reactive_fields: &'a [ReactiveField], computed_fields: &'a [ComputedField]) -> Self {
        // Plain-watch fields: watch=true and/or private_watch=true, watch_with_app=false.
        // These go into reactive_dispatch (no app access).
        let plain_watch_fields: Vec<&ReactiveField> = reactive_fields
            .iter()
            .filter(|f| (f.watcher != WatcherShape::None || f.private_watch) && !f.watch_with_app)
            .collect();

        // watch_with_app fields: watch_with_app=true (may or may not also have watch=true).
        let app_watch_fields: Vec<&ReactiveField> = reactive_fields
            .iter()
            .filter(|f| f.watch_with_app)
            .collect();

        // Computed fields whose recomputed value should fire a watcher.
        // The recompute step records a change under the computed field's name, which
        // re-iterates through dispatch; these arms invoke the matching `watch_*`.
        let computed_plain_watch: Vec<&ComputedField> = computed_fields
            .iter()
            .filter(|c| (c.watcher != WatcherShape::None || c.private_watch) && !c.watch_with_app)
            .collect();
        let computed_app_watch: Vec<&ComputedField> = computed_fields
            .iter()
            .filter(|c| c.watch_with_app)
            .collect();
        WatchSets {
            plain_watch_fields,
            app_watch_fields,
            computed_plain_watch,
            computed_app_watch,
        }
    }

    /// Match arms for the plain (no-app) watchers: reactive fields, then
    /// computed fields (which fire when the recomputed value changes).
    fn plain_arms(&self) -> Vec<TokenStream> {
        let mut arms: Vec<TokenStream> = self
            .plain_watch_fields
            .iter()
            .map(|field| watcher_arm(&field.ident, &field.ty, field.watcher, field.private_watch))
            .collect();
        for cf in &self.computed_plain_watch {
            arms.push(watcher_arm(&cf.ident, &cf.ty, cf.watcher, cf.private_watch));
        }
        arms
    }
}

/// The body of `reactive_dispatch`: plain watchers, then computed-field
/// recomputation (no `watch_with_app` watchers).
fn reactive_dispatch_body(
    sets: &WatchSets<'_>,
    computed_recompute_stmts: &[TokenStream],
) -> TokenStream {
    let has_plain_watch =
        !sets.plain_watch_fields.is_empty() || !sets.computed_plain_watch.is_empty();
    let has_computed = !computed_recompute_stmts.is_empty();
    if !has_plain_watch && !has_computed {
        quote! {
            let _ = (changes, ctx);
        }
    } else {
        let watcher_block = if has_plain_watch {
            let match_arms = sets.plain_arms();
            quote! {
                for change in changes {
                    match change.field_name {
                        #(#match_arms)*
                        _ => {}
                    }
                }
            }
        } else {
            quote! {}
        };

        let computed_block = if has_computed {
            quote! {
                #(#computed_recompute_stmts)*
            }
        } else {
            quote! {}
        };

        quote! {
            #watcher_block
            #computed_block
        }
    }
}

/// The `reactive_dispatch_with_app` override, generated only when a field
/// has `watch_with_app`; otherwise the trait default (which delegates to
/// `reactive_dispatch`) is kept.
fn reactive_dispatch_with_app_impl(
    sets: &WatchSets<'_>,
    computed_recompute_stmts: &[TokenStream],
) -> TokenStream {
    let has_app_watch = !sets.app_watch_fields.is_empty() || !sets.computed_app_watch.is_empty();
    if !has_app_watch {
        return quote! {};
    }
    let has_computed = !computed_recompute_stmts.is_empty();
    // All watcher arms for the with-app override: plain-watch first, then
    // app-watch. Computed-field plain watchers also fire here.
    let plain_arms = sets.plain_arms();

    let mut app_arms: Vec<TokenStream> = sets
        .app_watch_fields
        .iter()
        .map(|field| app_watcher_arm(&field.ident, &field.ty))
        .collect();
    // Computed-field with-app watchers.
    for cf in &sets.computed_app_watch {
        app_arms.push(app_watcher_arm(&cf.ident, &cf.ty));
    }

    let computed_block = if has_computed {
        quote! { #(#computed_recompute_stmts)* }
    } else {
        quote! {}
    };

    // If there are no plain arms and no app arms in the match, we need a fallback.
    let match_body = quote! {
        for change in changes {
            match change.field_name {
                #(#plain_arms)*
                #(#app_arms)*
                _ => {}
            }
        }
        #computed_block
    };

    quote! {
        fn reactive_dispatch_with_app(
            &mut self,
            app: &mut rusty_textual::App,
            changes: &[rusty_textual::reactive::ReactiveChange],
            ctx: &mut rusty_textual::reactive::ReactiveCtx,
        ) {
            #match_body
        }
    }
}

/// The getter, setter and mutation notifier generated for one reactive
/// field.
fn reactive_accessors(field: &ReactiveField) -> TokenStream {
    let field_ident = &field.ident;
    let field_ty = &field.ty;
    let setter_name = format_ident!("set_{}", field_ident);
    let mutate_name = format_ident!("mutate_{}", field_ident);
    let field_name_str = field_ident.to_string();
    let f_flags_expr = flags_expr(field);

    // Validation hooks (Python `_set` order): `_validate_<field>` first
    // when `private_validate` is set, then `validate_<field>` when
    // `validate` is set — each runs before the equality check and store.
    let mut validate_stmts: Vec<TokenStream> = Vec::new();
    if field.private_validate {
        let private_validate_fn = format_ident!("_validate_{}", field_ident);
        validate_stmts.push(quote! {
            let value = self.#private_validate_fn(value);
        });
    }
    if field.validate {
        let validate_fn = format_ident!("validate_{}", field_ident);
        validate_stmts.push(quote! {
            let value = self.#validate_fn(value);
        });
    }
    let validate_stmt = quote! { #(#validate_stmts)* };

    // Class toggles (Python `_set`: `obj.set_class(bool(value), *classes)`
    // runs after validation, before the equality gate — so an equal set
    // still re-applies classes). Bool fields only: any other field type
    // fails to compile here, loudly.
    let toggle_stmts: Vec<TokenStream> = field
        .toggle_classes
        .iter()
        .map(|class| {
            quote! { ctx.set_class(value, #class); }
        })
        .collect();
    let toggle_stmt = quote! { #(#toggle_stmts)* };

    let set_body = setter_store_body(field, &f_flags_expr);

    quote! {
        /// Generated getter for reactive field.
        pub fn #field_ident(&self) -> &#field_ty {
            &self.#field_ident
        }

        /// Generated setter for reactive field. Records the change in
        /// the provided [`ReactiveCtx`] if the value actually changed
        /// (or unconditionally for `always_update` fields).
        pub fn #setter_name(&mut self, value: #field_ty, ctx: &mut rusty_textual::reactive::ReactiveCtx)
        where
            #field_ty: PartialEq + Clone + Send + 'static,
        {
            #validate_stmt
            #toggle_stmt
            #set_body
        }

        /// Generated mutation notifier for a reactive field (Python
        /// `mutate_reactive`). Call this AFTER mutating the field in place
        /// (e.g. pushing to a `Vec`), to dispatch watchers / recompose
        /// unconditionally — the value is its own old and new value.
        pub fn #mutate_name(&mut self, ctx: &mut rusty_textual::reactive::ReactiveCtx)
        where
            #field_ty: Clone + Send + 'static,
        {
            let snapshot = self.#field_ident.clone();
            let snapshot_clone = self.#field_ident.clone();
            ctx.record_mutation(
                #field_name_str,
                #f_flags_expr,
                Box::new(snapshot),
                Box::new(snapshot_clone),
            );
        }
    }
}

/// The setter's store step: record the change when the value differs, or
/// always for `always_update` fields.
fn setter_store_body(field: &ReactiveField, f_flags_expr: &TokenStream) -> TokenStream {
    let field_ident = &field.ident;
    let field_name_str = field_ident.to_string();
    // `always_update` (Python `reactive(..., always_update=True)`) bypasses
    // the equality gate: the change is recorded (and watchers fire) even
    // when the new value equals the old one.
    if field.always_update {
        quote! {
            let old = self.#field_ident.clone();
            self.#field_ident = value;
            let new = self.#field_ident.clone();
            ctx.record_change(
                #field_name_str,
                #f_flags_expr,
                Box::new(old),
                Box::new(new),
            );
        }
    } else {
        // Change detection is exact on purpose, floats included; the
        // allow keeps `clippy::float_cmp` quiet in the user's crate.
        quote! {
            #[allow(clippy::float_cmp)]
            let __changed = self.#field_ident != value;
            if __changed {
                let old = self.#field_ident.clone();
                self.#field_ident = value;
                let new = self.#field_ident.clone();
                ctx.record_change(
                    #field_name_str,
                    #f_flags_expr,
                    Box::new(old),
                    Box::new(new),
                );
            }
        }
    }
}

/// The statement that recomputes one computed field when any of its
/// dependencies changed.
fn computed_recompute_stmt(cf: &ComputedField) -> TokenStream {
    let field_ident = &cf.ident;
    let compute_fn = format_ident!("compute_{}", field_ident);
    let dep_strs: Vec<&str> = cf
        .depends_on
        .iter()
        .map(std::string::String::as_str)
        .collect();
    let field_name_str = field_ident.to_string();

    quote! {
        // Check if any dependency of computed field changed.
        {
            let dep_names: &[&str] = &[#(#dep_strs),*];
            let dep_changed = changes.iter().any(|c| dep_names.contains(&c.field_name));
            if dep_changed {
                let new_val = self.#compute_fn();
                #[allow(clippy::float_cmp)] // Exact change detection, as for setters.
                let __changed = self.#field_ident != new_val;
                if __changed {
                    let old_val = self.#field_ident.clone();
                    self.#field_ident = new_val.clone();
                    ctx.record_change(
                        #field_name_str,
                        rusty_textual::reactive::ReactiveFlags::reactive(),
                        Box::new(old_val) as Box<dyn std::any::Any + Send>,
                        Box::new(new_val) as Box<dyn std::any::Any + Send>,
                    );
                }
            }
        }
    }
}

/// One `watch_with_app` match arm: calls `watch_<field>(app, old, new, ctx)`.
fn app_watcher_arm(ident: &syn::Ident, field_ty: &syn::Type) -> TokenStream {
    let field_name_str = ident.to_string();
    let watcher_name = format_ident!("watch_{}", ident);
    quote! {
        #field_name_str => {
            if let (Some(old), Some(new)) = (
                change.old_value.downcast_ref::<#field_ty>(),
                change.new_value.downcast_ref::<#field_ty>(),
            ) {
                self.#watcher_name(app, old, new, ctx);
            }
        }
    }
}

/// `reactive_record_init`: a synthetic change (old == new == current) for
/// each field that initialises (no `init = false`).
fn record_init_impl(init_fields: &[&ReactiveField]) -> TokenStream {
    if init_fields.is_empty() {
        quote! {}
    } else {
        let record_stmts: Vec<TokenStream> = init_fields
            .iter()
            .map(|field| {
                let field_ident = &field.ident;
                let field_name_str = field_ident.to_string();
                let f_flags_expr = flags_expr(field);
                // Init-phase changes must never recompose: Python's
                // `_initialize_reactive` fires watchers via `_check_watchers`,
                // which never refreshes/recomposes (recompose only happens in
                // `Reactive._set` / `mutate_reactive`). Recomposing at mount would
                // rebuild the freshly-composed tree and discard auto-focus.
                quote! {
                    ctx.record_change(
                        #field_name_str,
                        (#f_flags_expr).without_recompose(),
                        Box::new(self.#field_ident.clone()),
                        Box::new(self.#field_ident.clone()),
                    );
                }
            })
            .collect();

        quote! {
            fn reactive_record_init(&self, ctx: &mut rusty_textual::reactive::ReactiveCtx) {
                #(#record_stmts)*
            }
        }
    }
}
