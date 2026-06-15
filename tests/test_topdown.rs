use diffame::mapping::Mapping;
use diffame::matcher::topdown::{dice_coefficient, match_top_down, DEFAULT_MIN_HEIGHT};
use diffame::tree::{Tree, TreeBuilder};

/// Builds (r (a x) (b y)) where leaves carry labels x and y.
fn small_tree(left_label: &str, right_label: &str) -> Tree {
    let mut builder = TreeBuilder::new();
    let root_id = builder.add("r", "", None, 0, 10);
    let left_branch = builder.add("a", "", Some(root_id), 0, 5);
    let _left_leaf = builder.add("leaf", left_label, Some(left_branch), 1, 2);
    let right_branch = builder.add("b", "", Some(root_id), 5, 10);
    let _right_leaf = builder.add("leaf", right_label, Some(right_branch), 6, 7);
    builder.build(root_id)
}

#[test]
fn identical_trees_are_fully_mapped() {
    let source_tree = small_tree("x", "y");
    let destination_tree = small_tree("x", "y");
    let mut mapping = Mapping::new();
    match_top_down(
        &source_tree,
        &destination_tree,
        &mut mapping,
        DEFAULT_MIN_HEIGHT,
    );
    assert!(mapping.has_src(source_tree.root()));
    assert_eq!(
        mapping.get_dst(source_tree.root()),
        Some(destination_tree.root())
    );
    assert_eq!(mapping.len(), source_tree.node_count());
}

#[test]
fn completely_different_trees_yield_no_mapping() {
    let mut source_builder = TreeBuilder::new();
    let source_root = source_builder.add("alpha", "", None, 0, 5);
    let _ = source_builder.add("alpha_child", "a", Some(source_root), 0, 1);
    let source_tree = source_builder.build(source_root);

    let mut destination_builder = TreeBuilder::new();
    let destination_root = destination_builder.add("beta", "", None, 0, 5);
    let _ = destination_builder.add("beta_child", "b", Some(destination_root), 0, 1);
    let destination_tree = destination_builder.build(destination_root);

    let mut mapping = Mapping::new();
    match_top_down(
        &source_tree,
        &destination_tree,
        &mut mapping,
        DEFAULT_MIN_HEIGHT,
    );
    assert!(mapping.is_empty());
}

#[test]
fn shared_subtree_is_anchored() {
    let mut source_builder = TreeBuilder::new();
    let source_root = source_builder.add("root", "", None, 0, 0);
    let source_sub = source_builder.add("sub", "", Some(source_root), 0, 0);
    let source_mid = source_builder.add("mid", "", Some(source_sub), 0, 0);
    let _ = source_builder.add("x", "1", Some(source_mid), 0, 0);
    let source_extra = source_builder.add("extra", "", Some(source_root), 0, 0);
    let _ = source_builder.add("xx", "z", Some(source_extra), 0, 0);
    let source_tree = source_builder.build(source_root);

    let mut destination_builder = TreeBuilder::new();
    let destination_root = destination_builder.add("root", "", None, 0, 0);
    let other = destination_builder.add("other", "", Some(destination_root), 0, 0);
    let _ = destination_builder.add("yy", "w", Some(other), 0, 0);
    let destination_sub = destination_builder.add("sub", "", Some(destination_root), 0, 0);
    let destination_mid = destination_builder.add("mid", "", Some(destination_sub), 0, 0);
    let _ = destination_builder.add("x", "1", Some(destination_mid), 0, 0);
    let destination_tree = destination_builder.build(destination_root);

    let mut mapping = Mapping::new();
    match_top_down(
        &source_tree,
        &destination_tree,
        &mut mapping,
        DEFAULT_MIN_HEIGHT,
    );

    assert_eq!(mapping.get_dst(source_sub), Some(destination_sub));
}

#[test]
fn min_height_threshold_excludes_small_subtrees() {
    let mut source_builder = TreeBuilder::new();
    let source_root = source_builder.add("root", "", None, 0, 0);
    let small_subtree = source_builder.add("small", "", Some(source_root), 0, 0);
    let _ = source_builder.add("leaf", "v", Some(small_subtree), 0, 0);
    let source_tree = source_builder.build(source_root);

    let mut destination_builder = TreeBuilder::new();
    let destination_root = destination_builder.add("Root", "", None, 0, 0);
    let small_subtree_dest = destination_builder.add("small", "", Some(destination_root), 0, 0);
    let _ = destination_builder.add("leaf", "v", Some(small_subtree_dest), 0, 0);
    let destination_tree = destination_builder.build(destination_root);

    let mut mapping = Mapping::new();
    match_top_down(&source_tree, &destination_tree, &mut mapping, 2);
    assert!(!mapping.has_src(small_subtree));
}

#[test]
fn lowering_min_height_unlocks_smaller_subtrees() {
    let mut source_builder = TreeBuilder::new();
    let source_root = source_builder.add("root", "", None, 0, 0);
    let small_subtree = source_builder.add("small", "", Some(source_root), 0, 0);
    let _ = source_builder.add("leaf", "v", Some(small_subtree), 0, 0);
    let source_tree = source_builder.build(source_root);

    let mut destination_builder = TreeBuilder::new();
    let destination_root = destination_builder.add("Root", "", None, 0, 0);
    let small_subtree_dest = destination_builder.add("small", "", Some(destination_root), 0, 0);
    let _ = destination_builder.add("leaf", "v", Some(small_subtree_dest), 0, 0);
    let destination_tree = destination_builder.build(destination_root);

    let mut mapping = Mapping::new();
    match_top_down(&source_tree, &destination_tree, &mut mapping, 1);
    assert_eq!(mapping.get_dst(small_subtree), Some(small_subtree_dest));
}

#[test]
fn maps_only_unique_isomorphic_anchors_directly() {
    let mut source_builder = TreeBuilder::new();
    let source_root = source_builder.add("root", "", None, 0, 0);
    let source_subtree = source_builder.add("S", "", Some(source_root), 0, 0);
    let source_child = source_builder.add("child", "", Some(source_subtree), 0, 0);
    let _ = source_builder.add("leaf", "v", Some(source_child), 0, 0);
    let source_tree = source_builder.build(source_root);

    let mut destination_builder = TreeBuilder::new();
    let destination_root = destination_builder.add("root", "", None, 0, 0);
    let destination_subtree = destination_builder.add("S", "", Some(destination_root), 0, 0);
    let destination_child = destination_builder.add("child", "", Some(destination_subtree), 0, 0);
    let _ = destination_builder.add("leaf", "v", Some(destination_child), 0, 0);
    let destination_tree = destination_builder.build(destination_root);

    let mut mapping = Mapping::new();
    match_top_down(
        &source_tree,
        &destination_tree,
        &mut mapping,
        DEFAULT_MIN_HEIGHT,
    );
    assert_eq!(mapping.get_dst(source_subtree), Some(destination_subtree));
}

#[test]
fn dice_coefficient_zero_for_unmatched_subtrees() {
    let source_tree = small_tree("x", "y");
    let destination_tree = small_tree("a", "b");
    let mapping = Mapping::new();
    assert_eq!(
        dice_coefficient(
            &source_tree,
            source_tree.root(),
            &destination_tree,
            destination_tree.root(),
            &mapping
        ),
        0.0
    );
}

#[test]
fn dice_coefficient_one_when_all_descendants_mapped() {
    let source_tree = small_tree("x", "y");
    let destination_tree = small_tree("x", "y");
    let mut mapping = Mapping::new();
    let source_descendants = source_tree.descendants(source_tree.root());
    let destination_descendants = destination_tree.descendants(destination_tree.root());
    for (source_node, destination_node) in source_descendants
        .iter()
        .zip(destination_descendants.iter())
    {
        mapping.link(*source_node, *destination_node);
    }
    let dice = dice_coefficient(
        &source_tree,
        source_tree.root(),
        &destination_tree,
        destination_tree.root(),
        &mapping,
    );
    assert!((dice - 1.0).abs() < 1e-9);
}

#[test]
fn ambiguous_subtrees_resolved_by_position() {
    // Two identical (attr (inner (leaf "x"))) subtrees, each under a different
    // container.  Each container also has a unique child, so containers are
    // uniquely anchored.  The comparator chain should use the sibling-dice
    // (level 1) to match each attr to the copy under its mapped parent.
    //
    //   source: root -> [cA(uniq_a, attr), cB(uniq_b, attr)]
    //   dest:   root -> [cA(uniq_a, attr), cB(uniq_b, attr)]

    let mut src_b = TreeBuilder::new();
    let src_root = src_b.add("root", "", None, 0, 100);

    let src_ca = src_b.add("container_a", "", Some(src_root), 0, 40);
    let src_attr0 = src_b.add("attr", "", Some(src_ca), 10, 30);
    let src_attr0_i = src_b.add("inner", "", Some(src_attr0), 10, 25);
    let _ = src_b.add("leaf", "x", Some(src_attr0_i), 10, 15);

    let src_cb = src_b.add("container_b", "", Some(src_root), 40, 80);
    let src_attr1 = src_b.add("attr", "", Some(src_cb), 50, 70);
    let src_attr1_i = src_b.add("inner", "", Some(src_attr1), 50, 65);
    let _ = src_b.add("leaf", "x", Some(src_attr1_i), 50, 55);

    let source_tree = src_b.build(src_root);

    let mut dst_b = TreeBuilder::new();
    let dst_root = dst_b.add("root", "", None, 0, 100);

    let dst_ca = dst_b.add("container", "", Some(dst_root), 0, 40);
    let dst_ua = dst_b.add("uniq_a", "", Some(dst_ca), 0, 10);
    let _ = dst_b.add("leaf", "aaa", Some(dst_ua), 0, 5);
    let dst_attr0 = dst_b.add("attr", "", Some(dst_ca), 10, 30);
    let dst_attr0_i = dst_b.add("inner", "", Some(dst_attr0), 10, 25);
    let _ = dst_b.add("leaf", "x", Some(dst_attr0_i), 10, 15);

    let dst_cb = dst_b.add("container", "", Some(dst_root), 40, 80);
    let dst_ub = dst_b.add("uniq_b", "", Some(dst_cb), 40, 50);
    let _ = dst_b.add("leaf", "bbb", Some(dst_ub), 40, 45);
    let dst_attr1 = dst_b.add("attr", "", Some(dst_cb), 50, 70);
    let dst_attr1_i = dst_b.add("inner", "", Some(dst_attr1), 50, 65);
    let _ = dst_b.add("leaf", "x", Some(dst_attr1_i), 50, 55);

    let destination_tree = dst_b.build(dst_root);

    let mut mapping = Mapping::new();
    match_top_down(
        &source_tree,
        &destination_tree,
        &mut mapping,
        DEFAULT_MIN_HEIGHT,
    );

    // Each attr must match the copy under its own mapped parent.
    assert_eq!(
        mapping.get_dst(src_attr0),
        Some(dst_attr0),
        "attr under container_A must match dst attr under container_A"
    );
    assert_eq!(
        mapping.get_dst(src_attr1),
        Some(dst_attr1),
        "attr under container_B must match dst attr under container_B"
    );
}
