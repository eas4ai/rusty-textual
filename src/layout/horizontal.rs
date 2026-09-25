use crate::node_id::NodeId;
use crate::style::Style;
use crate::widget_tree::WidgetTree;

use super::common::{
    ChildSpec, apply_wrapper_sizing, extract_child_spec, get_node_style,
    measure_intrinsic_content_height, measure_intrinsic_content_width,
};
use super::region::Region;
use super::resolve_1d::{Edge, layout_resolve_1d_exact};

pub fn layout_horizontal(
    tree: &mut WidgetTree,
    children: &[NodeId],
    available: Region,
    viewport: (u16, u16),
    allow_v_overflow: bool,
) {
    if children.is_empty() {
        return;
    }

    // Phase 1: collect style specs.
    let mut specs: Vec<ChildSpec> = Vec::with_capacity(children.len());
    // Retain the (normalized) per-child style + intrinsic-width hint so the
    // width-aware height remeasure (Phase 2.5) can rebuild the height edge for
    // content-sized-height children once the resolved fr/fixed width is known.
    let mut styles: Vec<Style> = Vec::with_capacity(children.len());
    let mut intrinsic_widths: Vec<Option<u16>> = Vec::with_capacity(children.len());
    for &child in children {
        let (spec, style, intrinsic_width) =
            horizontal_child_spec(tree, child, available, viewport);
        specs.push(spec);
        intrinsic_widths.push(intrinsic_width);
        styles.push(style);
    }

    // Phase 2: build edges for width distribution.
    let widths = resolve_box_widths(&specs, available.width);

    // Phase 2.5: width-aware height remeasure for content-sized-height children.
    //
    // Python parity (`_resolve.resolve_box_models`): a child's auto/unset height
    // is measured by `_get_box_model` at the child's RESOLVED width — for an
    // `fr`/fixed-width child that width is only known after the fraction pass.
    // Phase 1 here measured intrinsic height from the widget's STALE
    // `layout_height()` (whatever width it was last laid out at), so a wrapping
    // Label in a `width: 1fr` horizontal row (e.g. `text_style`) reported the
    // wrong wrapped-line count and under/over-sized its box. Re-seed each
    // content-height child's measurement width to its resolved content width and
    // rebuild the height edge. Fires for BOTH `height: auto` and an UNSET height
    // (a content leaf that reports an intrinsic height) — Phase 1's remeasure
    // only covered explicit `auto`, never the unset-height + fr-width case.
    for (i, &child) in children.iter().enumerate() {
        remeasure_content_height(
            tree,
            child,
            &styles[i],
            &mut specs[i],
            (widths[i], intrinsic_widths[i]),
            available,
            viewport,
        );
    }

    // Phase 3: compute rects and write to tree.
    //
    // `layout_left` is the left edge of the current child's LAYOUT (box) region.
    // The resolved `widths[i]` are already margin-excluded box widths. The first
    // child's left edge is `available.x + margin.left`; each subsequent child's
    // left edge is the previous box's right edge plus the COLLAPSED gap
    // (`max(this.right, next.left)`), so adjacent margins overlap instead of
    // summing (Python `layouts/horizontal.py`).
    let mut layout_left = available.x + i32::from(specs[0].margin.left);
    for (i, &child) in children.iter().enumerate() {
        let spec = &specs[i];
        // Resolved widths are already box (margin-excluded), cumulative-floored.
        let layout_w = widths[i];

        // Layout rect excludes margin.
        let layout_x = layout_left;
        let layout_y = available.y + i32::from(spec.margin.top);
        let mut layout_h = available
            .height
            .saturating_sub(spec.margin.top + spec.margin.bottom);

        // Apply explicit height constraint (P2-25: height_edge.size includes chrome).
        if let Some(edge_h) = spec.height_edge.size {
            let explicit_h = edge_h.saturating_sub(spec.margin.top + spec.margin.bottom);
            if allow_v_overflow {
                // Vertically-scrollable parent (`overflow-y: auto|scroll`): let the
                // child keep its resolved height (which may exceed the viewport, e.g.
                // a `min-height` larger than the container) so the content overflows
                // and can be scrolled, rather than clamping it to the viewport height.
                layout_h = explicit_h;
            } else {
                layout_h = layout_h.min(explicit_h);
            }
        }

        // Apply max constraints (border-box: value already includes chrome).
        let layout_w = spec.clamp_to_max_width(layout_w);
        let layout_h = spec.clamp_to_max_height(layout_h);

        // CSS `offset` is applied AFTER container alignment (see
        // vertical.rs / `apply_flow_offsets`), not folded into the flow position
        // here — otherwise alignment would re-center the offset box and cancel it.
        let (visual_x, visual_y) = (layout_x, layout_y);

        // Layout rect and content rect.
        spec.write_rects(tree, child, (visual_x, visual_y), (layout_w, layout_h));

        // Advance to the next child's layout-box left edge: this layout box's
        // right edge plus the COLLAPSED gap between the two boxes
        // (`max(this.margin.right, next.margin.left)`). The last child has no
        // successor, so its trailing margin simply ends the row.
        if let Some(next) = specs.get(i + 1) {
            let gap = spec.margin.right.max(next.margin.left);
            layout_left = layout_x + i32::from(layout_w) + i32::from(gap);
        }
    }
}

/// Phase 1 for one child: its spec, its style (adjusted for transparent
/// wrappers) and its intrinsic width hint.
fn horizontal_child_spec(
    tree: &mut WidgetTree,
    child: NodeId,
    available: Region,
    viewport: (u16, u16),
) -> (ChildSpec, Style, Option<u16>) {
    let mut style = get_node_style(tree, child);
    // Transparent wrappers (`Node`): adopt the wrapped child's sizing.
    apply_wrapper_sizing(tree, child, &mut style);
    // `style.width`/`style.height` were normalized to `Some(Auto)` above
    // for transparent wrappers with auto children, so a plain `Some(Auto)`
    // check covers both real auto widgets and those wrappers.
    let width_is_auto = matches!(style.width.as_ref(), Some(crate::style::Scalar::Auto));
    let height_is_auto = matches!(style.height.as_ref(), Some(crate::style::Scalar::Auto));
    let mut intrinsic_height = tree
        .get(child)
        .and_then(|node| node.widget.layout_height())
        .and_then(|h| u16::try_from(h).ok());
    let mut intrinsic_width = tree
        .get(child)
        .and_then(|node| node.widget.content_width())
        .and_then(|w| u16::try_from(w).ok());
    // `extract_child_spec` now adds the full vertical chrome
    // (margin+border+padding) on the auto-HEIGHT arm, symmetric with the
    // auto-WIDTH arm — so the measured intrinsic stays PURE content on
    // both axes and the layout side owns all chrome (see vertical.rs).
    // The old `+ own_v_chrome` pre-add is retired.
    let (_own_h_chrome, own_v_chrome) = super::common::own_box_chrome(&style);
    if intrinsic_width.is_none() && width_is_auto {
        intrinsic_width = measure_intrinsic_content_width(tree, child, viewport);
    }
    if intrinsic_height.is_none() && height_is_auto {
        // Available CONTENT height this auto child would receive (full
        // container height minus own margins + chrome) so Python's
        // all-dynamic-children rule can fill an `fr`-height child.
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
    if style.expand == Some(true) && spec.width_edge.size.is_some() {
        spec.width_edge.size = None;
        spec.width_edge.fraction = spec.width_edge.fraction.max(1);
    }
    (spec, style, intrinsic_width)
}

/// Resolve each child's box (margin-excluded) width from its width edge.
fn resolve_box_widths(specs: &[ChildSpec], available_width: u16) -> Vec<u16> {
    // Python parity (`_resolve.resolve_box_models` + `layouts/horizontal.py`):
    // the COLLAPSED total margin is reserved from the container width BEFORE the
    // fraction distribution, then the remaining space is divided among the
    // children's BOX widths (margin-excluded). Adjacent horizontal margins
    // COLLAPSE — the interior gap between child `i` and `i+1` is
    // `max(margin_i.right, margin_{i+1}.left)`, not their sum.
    //
    // `extract_child_spec` folds each child's FULL left+right margin into a FIXED
    // edge's `size` (flexible `fr`/`auto` edges carry no margin). To divide on a
    // uniform, margin-excluded basis we strip each fixed edge's own margin here
    // and reserve the single collapsed margin total from the resolver input — so
    // a `fr` child also has its share of the margin reserved (without this, two
    // `1fr` children split the FULL width and then each loses its margin in
    // Phase 3, under-sizing every flexible box by its margin).
    let collapsed_margin_total: u16 = {
        let interior: u16 = specs
            .windows(2)
            .map(|pair| pair[0].margin.right.max(pair[1].margin.left))
            .sum();
        interior
            .saturating_add(specs[0].margin.left)
            .saturating_add(specs[specs.len() - 1].margin.right)
    };
    let edges: Vec<Edge> = specs
        .iter()
        .map(|s| {
            let margin_lr = s.margin.left + s.margin.right;
            Edge {
                // Fixed edges include margin in `size`/`min_size`; strip it so the
                // resolver works on box widths. Flexible edges (`size: None`)
                // carry no margin in `size` already.
                size: s.width_edge.size.map(|sz| sz.saturating_sub(margin_lr)),
                fraction: s.width_edge.fraction,
                min_size: s.width_edge.min_size.saturating_sub(margin_lr),
            }
        })
        .collect();
    let resolve_total = available_width.saturating_sub(collapsed_margin_total);
    // EXACT cumulative-floor resolution (Python `_resolve.resolve` +
    // `layouts/horizontal.py`): fixed and `fr` children alike are sized to exact
    // `f64` cells, then floored on the running position so non-integer widths
    // (e.g. 25vh = 7.5) fence-post like Python and the `fr` children reserve space
    // against the EXACT fixed sizes (not the un-carried integer ones). See
    // `layout_resolve_1d_exact`.
    let fixed_exact: Vec<Option<f64>> = specs.iter().map(|s| s.frac_width).collect();
    layout_resolve_1d_exact(resolve_total, &edges, &fixed_exact)
}

/// Phase 2.5 for one child: rebuild a content-sized height edge at the
/// child's resolved width. `resolved_box_w` is its box width from Phase 2 and
/// `intrinsic_width` its Phase 1 width hint.
fn remeasure_content_height(
    tree: &mut WidgetTree,
    child: NodeId,
    style: &Style,
    spec: &mut ChildSpec,
    (resolved_box_w, intrinsic_width): (u16, Option<u16>),
    available: Region,
    viewport: (u16, u16),
) {
    // Only content-sized-height children depend on the wrap width. Explicit
    // (non-auto) heights and pure fr/flex fills do not.
    let height_is_content = matches!(
        style.height.as_ref(),
        None | Some(crate::style::Scalar::Auto)
    );
    if !height_is_content {
        return;
    }
    // Box (margin-excluded) width resolved for this child, minus its own
    // horizontal chrome → the content width the widget wraps at.
    let resolved_content_w = resolved_box_w.saturating_sub(spec.h_box_chrome()).max(1);
    let avail_content_h = available
        .height
        .saturating_sub(style.effective_margin().top + style.effective_margin().bottom);
    // Re-seed the widget (and any wrapped subtree) at the resolved width so
    // `layout_height()` reflects the final wrap, then re-read it.
    if let Some(node) = tree.get_mut(child) {
        node.widget
            .on_layout(resolved_content_w, avail_content_h.max(1));
    }
    super::common::seed_wrapper_subtree_widths(
        tree,
        child,
        resolved_content_w,
        avail_content_h.max(1),
    );
    let remeasured_height = tree
        .get(child)
        .and_then(|node| node.widget.layout_height())
        .and_then(|h| u16::try_from(h).ok());
    if remeasured_height.is_none() {
        // No intrinsic content height at this width (e.g. a fill leaf or an
        // explicit-auto container drained into the arena): keep Phase 1's
        // spec, which already handled the fallback (full-fill or measured).
        return;
    }
    // Rebuild the height edge at the remeasured intrinsic height, preserving
    // the resolved width edge / max / box-sizing of the Phase 1 spec.
    let rebuilt = extract_child_spec(
        style,
        available.width,
        available.height,
        viewport,
        remeasured_height,
        intrinsic_width,
    );
    spec.height_edge = rebuilt.height_edge;
}
