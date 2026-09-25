use crate::node_id::NodeId;
use crate::style::Style;
use crate::widget_tree::WidgetTree;

use super::common::{
    ChildSpec, apply_wrapper_sizing, extract_child_spec, get_node_style, is_inline_app_screen,
    measure_intrinsic_content_height, measure_intrinsic_content_width,
};
use super::region::Region;
use super::resolve_1d::{Edge, layout_resolve_1d_exact};

pub fn layout_vertical(
    tree: &mut WidgetTree,
    children: &[NodeId],
    available: Region,
    viewport: (u16, u16),
    allow_h_overflow: bool,
) {
    if children.is_empty() {
        return;
    }

    // Phase 1: collect style specs (immutable borrow of tree).
    let specs: Vec<ChildSpec> = children
        .iter()
        .map(|&child| vertical_child_spec(tree, child, available, viewport, allow_h_overflow))
        .collect();

    // Phase 2: build edges for height distribution.
    let heights = resolve_box_heights(&specs, available.height);

    // Phase 3: compute rects and write to tree (mutable borrow).
    // Track previous child's bottom margin for CSS-style margin collapsing:
    // the gap between adjacent siblings is max(prev.bottom, cur.top) not the sum.
    let mut y = available.y;
    let mut prev_margin_bottom: u16 = 0;
    for (i, &child) in children.iter().enumerate() {
        let spec = &specs[i];
        // Resolved heights are already box (margin-excluded), cumulative-floored.
        let layout_h = heights[i];

        // Margins are positioned explicitly below (top margin added to `y`, the
        // gap between siblings collapsed). The first child's top margin advances
        // `y`; collapse the overlap with the previous child's bottom margin.
        let collapse = prev_margin_bottom.min(spec.margin.top);
        y -= i32::from(collapse);

        // Layout rect excludes margin. Positions are signed (flow may originate
        // at a negative coordinate when an ancestor is offset above/left of the
        // viewport); margins/sizes remain unsigned.
        let layout_x = available.x + i32::from(spec.margin.left);
        let layout_y = y + i32::from(spec.margin.top);
        let base_w = available
            .width
            .saturating_sub(spec.margin.left + spec.margin.right);
        let mut layout_w = base_w;

        // Apply explicit width constraint (P2-25: width_edge.size includes chrome).
        if let Some(edge_w) = spec.width_edge.size {
            let explicit_w = edge_w.saturating_sub(spec.margin.left + spec.margin.right);
            if allow_h_overflow {
                // Horizontally-scrollable parent (`overflow-x: auto|scroll`): the
                // child keeps its RESOLVED width even when it exceeds the viewport,
                // so the content overflows and can be scrolled instead of wrapping
                // to the viewport. This covers BOTH `width: auto` (intrinsic
                // width) AND an explicit oversized width like `width: 150%`
                // (Python `_resolve.resolve_box_models` calls `_get_box_model`
                // WITHOUT `constrain_width`, so an explicit percentage width
                // resolves to e.g. 1.5x the container and is NOT clamped — the
                // compositor clips it to the viewport at render time). The grid
                // layout is the only Python layout that passes `constrain_width`.
                layout_w = explicit_w;
            } else {
                layout_w = base_w.min(explicit_w);
            }
        }

        // Apply max-width constraint (border-box: value already includes chrome).
        let layout_w = spec.clamp_to_max_width(layout_w);

        // Apply max-height constraint (border-box: value already includes chrome).
        let layout_h = spec.clamp_to_max_height(layout_h);

        // NOTE: the CSS `offset` displacement is NOT applied here. It is stored
        // per WidgetPlacement in Python and applied AFTER container alignment
        // (`apply_parent_align`), so a `position: relative; offset: x y` child is
        // first centered/aligned by the container and THEN shifted — folding the
        // offset in here would let alignment re-center it and cancel the offset.
        // The post-align offset pass (`apply_flow_offsets`) handles it.
        let (visual_x, visual_y) = (layout_x, layout_y);

        // Layout rect, and the content rect: inner area after border + padding.
        spec.write_rects(tree, child, (visual_x, visual_y), (layout_w, layout_h));

        // Advance past this child's full outer box: top margin + box height +
        // bottom margin. `layout_h` is the resolved (max-clamped) box height.
        // Flow position uses layout_y (not visual_y) — offset is visual-only.
        y = layout_y + i32::from(layout_h) + i32::from(spec.margin.bottom);
        prev_margin_bottom = spec.margin.bottom;
    }
}

/// Phase 1 for one child: its style (adjusted for transparent wrappers), its
/// seeded and measured intrinsic size, and the resulting spec.
fn vertical_child_spec(
    tree: &mut WidgetTree,
    child: NodeId,
    available: Region,
    viewport: (u16, u16),
    allow_h_overflow: bool,
) -> ChildSpec {
    let mut style = get_node_style(tree, child);
    apply_wrapper_sizing(tree, child, &mut style);
    let inline_app_screen = is_inline_app_screen(tree, child);
    if inline_app_screen {
        // Inline, the viewport is the inline height, and the Screen fills it
        // as it fills the terminal in full-screen mode: its height rules
        // (`Screen:inline { height: auto }`) already set the inline height.
        style.height = None;
        style.min_height = None;
        style.max_height = None;
    } else {
        // Not for the inline Screen: it fills the viewport, so no seeded
        // measurement sizes it, and seeding would clamp its scroll offset
        // against the outer height (`AppRoot::on_layout`), in the measuring
        // pass at the terminal size too, before the final layout gives it
        // its real content box.
        seed_measure_width(tree, child, &style, available, viewport, allow_h_overflow);
    }

    let mut intrinsic_height = tree
        .get(child)
        .and_then(|node| node.widget.layout_height())
        .and_then(|h| u16::try_from(h).ok());
    let mut intrinsic_width = tree
        .get(child)
        .and_then(|node| node.widget.content_width())
        .and_then(|w| u16::try_from(w).ok());

    // Bottom-up intrinsic measurement for EXPLICITLY auto-sized containers
    // whose renderable children were drained into the arena tree
    // (`content_width()`/`layout_height()` == None). Only an explicit
    // `width: auto` / `height: auto` opts in — an UNSET dimension (None)
    // keeps the prior flex-fill behaviour so default `1fr` containers and
    // the Screen still fill. This narrows the blast radius to deliberately
    // author-marked `auto` containers.
    //
    // `style.width`/`style.height` were already normalized to `Some(Auto)`
    // above for transparent wrappers whose wrapped child is auto-sized, so a
    // plain `Some(Auto)` check now covers both real auto widgets and those
    // wrappers.
    let width_is_explicit_auto = matches!(style.width.as_ref(), Some(crate::style::Scalar::Auto));
    let height_is_explicit_auto = matches!(style.height.as_ref(), Some(crate::style::Scalar::Auto));
    // The measured value is the children's content extent (the container's
    // OWN border+padding are NOT included). `extract_child_spec` now adds the
    // full vertical chrome (margin+border+padding) on the auto-HEIGHT arm,
    // symmetric with the auto-WIDTH arm — so the measured intrinsic stays
    // PURE content on BOTH axes and the layout side owns all chrome. (The old
    // `+ own_v_chrome` pre-add compensated for the former margin-only height
    // arm and is retired now that the arm is symmetric.)
    let (_own_h_chrome, own_v_chrome) = super::common::own_box_chrome(&style);
    if intrinsic_width.is_none() && width_is_explicit_auto {
        intrinsic_width = measure_intrinsic_content_width(tree, child, viewport);
    }
    if intrinsic_height.is_none() && height_is_explicit_auto {
        // Available CONTENT height this auto container would receive (its
        // outer fill minus own margins + border/padding). Lets Python's
        // all-dynamic-children rule fill an `fr` child (e.g. `Center >
        // Middle(1fr)`).
        let avail_content_h = available
            .height
            .saturating_sub(style.effective_margin().top + style.effective_margin().bottom)
            .saturating_sub(own_v_chrome);
        intrinsic_height = measure_intrinsic_content_height(tree, child, viewport, avail_content_h);
    }
    let mut spec = extract_child_spec(
        &style,
        available.width,
        available.height,
        viewport,
        intrinsic_height,
        intrinsic_width,
    );

    // P2-35: `expand: true` opts this child into flex-grow behavior on
    // the layout axis even when intrinsic auto sizing would otherwise
    // produce a fixed size.
    if style.expand == Some(true) && spec.height_edge.size.is_some() {
        spec.height_edge.size = None;
        spec.height_edge.fraction = spec.height_edge.fraction.max(1);
    }
    spec
}

/// Lay the child (and a wrapped subtree) out at the content width it will
/// get, so its intrinsic height is measured at that width.
fn seed_measure_width(
    tree: &mut WidgetTree,
    child: NodeId,
    style: &Style,
    available: Region,
    viewport: (u16, u16),
    allow_h_overflow: bool,
) {
    let width_is_auto = matches!(
        style.width.as_ref(),
        None | Some(crate::style::Scalar::Auto)
    );
    // Intrinsic content-width hint, used to (a) widen the height-measurement seed and
    // (b) let auto-width children overflow a horizontally-scrollable parent.
    //
    // On a host that lets children overflow horizontally (overflow-x: auto|scroll,
    // OR a scroll host clipping overflow-x: hidden), an `auto`-width child must be
    // measured at its FULL (unwrapped) content width so its auto HEIGHT counts the
    // unwrapped line count — NOT the inflated count produced by wrapping at the
    // narrow viewport. `content_width()` is None for a fill/auto Label (it only
    // reports a hint when `shrink`), so fall back to `auto_content_width()` (the
    // rendered-text cell width used for `width: auto` sizing) in that case. This is
    // Python-faithful: `_resolve.resolve_box_models` measures `get_content_width`
    // unconstrained, and the compositor clips/h-scrolls the overflow — it never
    // re-wraps the child to the container. Mirrors the vertical counterpart of the
    // C13 horizontal-clip fix (root #3: vertical/content-axis virtual-height
    // inflation). (e.g. `scrollbar_corner_color`: a long-line Label kept its full
    // ~430-col width and 71-row height instead of wrapping to 4 extra rows.)
    //
    // Crucially this applies ONLY to EXPLICIT `width: auto` (shrink-to-content,
    // e.g. Label's DEFAULT_CSS), NOT to an UNSET width (Python `1fr` fill, e.g.
    // Static). A fill-width child (Static in `overflow.py`) must keep wrapping to
    // the viewport — Python sizes it to the container width and wraps it. Using
    // `auto_content_width()` on a fill child would wrongly shrink it to its
    // longest line and stop the wrap.
    let width_is_explicit_auto = matches!(style.width.as_ref(), Some(crate::style::Scalar::Auto));
    let pre_intrinsic_w = tree
        .get(child)
        .and_then(|node| {
            node.widget.content_width().or_else(|| {
                if allow_h_overflow && width_is_explicit_auto {
                    node.widget.auto_content_width()
                } else {
                    None
                }
            })
        })
        .and_then(|w| u16::try_from(w).ok());

    // Seed auto-height widgets with a realistic content width before we ask
    // for intrinsic height. Without this, widgets that depend on width
    // (e.g. Markdown) can measure at width=1 and inflate their first-frame
    // height by orders of magnitude.
    let seed_spec = extract_child_spec(
        style,
        available.width,
        available.height,
        viewport,
        None,
        None,
    );
    let mut seed_layout_w = available
        .width
        .saturating_sub(seed_spec.margin.left + seed_spec.margin.right);
    if let Some(edge_w) = seed_spec.width_edge.size {
        let explicit_w = edge_w.saturating_sub(seed_spec.margin.left + seed_spec.margin.right);
        seed_layout_w = seed_layout_w.min(explicit_w);
    }
    // Horizontally-scrollable parent: measure auto-width children at their intrinsic
    // width so wrapping widgets (e.g. Label) report unwrapped height.
    if allow_h_overflow
        && width_is_auto
        && let Some(iw) = pre_intrinsic_w
    {
        let iw_outer = iw.saturating_add(seed_spec.h_box_chrome());
        seed_layout_w = seed_layout_w.max(iw_outer);
    }
    seed_layout_w = seed_spec.clamp_to_max_width(seed_layout_w);
    let seed_content_w = seed_layout_w.saturating_sub(seed_spec.h_box_chrome());
    let seed_content_h = available.height.max(1);
    if let Some(node) = tree.get_mut(child) {
        node.widget.on_layout(seed_content_w.max(1), seed_content_h);
    }
    // Transparent wrappers (`Node`) pass their content box straight through to
    // their single drained child, but `on_layout` on the wrapper is a no-op.
    // Seed the wrapped subtree with the wrapper's content width so width-
    // dependent intrinsic height (e.g. a wrapping Static/Label) measures at
    // the correct width instead of its stale full-viewport width.
    super::common::seed_wrapper_subtree_widths(tree, child, seed_content_w.max(1), seed_content_h);
}

/// Resolve each child's box (margin-excluded) height from its height edge.
fn resolve_box_heights(specs: &[ChildSpec], available_height: u16) -> Vec<u16> {
    // Python parity (`_resolve.resolve_box_models` + `layouts/vertical.py`): the
    // COLLAPSED total vertical margin is reserved from the container height
    // BEFORE the fraction distribution, then the remainder is divided among the
    // children's BOX heights (margin-excluded). `extract_child_spec` folds each
    // child's FULL top+bottom margin into a FIXED edge's `size` (flexible
    // `fr`/`auto` edges carry no margin); strip it here and reserve the single
    // collapsed total from the resolver input so a `fr` child also has its share
    // of the margin reserved (otherwise two `1fr` children split the FULL height
    // and each loses its margin in Phase 3, under-sizing every flexible box).
    let collapsed_margin_total: u16 = {
        let interior: u16 = specs
            .windows(2)
            .map(|pair| pair[0].margin.bottom.max(pair[1].margin.top))
            .sum();
        interior
            .saturating_add(specs[0].margin.top)
            .saturating_add(specs[specs.len() - 1].margin.bottom)
    };
    let edges: Vec<Edge> = specs
        .iter()
        .map(|s| {
            let margin_tb = s.margin.top + s.margin.bottom;
            Edge {
                size: s.height_edge.size.map(|sz| sz.saturating_sub(margin_tb)),
                fraction: s.height_edge.fraction,
                min_size: s.height_edge.min_size.saturating_sub(margin_tb),
            }
        })
        .collect();
    let resolve_total = available_height.saturating_sub(collapsed_margin_total);
    // EXACT cumulative-floor resolution (Python `_resolve.resolve` +
    // `layouts/vertical.py`): fixed and `fr` children alike are sized to exact
    // `f64` cells, then floored on the RUNNING position so a stack of non-integer
    // heights (e.g. 12.5h = 3.75) fence-posts like Python instead of each child
    // truncating independently AND the `fr` children reserving space against the
    // un-carried integer fixed sizes (which overflowed the row by the carry).
    let fixed_exact: Vec<Option<f64>> = specs.iter().map(|s| s.frac_height).collect();
    layout_resolve_1d_exact(resolve_total, &edges, &fixed_exact)
}
