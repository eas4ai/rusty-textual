//! Rename-robust crate paths for macro expansions.
//!
//! Expansions hardcode `rusty_textual::...` paths, which fail to resolve for
//! consumers that rename the dependency (the docs examples use the short
//! `textual` name). Each macro entry point runs its output through
//! [`retarget_crate_path`], which rewrites those idents to the name the
//! consuming crate actually uses (detected via `proc-macro-crate`).
//!
//! The rewrite only fires for renamed consumers: inside `rusty-textual` itself
//! (`FoundCrate::Itself` — a crate may name itself) and under the canonical
//! name the tokens pass through untouched. Only idents followed by `::` are
//! rewritten, so a user binding or field that merely happens to be named
//! `rusty_textual` is never touched.

use proc_macro2::{Ident, Spacing, Span, TokenStream, TokenTree};

/// Rewrite hardcoded `rusty_textual` path heads in `tokens` to the consuming
/// crate's name for `rusty-textual`. No-op unless the consumer renamed the dep.
pub(crate) fn retarget_crate_path(tokens: TokenStream) -> TokenStream {
    let name = match proc_macro_crate::crate_name("rusty-textual") {
        Ok(proc_macro_crate::FoundCrate::Name(name)) if name != "rusty_textual" => name,
        _ => return tokens,
    };
    let replacement = Ident::new(&name, Span::call_site());
    retarget_stream(tokens, &replacement)
}

fn retarget_stream(tokens: TokenStream, replacement: &Ident) -> TokenStream {
    let list: Vec<TokenTree> = tokens.into_iter().collect();
    let mut out = Vec::with_capacity(list.len());
    for (i, tt) in list.iter().enumerate() {
        match tt {
            TokenTree::Group(group) => {
                let mut rebuilt = proc_macro2::Group::new(
                    group.delimiter(),
                    retarget_stream(group.stream(), replacement),
                );
                rebuilt.set_span(group.span());
                out.push(TokenTree::Group(rebuilt));
            }
            TokenTree::Ident(ident)
                if ident == "rusty_textual" && is_path_use(&list, i) =>
            {
                let mut renamed = replacement.clone();
                renamed.set_span(ident.span());
                out.push(TokenTree::Ident(renamed));
            }
            other => out.push(other.clone()),
        }
    }
    out.into_iter().collect()
}

/// `true` when the ident at `i` opens a path (`rusty_textual::...`), as
/// opposed to a bare binding or field name.
fn is_path_use(list: &[TokenTree], i: usize) -> bool {
    matches!(
        (list.get(i + 1), list.get(i + 2)),
        (Some(TokenTree::Punct(a)), Some(TokenTree::Punct(b)))
            if a.as_char() == ':'
                && b.as_char() == ':'
                && a.spacing() == Spacing::Joint
    )
}
