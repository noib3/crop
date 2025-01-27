use alloc::vec::Vec;
use core::ops::Range;

use super::*;

/// A self-balancing tree with metadata stored in each node.
#[derive(Default)]
pub struct Tree<const ARITY: usize, L: Leaf> {
    pub(super) root: Arc<Node<ARITY, L>>,
}

impl<const ARITY: usize, L: Leaf> Clone for Tree<ARITY, L> {
    #[inline]
    fn clone(&self) -> Self {
        Tree { root: Arc::clone(&self.root) }
    }
}

impl<const ARITY: usize, L: Leaf> core::fmt::Debug for Tree<ARITY, L> {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        if !f.alternate() {
            f.debug_struct("Tree").field("root", &self.root).finish()
        } else {
            write!(f, "{:#?}", self.root)
        }
    }
}

impl<const ARITY: usize, L: BalancedLeaf + Clone> From<TreeSlice<'_, ARITY, L>>
    for Tree<ARITY, L>
{
    #[inline]
    fn from(slice: TreeSlice<'_, ARITY, L>) -> Tree<ARITY, L> {
        let root = if slice.base_measure() == slice.root().base_measure() {
            // If the TreeSlice and its root have the same base measure it
            // means the TreeSlice spanned the whole Tree from which it was
            // created and we can simply clone the root.
            Arc::clone(slice.root())
        } else if slice.leaf_count() == 1 {
            debug_assert!(slice.root().is_leaf());

            Arc::new(Node::Leaf(Lnode::new(
                slice.start_slice.into(),
                slice.summary,
            )))
        } else if slice.leaf_count() == 2 {
            let mut first = Lnode::from(L::from(slice.start_slice));

            let mut second = Lnode::from(L::from(slice.end_slice));

            first.balance(&mut second);

            let first = Arc::new(Node::Leaf(first));

            if !second.is_empty() {
                let second = Arc::new(Node::Leaf(second));
                let root = Inode::from_children([first, second]);
                Arc::new(Node::Internal(root))
            } else {
                first
            }
        } else {
            from_treeslice::into_tree_root(slice)
        };

        Tree { root }
    }
}

impl<const ARITY: usize, L: Leaf> Tree<ARITY, L> {
    #[doc(hidden)]
    pub fn assert_invariants(&self) {
        match &*self.root {
            Node::Internal(root) => {
                // The root is the only inode that can have as few as 2
                // children.
                assert!(root.len() >= 2 && root.len() <= ARITY);

                for child in root.children() {
                    child.assert_invariants()
                }
            },

            Node::Leaf(leaf) => leaf.assert_invariants(),
        }
    }

    #[inline]
    pub fn base_measure(&self) -> L::BaseMetric {
        self.measure::<L::BaseMetric>()
    }

    /// Returns the `M2`-measure of all the leaves before `up_to` plus the
    /// `M2`-measure of the left sub-slice of the leaf at `up_to`.
    #[track_caller]
    #[inline]
    pub fn convert_measure<M1, M2>(&self, up_to: M1) -> M2
    where
        M1: SlicingMetric<L>,
        M2: Metric<L::Summary>,
    {
        debug_assert!(up_to <= self.measure::<M1>());
        self.root.convert_measure(up_to)
    }

    /// Creates a new `Tree` from a sequence of leaves.
    ///
    /// If the iterator doesn't yield any items the `Tree` will contain a
    /// single leaf with its default value.
    #[inline]
    pub fn from_leaves<I>(leaves: I) -> Self
    where
        I: IntoIterator<Item = L>,
        L: Default,
    {
        let mut leaves =
            leaves.into_iter().map(Lnode::from).map(Node::Leaf).map(Arc::new);

        let Some(first) = leaves.next() else { return Self::default() };

        let Some(second) = leaves.next() else { return Self { root: first } };

        let leaves = {
            let (lo, hi) = leaves.size_hint();
            let mut l = Vec::with_capacity(2 + hi.unwrap_or(lo));
            l.push(first);
            l.push(second);
            l.extend(leaves);
            l
        };

        let root = Inode::from_nodes(leaves);

        Self { root: Arc::new(Node::Internal(root)) }
    }

    /// Returns `true` if the two roots point to the same allocation.
    #[inline]
    pub fn is_instance(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.root, &other.root)
    }

    /// Returns the leaf containing the `measure`-th unit of the `M`-metric,
    /// plus the `M`-measure of all the leaves before it.
    #[inline]
    pub fn leaf_at_measure<M>(&self, measure: M) -> (L::Slice<'_>, M)
    where
        M: Metric<L::Summary>,
    {
        debug_assert!(measure <= self.measure::<M>() + M::one());

        self.root.leaf_at_measure(measure)
    }

    #[inline]
    pub fn leaf_count(&self) -> usize {
        self.root.leaf_count()
    }

    /// Returns an iterator over the leaves of this `Tree`.
    #[inline]
    pub fn leaves(&self) -> Leaves<'_, ARITY, L> {
        Leaves::from(self)
    }

    /// Returns the `M`-measure of this `Tree` obtaining by summing up the
    /// `M`-measures of all its leaves.
    #[inline]
    pub fn measure<M>(&self) -> M
    where
        M: Metric<L::Summary>,
    {
        M::measure(self.summary())
    }

    /// Replaces a range of the `Tree` with the given replacement.
    #[track_caller]
    #[inline]
    pub fn replace<M>(
        &mut self,
        range: Range<M>,
        replace_with: L::Replacement<'_>,
    ) where
        M: Metric<L::Summary>,
        L: ReplaceableLeaf<M> + Clone,
    {
        if let Some(extras) =
            tree_replace::replace(&mut self.root, range, replace_with)
        {
            debug_assert!(extras
                .iter()
                .all(|n| n.depth() == self.root.depth()));

            self.root = Arc::new(Node::Internal(Inode::from_nodes(
                core::iter::once(Arc::clone(self.root())).exact_chain(extras),
            )));
        }
    }

    #[inline]
    pub(super) fn root(&self) -> &Arc<Node<ARITY, L>> {
        &self.root
    }

    /// Returns a slice of the `Tree` in the range of the given metric.
    #[track_caller]
    #[inline]
    pub fn slice<M>(&self, range: Range<M>) -> TreeSlice<'_, ARITY, L>
    where
        M: SlicingMetric<L>,
        L::BaseMetric: SlicingMetric<L>,
        for<'d> L::Slice<'d>: Default,
    {
        debug_assert!(M::zero() <= range.start);
        debug_assert!(range.start <= range.end);
        debug_assert!(range.end <= self.measure::<M>() + M::one());

        TreeSlice::from_range_in_root(&self.root, range)
    }

    #[inline]
    pub fn summary(&self) -> &L::Summary {
        self.root.summary()
    }

    /// Returns an iterator over the `M`-units of this `Tree`.
    #[inline]
    pub fn units<M>(&self) -> Units<'_, ARITY, L, M>
    where
        M: Metric<L::Summary>,
        for<'d> L::Slice<'d>: Default,
    {
        Units::from(self)
    }
}

mod from_treeslice {
    //! This module handles the logic used to convert `TreeSlice`s into
    //! `Tree`s.

    use super::*;

    /// Converts a `TreeSlice` into the root of an equivalent `Tree`,
    /// rebalancing if necessary.
    ///
    /// # Panics
    ///
    /// This function can only be called if the slice spans at least 3 leaves.
    /// Leaf counts of 1 and 2 must be handled by the caller.
    #[inline]
    pub(super) fn into_tree_root<const N: usize, L: BalancedLeaf + Clone>(
        slice: TreeSlice<'_, N, L>,
    ) -> Arc<Node<N, L>> {
        debug_assert!(slice.leaf_count() >= 3);

        let (root, invalid_in_first, invalid_in_last) = cut_tree_slice(slice);

        let mut root = Arc::new(Node::Internal(root));

        if invalid_in_first > 0 {
            {
                let inode =
                    Arc::get_mut(&mut root).unwrap().get_internal_mut();

                inode.balance_left_side();
            }

            Node::replace_with_single_child(&mut root);
        }

        if invalid_in_last > 0 {
            {
                // For the root to become a leaf node after the previous call
                // to `pull_up_singular` the TreeSlice would've had to span 2
                // leaves, and that case case should have already been handled
                // before calling this function.
                let inode =
                    Arc::get_mut(&mut root).unwrap().get_internal_mut();

                inode.balance_right_side();
            }

            Node::replace_with_single_child(&mut root);
        }

        root
    }

    /// Returns a `(root, invalid_first, invalid_last)` tuple where:
    ///
    /// - `root` is the internal node obtained by removing all the nodes before
    ///   `slice.before` and after `slice.before + slice.base_measure`,
    ///
    /// - `invalid_{first,last}` are the number of invalid nodes contained in
    ///   the subtrees of the first and last child, respectively.
    ///
    /// Note that all the `Arc`s enclosing the nodes on the left and right side
    /// of the subtree under `root` are guaranteed to have a strong count of 1,
    /// so it's ok to call `Arc::get_mut().unwrap()` on them, while the nodes
    /// in the middle will usually be `Arc::clone`d from the slice.
    ///
    /// # Panics
    ///
    /// Panics if the slice spans less than 3 leaves.
    #[inline]
    fn cut_tree_slice<const N: usize, L: BalancedLeaf + Clone>(
        slice: TreeSlice<'_, N, L>,
    ) -> (Inode<N, L>, usize, usize) {
        debug_assert!(slice.leaf_count() >= 3);

        let mut root = Inode::empty();
        let mut invalid_first = 0;
        let mut invalid_last = 0;

        let mut offset = L::BaseMetric::zero();

        // The slice's leaf count is > 1 so its root has to be an internal
        // node.
        let mut children = slice.root().get_internal().children().iter();

        let start = L::BaseMetric::measure(&slice.offset);

        for child in children.by_ref() {
            let this = child.base_measure();

            if offset + this > start {
                if start == L::BaseMetric::zero() {
                    root.push(Arc::clone(child));
                } else {
                    let first = cut_start_rec(
                        child,
                        start - offset,
                        slice.start_slice,
                        slice.start_summary.clone(),
                        &mut invalid_first,
                    );

                    root.push(first);
                }

                offset += this;
                break;
            } else {
                offset += this;
            }
        }

        let end = start + slice.base_measure();

        for child in children {
            let this = child.base_measure();

            if offset + this >= end {
                if end == slice.root().base_measure() {
                    root.push(Arc::clone(child));
                } else {
                    let last = cut_end_rec(
                        child,
                        end - offset,
                        slice.end_slice,
                        slice.end_summary.clone(),
                        &mut invalid_last,
                    );

                    root.push(last);
                }

                break;
            } else {
                root.push(Arc::clone(child));
                offset += this;
            }
        }

        (root, invalid_first, invalid_last)
    }

    /// Recursively removes all the nodes before `take_from`, replacing the
    /// leaf at `take_from` with `start_slice`. Returns the resulting node.
    #[inline]
    fn cut_start_rec<const N: usize, L: BalancedLeaf + Clone>(
        node: &Arc<Node<N, L>>,
        take_from: L::BaseMetric,
        start_slice: L::Slice<'_>,
        start_summary: L::Summary,
        invalid_nodes: &mut usize,
    ) -> Arc<Node<N, L>> {
        match &**node {
            Node::Internal(i) => {
                let mut inode = Inode::empty();

                let mut offset = L::BaseMetric::zero();

                let mut children = i.children().iter();

                for child in children.by_ref() {
                    let this = child.base_measure();

                    if offset + this > take_from {
                        let first = cut_start_rec(
                            child,
                            take_from - offset,
                            start_slice,
                            start_summary,
                            invalid_nodes,
                        );

                        let first_is_underfilled = first.is_underfilled();

                        inode.push(first);

                        for child in children {
                            inode.push(Arc::clone(child));
                        }

                        if first_is_underfilled && inode.len() > 1 {
                            inode.balance_first_child_with_second();
                            *invalid_nodes -= 1;
                        }

                        if inode.is_underfilled() {
                            *invalid_nodes += 1;
                        }

                        return Arc::new(Node::Internal(inode));
                    } else {
                        offset += this;
                    }
                }

                unreachable!();
            },

            Node::Leaf(_) => {
                let lnode = Lnode::new(start_slice.into(), start_summary);

                if lnode.is_underfilled() {
                    *invalid_nodes += 1;
                }

                Arc::new(Node::Leaf(lnode))
            },
        }
    }

    /// Recursively removes all the nodes after `take_up_to`, replacing the
    /// leaf at `take_up_to` with `end_slice`. Returns the resulting node.
    #[inline]
    fn cut_end_rec<const N: usize, L: BalancedLeaf + Clone>(
        node: &Arc<Node<N, L>>,
        take_up_to: L::BaseMetric,
        end_slice: L::Slice<'_>,
        end_summary: L::Summary,
        invalid_nodes: &mut usize,
    ) -> Arc<Node<N, L>> {
        match &**node {
            Node::Internal(i) => {
                let mut inode = Inode::empty();

                let mut offset = L::BaseMetric::zero();

                for child in i.children() {
                    let this = child.base_measure();

                    if offset + this >= take_up_to {
                        let last = cut_end_rec(
                            child,
                            take_up_to - offset,
                            end_slice,
                            end_summary,
                            invalid_nodes,
                        );

                        let last_is_underfilled = last.is_underfilled();

                        inode.push(last);

                        if last_is_underfilled && inode.len() > 1 {
                            inode.balance_last_child_with_penultimate();
                            *invalid_nodes -= 1;
                        }

                        if inode.is_underfilled() {
                            *invalid_nodes += 1;
                        }

                        return Arc::new(Node::Internal(inode));
                    } else {
                        inode.push(Arc::clone(child));
                        offset += this;
                    }
                }

                unreachable!();
            },

            Node::Leaf(_) => {
                let lnode = Lnode::new(end_slice.into(), end_summary);

                if lnode.is_underfilled() {
                    *invalid_nodes = 1;
                }

                Arc::new(Node::Leaf(lnode))
            },
        }
    }
}

mod tree_replace {
    //! This module contains the logic used to implement [`Tree::replace()`].

    use super::*;

    /// Recursively calls itself until it finds the deepest node that fully
    /// contains `range`, calling [`replace_range_in_deepest`] if it's an
    /// internal node.
    ///
    /// It can return a vector of extra nodes (of the same depth as `node`) to
    /// be inserted *right after* `node` if the replacement was
    /// insertion-heavy.
    ///
    /// Viceversa, if the replacement was deletion-heavy it'll usually return
    /// `None`, and `node` could be underfilled or even at a lower depth than
    /// it was before calling this function.
    #[track_caller]
    #[inline]
    pub(super) fn replace<const N: usize, M, L>(
        node: &mut Arc<Node<N, L>>,
        mut range: Range<M>,
        replace_with: L::Replacement<'_>,
    ) -> Option<Vec<Arc<Node<N, L>>>>
    where
        M: Metric<L::Summary>,
        L: ReplaceableLeaf<M> + Clone,
    {
        let inode = match Arc::make_mut(node) {
            Node::Internal(inode) => inode,

            Node::Leaf(leaf) => {
                return leaf.replace(range, replace_with).map(|extras| {
                    extras.map(Node::Leaf).map(Arc::new).collect()
                });
            },
        };

        let Some((child_idx, offset)) =
            inode.child_containing_range(range.clone())
        else {
            let extras = replace_range_in_deepest(inode, range, replace_with);

            Node::replace_with_single_child(node);

            return extras;
        };

        range.start -= offset;
        range.end -= offset;

        let extras = inode.with_child_mut(child_idx, |child| {
            replace(child, range, replace_with)
        });

        let child = inode.child(child_idx);

        // Case 1: there are some extra child nodes to insert *after* the child
        // containing the replacement range.
        if let Some(extras) = extras {
            debug_assert!(extras
                .iter()
                .all(|n| n.depth() == inode.depth() - 1));

            inode.insert_children(child_idx + 1, extras).map(|extras| {
                extras.map(Node::Internal).map(Arc::new).collect()
            })
        }
        // Case 2: the child stayed at the same depth but it's now underfilled
        // and needs to be rebalanced with one if its siblings.
        else if child.depth() == inode.depth() - 1 && child.is_underfilled()
        {
            inode.balance_child(child_idx);
            Node::replace_with_single_child(node);
            None
        }
        // Case 3: the child is now at a lower depth than its siblings and
        // needs to be appended/prepended to another child.
        else if child.depth() < inode.depth() - 1 {
            debug_assert!(inode.depth() >= 2);
            let child = inode.remove(child_idx);
            inode.insert_at_depth(child_idx, child);
            Node::replace_with_single_child(node);
            None
        }
        // Case 4: none of the above, the child is well balanced and we can
        // simply return.
        else {
            debug_assert!(child.depth() == inode.depth() - 1);
            debug_assert!(!child.is_underfilled());
            None
        }
    }

    /// Replaces the given range of the inode. At this point `inode` is assumed
    /// to be the deepest node that fully contains the range.
    ///
    /// It can return a vector of inodes of the same depth as `inode` if the
    /// replacement was insertion-heavy, and the inode can be underfilled (or
    /// even contain a single child) if it was deletion-heavy.
    #[track_caller]
    #[inline]
    fn replace_range_in_deepest<const N: usize, M, L>(
        inode: &mut Inode<N, L>,
        range: Range<M>,
        replace_with: L::Replacement<'_>,
    ) -> Option<Vec<Arc<Node<N, L>>>>
    where
        M: Metric<L::Summary>,
        L: ReplaceableLeaf<M> + Clone,
    {
        let (start_idx, end_idx, extra_leaves) =
            inode_replace_nodes_in_start_and_end_subtrees(
                inode,
                range,
                replace_with,
            );

        let mut extra_leaves = extra_leaves?;

        replace_child_range_with_leaves_from_back(
            inode,
            start_idx + 1..end_idx,
            &mut extra_leaves,
        );

        if extra_leaves.is_empty() {
            return None;
        }

        let extras = if inode.depth() == 1 {
            extra_leaves
        } else {
            let mut extra_leaves = extra_leaves.into_iter();

            let target_depth = inode.depth() - 1;

            let mut extras =
                TargetDepth::new(extra_leaves.by_ref(), target_depth)
                    .collect::<Vec<_>>();

            debug_assert!(extra_leaves.len() == 0);

            let mut last = extras.pop().unwrap();

            // If the depth is ok but there aren't enough children:
            //
            // if there's another node in extra use it to rebalance it.
            // if not use the start node.
            //
            // If the depth is < than the target
            //
            // if there's another node in extra use it to append at depth.
            // if not use the start node to rebalance it.

            if last.depth() == target_depth && !last.is_underfilled() {
                extras.push(last)
            } else {
                debug_assert!(inode.depth() >= 2);

                inode.with_child_mut(start_idx, |start| {
                    let previous_node = {
                        let n = extras.last_mut().unwrap_or(start);
                        // The inode's depth is >= 2 so all its children are
                        // internal nodes.
                        Arc::make_mut(n).get_internal_mut()
                    };

                    if last.depth() == previous_node.depth() {
                        debug_assert!(last.is_underfilled());

                        let l = Arc::make_mut(&mut last).get_internal_mut();

                        previous_node.balance(l);

                        if !l.is_empty() {
                            debug_assert!(!l.is_underfilled());
                            extras.push(last)
                        }
                    } else if let Some(extra) =
                        previous_node.append_at_depth(last)
                    {
                        extras.push(Arc::new(Node::Internal(extra)));
                    }
                });
            }

            extras
        };

        debug_assert!(extras.iter().all(|n| n.depth() == inode.depth() - 1));

        inode
            .insert_children(start_idx + 1, extras)
            .map(|extras| extras.map(Node::Internal).map(Arc::new).collect())
    }

    /// Returns the following values:
    ///
    /// `start_idx`: the index of the child containing the start of the range;
    ///
    /// `end_idx`: the index of the child containing the end of the range
    /// (strictly greater than `start_idx` because `inode` is assumed to be the
    /// deepest node fully containing the range);
    ///
    /// `extra_leaves`: a vector of leaf nodes to be inserted between
    /// `start_idx` and `end_idx`. This is only present if the replacement was
    /// insertion-heavy.
    #[track_caller]
    #[inline]
    fn inode_replace_nodes_in_start_and_end_subtrees<const N: usize, M, L>(
        inode: &mut Inode<N, L>,
        range: Range<M>,
        replace_with: L::Replacement<'_>,
    ) -> (usize, usize, Option<Vec<Arc<Node<N, L>>>>)
    where
        M: Metric<L::Summary>,
        L: ReplaceableLeaf<M> + Clone,
    {
        let mut start_idx = 0;
        let mut end_idx = 0;
        let mut start_should_rebalance = false;
        let mut end_should_rebalance = false;
        let mut extra_leaves = None;

        let mut offset = M::zero();
        let mut indexes = 0..inode.len();

        for (idx, child) in indexes.by_ref().map(|idx| (idx, inode.child(idx)))
        {
            let child_measure = child.measure::<M>();

            offset += child_measure;

            if offset >= range.start {
                start_idx = idx;

                extra_leaves = inode.with_child_mut(start_idx, |child| {
                    replace_nodes_in_start_subtree(
                        Arc::make_mut(child),
                        range.start + child_measure - offset,
                        replace_with,
                        &mut start_should_rebalance,
                    )
                });

                break;
            }
        }

        let mut extra_leaves = extra_leaves.map(|leaves| {
            let leaves = leaves.collect::<Vec<_>>();
            debug_assert!(!leaves.is_empty());
            debug_assert!(leaves.iter().all(|l| l.is_leaf()));
            leaves
        });

        for (idx, child) in indexes.map(|idx| (idx, inode.child(idx))) {
            let child_measure = child.measure::<M>();

            offset += child_measure;

            if offset >= range.end {
                end_idx = idx;

                inode.with_child_mut(end_idx, |child| {
                    replace_nodes_in_end_subtree(
                        Arc::make_mut(child),
                        range.end + child_measure - offset,
                        &mut extra_leaves,
                        &mut end_should_rebalance,
                    )
                });

                break;
            }
        }

        // There are no more leaves to insert, that means the replacement was
        // deletion-heavy and we can remove the children between (but not
        // including) `start_idx` and `end_idx`.
        if extra_leaves.is_none() {
            inode.drain(start_idx + 1..end_idx);

            if start_should_rebalance || end_should_rebalance {
                fix_seam_between_subtrees(
                    inode,
                    start_idx + 1,
                    start_should_rebalance,
                    end_should_rebalance,
                );
            }
        }

        (start_idx, end_idx, extra_leaves)
    }

    /// Recursively calls itself until it reaches the leaf containing the start
    /// of the replacement range, then uses any extra leaves returned by
    /// calling [`Lnode::replace()`] to replace the nodes after that leaf, or
    /// removes them if there are no extra leaves.
    #[track_caller]
    #[inline]
    fn replace_nodes_in_start_subtree<const N: usize, M, L>(
        node: &mut Node<N, L>,
        replace_from: M,
        replace_with: L::Replacement<'_>,
        should_rebalance: &mut bool,
    ) -> Option<impl ExactSizeIterator<Item = Arc<Node<N, L>>>>
    where
        M: Metric<L::Summary>,
        L: ReplaceableLeaf<M> + Clone,
    {
        let inode = match node {
            Node::Internal(inode) => inode,

            Node::Leaf(leaf) => {
                if let Some(extra_leaves) =
                    leaf.replace(replace_from.., replace_with)
                {
                    return Some(extra_leaves.map(Node::Leaf).map(Arc::new));
                } else {
                    *should_rebalance = leaf.is_underfilled();
                    return None;
                }
            },
        };

        let (start_idx, offset) = inode.child_at_measure(replace_from);

        let extra_leaves = inode.with_child_mut(start_idx, |child| {
            replace_nodes_in_start_subtree(
                Arc::make_mut(child),
                replace_from - offset,
                replace_with,
                should_rebalance,
            )
        });

        let extra_leaves = if let Some(mut extra_leaves) = extra_leaves {
            replace_child_range_with_leaves(
                inode,
                start_idx + 1..inode.len(),
                &mut extra_leaves,
            );

            (extra_leaves.len() > 0).then_some(extra_leaves)
        } else {
            inode.drain(start_idx + 1..);
            None
        };

        *should_rebalance |= inode.is_underfilled();

        extra_leaves
    }

    /// Recursively calls itself until it reaches the leaf containing the end
    /// of the replacement range, then uses the extra leaves to replace the
    /// nodes before that leaf, or removes them if there are no extra leaves.
    #[track_caller]
    #[inline]
    fn replace_nodes_in_end_subtree<const N: usize, M, L>(
        node: &mut Node<N, L>,
        replace_up_to: M,
        extra_leaves: &mut Option<Vec<Arc<Node<N, L>>>>,
        should_rebalance: &mut bool,
    ) where
        M: Metric<L::Summary>,
        L: ReplaceableLeaf<M> + Clone,
    {
        let inode = match node {
            Node::Internal(inode) => inode,

            Node::Leaf(leaf) => {
                leaf.remove_up_to(replace_up_to);

                if leaf.is_underfilled() {
                    if let Some(leaves) = extra_leaves {
                        let mut last = leaves.pop().unwrap();

                        let l =
                            Arc::get_mut(&mut last).unwrap().get_leaf_mut();

                        l.balance(leaf);

                        if leaf.is_empty() {
                            core::mem::swap(leaf, l)
                        } else {
                            leaves.push(last)
                        }

                        if leaves.is_empty() {
                            *extra_leaves = None;
                        }
                    }
                }

                *should_rebalance = leaf.is_underfilled();
                return;
            },
        };

        let mut end_idx = 0;

        let mut offset = inode.measure::<M>();

        for (idx, child) in inode.children().iter().enumerate().rev() {
            let child_measure = child.measure::<M>();

            offset -= child_measure;

            if offset < replace_up_to {
                end_idx = idx;

                inode.with_child_mut(end_idx, |child| {
                    replace_nodes_in_end_subtree(
                        Arc::make_mut(child),
                        replace_up_to - offset,
                        extra_leaves,
                        should_rebalance,
                    )
                });

                break;
            }
        }

        if let Some(leaves) = extra_leaves {
            replace_child_range_with_leaves_from_back(
                inode,
                0..end_idx,
                leaves,
            );

            if leaves.is_empty() {
                *extra_leaves = None;
            }
        } else {
            inode.drain(..end_idx);
        }

        *should_rebalance |= inode.is_underfilled();
    }

    /// Replaces `inode`'s children in the given index range by building nodes
    /// at the children's depth using the `leaves` iterator.
    ///
    /// If the iterator is exhausted before the whole range has been replaced
    /// it'll just remove the remaining children.
    #[inline]
    fn replace_child_range_with_leaves<const N: usize, L, I>(
        inode: &mut Inode<N, L>,
        child_range: Range<usize>,
        leaves: &mut I,
    ) where
        I: Iterator<Item = Arc<Node<N, L>>> + ExactSizeIterator,
        L: BalancedLeaf + Clone,
    {
        debug_assert!(child_range.start >= 1);
        debug_assert!(child_range.start <= child_range.end);
        debug_assert!(child_range.end <= inode.len());

        let end = child_range.end;

        if inode.depth() == 1 {
            for child_idx in child_range {
                if let Some(leaf) = leaves.next() {
                    debug_assert!(leaf.is_leaf());
                    inode.swap(child_idx, leaf);
                } else {
                    inode.drain(child_idx..end);
                    return;
                }
            }
        } else {
            let target_depth = inode.depth() - 1;

            let mut replacements =
                TargetDepth::new(leaves.by_ref(), target_depth);

            for child_idx in child_range {
                if let Some(replacement) = replacements.next() {
                    if replacement.depth() == target_depth
                        && !replacement.is_underfilled()
                    {
                        inode.swap(child_idx, replacement);
                    } else {
                        let mut last = replacement;

                        let last =
                            inode.with_child_mut(child_idx - 1, |previous| {
                                let previous_child =
                                    Arc::make_mut(previous).get_internal_mut();

                                if last.depth() == previous_child.depth() {
                                    debug_assert!(last.is_underfilled());

                                    let l = Arc::make_mut(&mut last)
                                        .get_internal_mut();

                                    previous_child.balance(l);

                                    (!l.is_empty()).then_some(last)
                                } else {
                                    previous_child
                                        .append_at_depth(last)
                                        .map(Node::Internal)
                                        .map(Arc::new)
                                }
                            });

                        if let Some(last) = last {
                            inode.swap(child_idx, last);
                            inode.drain(child_idx + 1..end);
                        } else {
                            inode.drain(child_idx..end);
                        }

                        debug_assert_eq!(0, leaves.len());

                        return;
                    }
                } else {
                    inode.drain(child_idx..end);
                    return;
                }
            }
        }
    }

    /// Same as [`replace_child_range_with_leaves_from_back`], except it
    /// replaces the children in the given index range going backwards, i.e.
    /// starting from the last child.
    #[inline]
    fn replace_child_range_with_leaves_from_back<const N: usize, L>(
        inode: &mut Inode<N, L>,
        child_range: Range<usize>,
        leaves: &mut Vec<Arc<Node<N, L>>>,
    ) where
        L: BalancedLeaf + Clone,
    {
        debug_assert!(child_range.start <= child_range.end);
        debug_assert!(child_range.end < inode.len());

        let start = child_range.start;

        if inode.depth() == 1 {
            for child_idx in child_range.rev() {
                if let Some(leaf) = leaves.pop() {
                    debug_assert!(leaf.is_leaf());
                    inode.swap(child_idx, leaf);
                } else {
                    inode.drain(start..child_idx + 1);
                    return;
                }
            }
        } else {
            let target_depth = inode.depth() - 1;

            let mut replacements =
                TargetDepthFromBack::new(leaves, target_depth);

            for child_idx in child_range.rev() {
                if let Some(replacement) = replacements.next() {
                    if replacement.depth() == target_depth
                        && !replacement.is_underfilled()
                    {
                        inode.swap(child_idx, replacement);
                    } else {
                        let mut last = replacement;

                        let last =
                            inode.with_child_mut(child_idx + 1, |next| {
                                let next_child =
                                    Arc::make_mut(next).get_internal_mut();

                                if last.depth() == next_child.depth() {
                                    debug_assert!(last.is_underfilled());

                                    let l = Arc::make_mut(&mut last)
                                        .get_internal_mut();

                                    l.balance(next_child);

                                    if next_child.is_empty() {
                                        *next = last;
                                        None
                                    } else {
                                        Some(last)
                                    }
                                } else {
                                    next_child
                                        .prepend_at_depth(last)
                                        .map(Node::Internal)
                                        .map(Arc::new)
                                }
                            });

                        if let Some(last) = last {
                            inode.swap(child_idx, last);
                            inode.drain(start..child_idx);
                        } else {
                            inode.drain(start..child_idx + 1);
                        }

                        debug_assert_eq!(0, leaves.len());

                        return;
                    }
                } else {
                    inode.drain(start..child_idx + 1);
                    return;
                }
            }
        }
    }

    /// Fixes the seam left by a deletion-heavy replacement in the given inode.
    ///
    /// The left and right side of the seam are under the children before and
    /// after the `seam_offset`, respectively.
    #[inline]
    fn fix_seam_between_subtrees<const N: usize, L>(
        inode: &mut Inode<N, L>,
        seam_offset: usize,
        start_should_rebalance: bool,
        end_should_rebalance: bool,
    ) where
        L: BalancedLeaf + Clone,
    {
        debug_assert!(seam_offset < inode.len());
        debug_assert!(start_should_rebalance | end_should_rebalance);

        let start_idx = seam_offset - 1;
        let end_idx = seam_offset;

        if start_should_rebalance {
            inode.with_child_mut(start_idx, |start| {
                Node::replace_with_single_child(start);

                if let Node::Internal(inode) = Arc::make_mut(start) {
                    inode.balance_right_side();
                    Node::replace_with_single_child(start);
                }
            })
        }

        if end_should_rebalance {
            inode.with_child_mut(end_idx, |end| {
                Node::replace_with_single_child(end);

                if let Node::Internal(inode) = &mut Arc::make_mut(end) {
                    inode.balance_left_side();
                    Node::replace_with_single_child(end);
                }
            })
        }

        let original_depth = inode.depth() - 1;

        let start_depth = inode.child(start_idx).depth();

        let end_depth = inode.child(end_idx).depth();

        use core::cmp::Ordering::*;

        match (original_depth.cmp(&start_depth), start_depth.cmp(&end_depth)) {
            (Equal, Equal) => {
                // Rebalance start and end, then rebalance start with
                // previous/next sibling.
                inode.balance_child(end_idx);

                if inode.len() > 1 {
                    inode.balance_child(start_idx);
                }
            },

            (Equal, Greater) => {
                // Append end on start, then rebalance start with previous/next
                // sibling.

                let end = inode.remove(end_idx);

                let end = inode.with_child_mut(start_idx, |start| {
                    let start = Arc::make_mut(start).get_internal_mut();
                    start.append_at_depth(end)
                });

                if let Some(end) = end {
                    debug_assert_eq!(end.depth(), original_depth);
                    inode.insert(end_idx, Arc::new(Node::Internal(end)));
                }

                if inode.len() > 1 {
                    inode.balance_child(start_idx);
                }
            },

            (Greater, Equal) => {
                let mut end = inode.remove(end_idx);
                let mut start = inode.remove(start_idx);

                Arc::make_mut(&mut start).balance(Arc::make_mut(&mut end));

                if !end.is_empty() {
                    let i = Inode::from_children([start, end]);

                    if inode.is_empty() {
                        *inode = i
                    } else if inode.depth() == i.depth() + 1 {
                        inode.insert(start_idx, Arc::new(Node::Internal(i)));
                        inode.balance_child(start_idx);
                    } else {
                        inode.insert_at_depth(
                            start_idx,
                            Arc::new(Node::Internal(i)),
                        );
                    }
                } else if inode.is_empty() {
                    inode.insert(0, start);
                } else {
                    inode.insert_at_depth(start_idx, start);
                }
            },

            (Greater, Greater) => {
                let end = inode.remove(end_idx);
                let mut start = inode.remove(start_idx);

                let end = {
                    let s = Arc::make_mut(&mut start).get_internal_mut();
                    s.append_at_depth(end)
                };

                if let Some(end) = end {
                    let i = Inode::from_children([
                        start,
                        Arc::new(Node::Internal(end)),
                    ]);

                    if inode.is_empty() {
                        *inode = i;
                    } else if inode.depth() == i.depth() + 1 {
                        inode.insert(start_idx, Arc::new(Node::Internal(i)));
                        inode.balance_child(start_idx);
                    } else {
                        inode.insert_at_depth(
                            start_idx,
                            Arc::new(Node::Internal(i)),
                        );
                    }
                } else if inode.is_empty() {
                    inode.insert(0, start);
                } else {
                    inode.insert_at_depth(start_idx, start);
                }
            },

            (Greater, Less) if end_depth == original_depth => {
                // Prepend start on end, then rebalance end with previous/next
                // sibling.

                let start = inode.remove(start_idx);

                let start = inode.with_child_mut(start_idx, |end| {
                    let end = Arc::make_mut(end).get_internal_mut();
                    end.prepend_at_depth(start)
                });

                if let Some(start) = start {
                    inode.insert(start_idx, Arc::new(Node::Internal(start)));
                    inode.balance_child(end_idx);
                } else if inode.len() > 1 {
                    inode.balance_child(start_idx);
                }
            },

            (Greater, Less) => {
                debug_assert!(original_depth > end_depth);

                let mut end = inode.remove(end_idx);
                let start = inode.remove(start_idx);

                let start = {
                    let e = Arc::make_mut(&mut end).get_internal_mut();
                    e.prepend_at_depth(start)
                };

                if let Some(start) = start {
                    let i = Inode::from_children([
                        Arc::new(Node::Internal(start)),
                        end,
                    ]);

                    if inode.is_empty() {
                        *inode = i;
                    } else if inode.depth() == i.depth() + 1 {
                        inode.insert(start_idx, Arc::new(Node::Internal(i)));
                        inode.balance_child(start_idx);
                    } else {
                        inode.insert_at_depth(
                            start_idx,
                            Arc::new(Node::Internal(i)),
                        );
                    }
                } else if inode.is_empty() {
                    inode.insert(0, end);
                } else {
                    inode.insert_at_depth(start_idx, end);
                }
            },

            _ => unreachable!(),
        }
    }

    use iter::{TargetDepth, TargetDepthFromBack};

    mod iter {
        //! Iterators transforming iterators/vectors of leaf nodes into
        //! internal nodes at some target depth.

        use super::*;

        /// The minimum number of leaves required by [`Inode::from_nodes()`] to
        /// produce an internal node of the target depth with at least
        /// [`Inode::min_children()`] children.
        const fn min_leaves_for_depth<const N: usize, L: Leaf>(
            target_depth: usize,
        ) -> usize {
            (Inode::<N, L>::min_children() - 1)
                * max_leaves_for_depth::<N, L>(target_depth - 1)
                + 1
        }

        /// The maximum number of leaves that can be fed to
        /// [`Inode::from_nodes()`] to produce an internal node of the target
        /// depth with no more than [`Inode::max_children()`] children.
        const fn max_leaves_for_depth<const N: usize, L: Leaf>(
            target_depth: usize,
        ) -> usize {
            Inode::<N, L>::max_children().pow(target_depth as u32)
        }

        /// Transforms an iterator over leaf nodes into internal nodes at a
        /// given target depth that are all guaranteed to have between
        /// `min_children` and `max_children` children, except for the last
        /// node which can be at a lower depth than the target (can even be a
        /// leaf node) and contain less than `min_children` children.
        pub(super) struct TargetDepth<const N: usize, L, Leaves>
        where
            L: Leaf,
            Leaves: ExactSizeIterator<Item = Arc<Node<N, L>>>,
        {
            leaves: Leaves,
            target_depth: usize,
            min_leaves_for_depth: usize,
            max_leaves_for_depth: usize,
        }

        impl<const N: usize, L, Leaves> TargetDepth<N, L, Leaves>
        where
            L: Leaf,
            Leaves: ExactSizeIterator<Item = Arc<Node<N, L>>>,
        {
            /// # Panics
            ///
            /// Panics if `leaves` yields 0 leaves or if the target_depth is 0.
            #[inline]
            pub(in crate::tree) fn new(
                leaves: Leaves,
                target_depth: usize,
            ) -> Self {
                debug_assert!(leaves.len() > 0);
                debug_assert!(target_depth > 0);

                Self {
                    leaves,
                    target_depth,
                    min_leaves_for_depth: min_leaves_for_depth::<N, L>(
                        target_depth,
                    ),
                    max_leaves_for_depth: max_leaves_for_depth::<N, L>(
                        target_depth,
                    ),
                }
            }
        }

        impl<const N: usize, L, Leaves> Iterator for TargetDepth<N, L, Leaves>
        where
            L: Leaf,
            Leaves: ExactSizeIterator<Item = Arc<Node<N, L>>>,
        {
            type Item = Arc<Node<N, L>>;

            #[inline]
            fn next(&mut self) -> Option<Self::Item> {
                let remaining = self.leaves.len();

                if remaining == 0 {
                    None
                } else if remaining < self.min_leaves_for_depth {
                    let last = if self.leaves.len() == 1 {
                        self.leaves.next().unwrap()
                    } else {
                        let last = Inode::from_nodes(self.leaves.by_ref());

                        debug_assert!(
                            last.depth() < self.target_depth
                                || last.len() < Inode::<N, L>::min_children()
                        );

                        debug_assert!(
                            last.len() <= Inode::<N, L>::max_children()
                        );

                        Arc::new(Node::Internal(last))
                    };

                    Some(last)
                } else {
                    let inode = Inode::from_nodes(
                        self.leaves.by_ref().take(self.max_leaves_for_depth),
                    );

                    debug_assert_eq!(inode.depth(), self.target_depth);

                    debug_assert!(
                        inode.len() >= Inode::<N, L>::min_children()
                    );

                    debug_assert!(
                        inode.len() <= Inode::<N, L>::max_children()
                    );

                    Some(Arc::new(Node::Internal(inode)))
                }
            }
        }

        /// Same as `TargetDepth` except the inodes are constructed from back
        /// to front instead of front to back by draining the nodes off of the
        /// vector.
        pub(super) struct TargetDepthFromBack<'a, const N: usize, L>
        where
            L: Leaf,
        {
            leaves: &'a mut Vec<Arc<Node<N, L>>>,
            target_depth: usize,
            min_leaves_for_depth: usize,
            max_leaves_for_depth: usize,
        }

        impl<'a, const N: usize, L> TargetDepthFromBack<'a, N, L>
        where
            L: Leaf,
        {
            /// # Panics
            ///
            /// Panics if `leaves` is empty or if the target_depth is 0.
            #[inline]
            pub(in crate::tree) fn new(
                leaves: &'a mut Vec<Arc<Node<N, L>>>,
                target_depth: usize,
            ) -> Self {
                debug_assert!(!leaves.is_empty());
                debug_assert!(target_depth > 0);

                Self {
                    leaves,
                    target_depth,
                    min_leaves_for_depth: min_leaves_for_depth::<N, L>(
                        target_depth,
                    ),
                    max_leaves_for_depth: max_leaves_for_depth::<N, L>(
                        target_depth,
                    ),
                }
            }
        }

        impl<const N: usize, L> Iterator for TargetDepthFromBack<'_, N, L>
        where
            L: Leaf,
        {
            type Item = Arc<Node<N, L>>;

            #[inline]
            fn next(&mut self) -> Option<Self::Item> {
                // TODO: this feels a bit repetitive with `TargetDepth`, can we
                // DRY it up?

                let remaining = self.leaves.len();

                if remaining == 0 {
                    None
                } else if remaining < self.min_leaves_for_depth {
                    let last = if self.leaves.len() == 1 {
                        self.leaves.pop().unwrap()
                    } else {
                        let last = Inode::from_nodes(self.leaves.drain(..));

                        debug_assert!(
                            last.depth() < self.target_depth
                                || last.len() < Inode::<N, L>::min_children()
                        );

                        debug_assert!(
                            last.len() <= Inode::<N, L>::max_children()
                        );

                        Arc::new(Node::Internal(last))
                    };

                    Some(last)
                } else {
                    let inode = Inode::from_nodes(
                        self.leaves.drain(
                            self.leaves
                                .len()
                                .saturating_sub(self.max_leaves_for_depth)..,
                        ),
                    );

                    debug_assert_eq!(inode.depth(), self.target_depth);

                    debug_assert!(
                        inode.len() >= Inode::<N, L>::min_children()
                    );

                    debug_assert!(
                        inode.len() <= Inode::<N, L>::max_children()
                    );

                    Some(Arc::new(Node::Internal(inode)))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use core::ops::{Add, AddAssign, Sub, SubAssign};

    use super::*;

    #[derive(Copy, Clone, Default, Debug, Eq, PartialEq)]
    pub struct Count {
        count: usize,
        leaves: usize,
    }

    impl Add<&Self> for Count {
        type Output = Self;

        #[inline]
        fn add(self, rhs: &Self) -> Self {
            Count {
                count: self.count + rhs.count,
                leaves: self.leaves + rhs.leaves,
            }
        }
    }

    impl Sub<&Self> for Count {
        type Output = Self;

        #[inline]
        fn sub(self, rhs: &Self) -> Self {
            Count {
                count: self.count - rhs.count,
                leaves: self.leaves - rhs.leaves,
            }
        }
    }

    impl AddAssign<&Self> for Count {
        fn add_assign(&mut self, rhs: &Self) {
            self.count += rhs.count;
            self.leaves += rhs.leaves;
        }
    }

    impl SubAssign<&Self> for Count {
        fn sub_assign(&mut self, rhs: &Self) {
            self.count -= rhs.count;
            self.leaves -= rhs.leaves;
        }
    }

    impl Summarize for usize {
        type Summary = Count;

        fn summarize(&self) -> Self::Summary {
            Count { count: *self, leaves: 1 }
        }
    }

    type LeavesMetric = usize;

    #[derive(Copy, Clone, Debug, PartialEq)]
    pub struct UsizeSlice<'a>(pub &'a usize);

    impl From<UsizeSlice<'_>> for usize {
        fn from(s: UsizeSlice<'_>) -> Self {
            *s.0
        }
    }

    impl Summarize for UsizeSlice<'_> {
        type Summary = Count;

        fn summarize(&self) -> Self::Summary {
            self.0.summarize()
        }
    }

    impl AsSlice for usize {
        type Slice<'a> = UsizeSlice<'a>;

        fn as_slice(&self) -> UsizeSlice<'_> {
            UsizeSlice(self)
        }
    }

    impl Metric<Count> for LeavesMetric {
        fn zero() -> Self {
            0
        }

        fn one() -> Self {
            1
        }

        fn measure(count: &Count) -> Self {
            count.leaves
        }
    }

    impl BaseMeasured for usize {
        type BaseMetric = LeavesMetric;
    }

    #[test]
    fn easy() {
        let tree = Tree::<4, usize>::from_leaves(0..20);
        assert_eq!(190, tree.summary().count);
    }
}
