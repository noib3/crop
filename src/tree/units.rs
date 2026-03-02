use alloc::vec::Vec;

use super::traits::{
    DoubleEndedUnitMetric,
    Leaf,
    LeafSlice,
    Metric,
    Summary,
    UnitMetric,
};
use super::tree_slice;
use super::{Arc, Node, Tree, TreeSlice};

/// An iterator over the units of a metric.
//
// Just like the `Leaves` iterator, this iterator is also implemented using two
// separate iterators, one for iterating forward (used in the `Iterator` impl),
// and the other for iterating backward (used in the `DoubleEndedIterator`
// impl).
//
// These two iterators are completely independent and don't know about each
// other, which could cause them to overlap if alternating between calling
// `Units::next()` and `Units::next_back()`.
//
// To prevent this we also store the base length of the unyielded iterating
// range, which is decreased as new `TreeSliece`s are yielded (both forward and
// backward). Once that reaches zero this iterator will stop yielding any more
// items.
#[derive(Clone)]
pub struct Units<'a, const ARITY: usize, L: Leaf, M: Metric<L::Summary>> {
    /// Iterates over the `M`-units from front to back.
    forward: UnitsForward<'a, ARITY, L, M>,

    /// Iterates over the `M`-units from back to front.
    backward: UnitsBackward<'a, ARITY, L, M>,

    /// The base length of all the `TreeSlice`s which are yet to be yielded.
    remaining: L::BaseMetric,
}

impl<'a, const ARITY: usize, L: Leaf, M: Metric<L::Summary>>
    From<&'a Tree<ARITY, L>> for Units<'a, ARITY, L, M>
where
    for<'d> L::Slice<'d>: Default,
{
    #[inline]
    fn from(tree: &'a Tree<ARITY, L>) -> Units<'a, ARITY, L, M> {
        Self {
            forward: UnitsForward::from(tree),
            backward: UnitsBackward::from(tree),
            remaining: tree.base_len(),
        }
    }
}

impl<'a, const ARITY: usize, L: Leaf, M: Metric<L::Summary>>
    From<&TreeSlice<'a, ARITY, L>> for Units<'a, ARITY, L, M>
where
    for<'d> L::Slice<'d>: Default,
{
    #[inline]
    fn from(tree_slice: &TreeSlice<'a, ARITY, L>) -> Units<'a, ARITY, L, M> {
        Self {
            forward: UnitsForward::from(tree_slice),
            backward: UnitsBackward::from(tree_slice),
            remaining: tree_slice.base_len(),
        }
    }
}

impl<'a, const ARITY: usize, L: Leaf, M: UnitMetric<L>> Iterator
    for Units<'a, ARITY, L, M>
{
    /// The iterator returns the next `TreeSlice` in the iterating range
    /// together with its advance.
    ///
    /// # On advances
    ///
    /// The advance describes how much of the iterating range has been yielded
    /// by returning this `TreeSlice`, and it's always bigger than or equal to
    /// the base length of the slice.
    ///
    /// To give an example let's consider the string "foo\nbar\nbaz".
    ///
    /// The `RawLineMetric` would first yield "foo\n", then "bar\n", and
    /// finally "baz". In this case the advance is always equal to the base
    /// length of the slice.
    ///
    /// On the other hand, the `LineMetric` would first yield "foo" without
    /// including the trailing newline, then "bar" and lastly "baz". In the
    /// first and second calls the advance is 1 byte longer than the byte
    /// length of the slice to account for the newlines, which are not part of
    /// the returned slices.
    ///
    /// The name is taken from [typography][1], where the advance of a glyph is
    /// the sum of its width plus some kerning to separate it from the
    /// following glyph.
    ///
    /// [1]: https://freetype.org/freetype2/docs/glyphs/glyph-metrics-3.svg
    type Item = (TreeSlice<'a, ARITY, L>, L::BaseMetric);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == L::BaseMetric::zero() {
            return None;
        }

        let iter = &mut self.forward;

        if !iter.is_initialized {
            iter.initialize();
        }

        let (tree_slice, advance) =
            if iter.start_slice.measure::<M>() > M::zero() {
                iter.next_unit_in_leaf()
            } else if iter.units_yielded < iter.units_total {
                iter.next_unit_in_range()
            } else if iter.base_total > iter.base_yielded {
                let (remainder, advance) = iter.remainder();

                debug_assert_eq!(advance, iter.base_total - iter.base_yielded);

                iter.base_yielded = iter.base_total;

                return Some((remainder, advance));
            } else {
                return None;
            };

        iter.base_yielded += advance;
        iter.units_yielded += M::one();

        Some((tree_slice, advance))
    }
}

impl<const ARITY: usize, L: Leaf, M: DoubleEndedUnitMetric<L>>
    DoubleEndedIterator for Units<'_, ARITY, L, M>
{
    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.remaining == L::BaseMetric::zero() {
            return None;
        }

        let iter = &mut self.backward;

        if !iter.is_initialized {
            iter.initialize();

            if iter.base_remaining > L::BaseMetric::zero() {
                if let Some((remainder, advance)) = iter.remainder() {
                    iter.base_remaining -= advance;
                    return Some((remainder, advance));
                }
            }
        }

        #[rustfmt::skip]
        let (tree_slice, advance) =
            if iter.end_slice.measure::<M>() > M::one() {
                iter.previous_unit_in_leaf()
            } else if iter.units_remaining > M::one() {
                iter.previous_unit_in_range()
            } else if iter.base_remaining > L::BaseMetric::zero() {
                iter.first()
            } else {
                return None;
            };

        iter.base_remaining -= advance;
        iter.units_remaining -= M::one();

        Some((tree_slice, advance))
    }
}

impl<const ARITY: usize, L: Leaf, M: UnitMetric<L>> core::iter::FusedIterator
    for Units<'_, ARITY, L, M>
{
}

struct UnitsForward<'a, const N: usize, L: Leaf, M: Metric<L::Summary>> {
    /// Whether `Self` has been initialized by calling
    /// [`initialize`](UnitsForward::initialize()).
    is_initialized: bool,

    /// The path from the root node down to `leaf_node`. All the nodes in the
    /// path are guaranteed to be internal nodes, and the second item in each
    /// tuple represents the child index of next node in the path, or the index
    /// of the leaf node for the last node.
    path: Vec<(&'a Arc<Node<N, L>>, usize)>,

    /// The current leaf node.
    leaf_node: &'a Arc<Node<N, L>>,

    /// How much of `leaf_node`'s base length has already been yielded.
    yielded_in_leaf: L::BaseMetric,

    /// The `start_slice` field of the next `TreeSlice` that'll be returned by
    /// [`next`](Self::next()).
    start_slice: L::Slice<'a>,

    /// The first slice in the yielding range. It's only set if we're iterating
    /// over a `TreeSlice`.
    first_slice: Option<L::Slice<'a>>,

    /// The last slice in the yielding range. It's only set if
    /// we're iterating over a `TreeSlice`.
    last_slice: Option<L::Slice<'a>>,

    /// The start of the yielding range as an offset into the root.
    base_start: L::BaseMetric,

    /// The base length of all the advances yielded so far.
    base_yielded: L::BaseMetric,

    /// The total base length of the iterating range (doesn't change).
    base_total: L::BaseMetric,

    /// The `M`-units yielded so far.
    units_yielded: M,

    /// The total `M`-length of the iterating range (doesn't change).
    units_total: M,
}

impl<const N: usize, L: Leaf, M: Metric<L::Summary>> Clone
    for UnitsForward<'_, N, L, M>
{
    #[inline]
    fn clone(&self) -> Self {
        Self { path: self.path.clone(), ..*self }
    }
}

impl<'a, const ARITY: usize, L: Leaf, M: Metric<L::Summary>>
    From<&'a Tree<ARITY, L>> for UnitsForward<'a, ARITY, L, M>
where
    for<'d> L::Slice<'d>: Default,
{
    #[inline]
    fn from(tree: &'a Tree<ARITY, L>) -> UnitsForward<'a, ARITY, L, M> {
        Self {
            is_initialized: false,
            path: Vec::with_capacity(tree.root().depth()),
            leaf_node: tree.root(),
            yielded_in_leaf: L::BaseMetric::zero(),
            start_slice: L::Slice::default(),
            first_slice: None,
            last_slice: None,
            base_start: L::BaseMetric::zero(),
            base_yielded: L::BaseMetric::zero(),
            base_total: tree.base_len(),
            units_yielded: M::zero(),
            units_total: tree.measure::<M>(),
        }
    }
}

impl<'a, const ARITY: usize, L: Leaf, M: Metric<L::Summary>>
    From<&TreeSlice<'a, ARITY, L>> for UnitsForward<'a, ARITY, L, M>
where
    for<'d> L::Slice<'d>: Default,
{
    #[inline]
    fn from(
        tree_slice: &TreeSlice<'a, ARITY, L>,
    ) -> UnitsForward<'a, ARITY, L, M> {
        Self {
            is_initialized: false,
            path: Vec::with_capacity(tree_slice.root().depth()),
            leaf_node: tree_slice.root(),
            yielded_in_leaf: L::BaseMetric::zero(),
            start_slice: L::Slice::default(),
            first_slice: Some(tree_slice.start_slice),
            last_slice: Some(tree_slice.end_slice),
            base_start: tree_slice.offset,
            base_yielded: L::BaseMetric::zero(),
            base_total: tree_slice.base_len(),
            units_yielded: M::zero(),
            units_total: tree_slice.measure::<M>(),
        }
    }
}

impl<'a, const N: usize, L: Leaf, M: UnitMetric<L>> UnitsForward<'a, N, L, M> {
    /// Initializes `Self` by populating the path down to the internal node
    /// containing the leaf node at `base_offset`, which is set to `leaf_node`.
    ///
    /// Also sets `yielded_in_leaf`, `start_slice` and `start_summary`.
    #[inline]
    fn initialize(&mut self) {
        debug_assert!(!self.is_initialized);

        self.is_initialized = true;

        // The leaf node is actually the root at this point.
        let mut node = self.leaf_node;

        let mut offset = L::BaseMetric::zero();

        'outer: loop {
            match &**node {
                Node::Internal(inode) => {
                    for (idx, child) in inode.children().iter().enumerate() {
                        let child_len = child.base_len();

                        if offset + child_len > self.base_start {
                            self.path.push((node, idx));
                            node = child;
                            continue 'outer;
                        } else {
                            offset += child_len;
                        }
                    }

                    unreachable!();
                },

                Node::Leaf(leaf) => {
                    self.leaf_node = node;

                    match self.first_slice.take() {
                        Some(slice) => {
                            self.yielded_in_leaf = self.base_start - offset;
                            self.start_slice = slice;
                        },

                        None => {
                            self.start_slice = leaf.as_slice();
                        },
                    }

                    return;
                },
            }
        }
    }

    /// Returns the next leaf in the iterating range after the current
    /// `leaf_node` as a slice, **without** checking if there is one. If we're
    /// iterating over a `TreeSlice` and the next leaf contains its
    /// `last_slice`, that'll be returned instead.
    #[inline]
    fn next_leaf(&mut self) -> L::Slice<'a> {
        debug_assert!(self.base_total > self.base_yielded);

        let mut node = loop {
            let &mut (node, ref mut child_idx) = self.path.last_mut().unwrap();

            // Every node in the path is an internal node.
            let inode = node.get_internal();

            *child_idx += 1;

            if *child_idx < inode.len() {
                break inode.child(*child_idx);
            } else {
                self.path.pop();
            }
        };

        loop {
            match &**node {
                Node::Internal(inode) => {
                    self.path.push((node, 0));
                    node = inode.first();
                    continue;
                },

                Node::Leaf(leaf) => {
                    self.leaf_node = node;

                    let contains_last_slice =
                        leaf.base_len() > self.base_total - self.base_yielded;

                    return if contains_last_slice {
                        self.last_slice.take().unwrap()
                    } else {
                        leaf.as_slice()
                    };
                },
            }
        }
    }

    /// Yields the next unit in the current `self.leaf_node`. This function
    /// should only be called when `self.start_slice` has an `M`-length of at
    /// least `M::one()`.
    #[inline]
    fn next_unit_in_leaf(&mut self) -> (TreeSlice<'a, N, L>, L::BaseMetric) {
        debug_assert_ne!(self.start_slice.measure::<M>(), M::zero());
        debug_assert!(self.units_total > self.units_yielded);

        let (slice, rest, advance) = M::first_unit(self.start_slice);

        let offset = self.yielded_in_leaf;

        self.yielded_in_leaf += advance;
        self.start_slice = rest;

        (
            TreeSlice {
                root: self.leaf_node,
                offset,
                start_slice: slice,
                end_slice: slice,
                summary: slice.summarize(),
            },
            advance,
        )
    }

    /// Traverses the path to reach the next leaf node with a non-zero
    /// `M`-length.
    ///
    /// Returns a `(leaf, root, before, summary)` tuple where:
    ///
    /// - `leaf` is that leaf node;
    ///
    /// - `root` is the deepest internal node containing both the current
    ///   `self.leaf_node` and `leaf` in its subtree;
    ///
    /// - `before` is the total base length of all the nodes from the first
    ///   leaf in `root`'s subtree to the leaf preceding the current
    ///   `self.leaf_node`. If `self.leaf_node` is the first leaf in `root`'s
    ///   subtree this will be zero;
    ///
    /// - `summary` is the total summary of all the nodes between (but not
    ///   including) `self.leaf_node` and `leaf`. If `leaf` is the leaf node
    ///   immediately after `self.leaf`, then it will be empty.
    ///
    /// NOTE: it assumes that such a leaf node exists. If that's not the case
    /// this function may panic or return a leaf node outside of the valid
    /// range for this iterator.
    ///
    /// NOTE: after calling this function the path will contain the path from
    /// the root down to the internal node containing `leaf`, and
    /// `self.leaf_node` will be set to `leaf`.
    #[allow(clippy::type_complexity)]
    #[inline]
    fn next_leaf_with_measure(
        &mut self,
    ) -> (&'a L, &'a Arc<Node<N, L>>, L::BaseMetric, L::Summary) {
        debug_assert!(self.units_total > self.units_yielded);

        let mut before = L::BaseMetric::zero();
        let mut summary = L::Summary::empty();

        // Step 1: pop nodes off the path until we find a node with some
        // `M`-units that we haven't yielded yet.

        'outer: loop {
            let (node, child_idx) = self.path.pop().unwrap();

            // Every node in the path is an internal node.
            let inode = node.get_internal();

            for child in &inode.children()[..child_idx] {
                before += child.base_len();
            }

            for (idx, child) in
                inode.children()[child_idx + 1..].iter().enumerate()
            {
                if child.measure::<M>() > M::zero() {
                    self.path.push((node, child_idx + 1 + idx));
                    break 'outer;
                } else {
                    summary += child.summary();
                }
            }
        }

        let &(inode, child_idx) = self.path.last().unwrap();

        // Step 2: push nodes on the path until we get to the first leaf node
        // with a positive `M`-length. Once we get there we're done.

        // Every node in the path is an internal node.
        let mut node = inode.get_internal().child(child_idx);

        'outer: loop {
            match &**node {
                Node::Internal(inode) => {
                    for (idx, child) in inode.children().iter().enumerate() {
                        if child.measure::<M>() > M::zero() {
                            self.path.push((node, idx));
                            node = child;
                            continue 'outer;
                        } else {
                            summary += child.summary();
                        }
                    }

                    unreachable!();
                },

                Node::Leaf(leaf) => {
                    debug_assert!(leaf.measure::<M>() > M::zero());

                    self.leaf_node = node;
                    return (leaf, inode, before, summary);
                },
            }
        }
    }

    /// Returns the leaf node before the current `leaf_node`, without checking
    /// if there is one in the valid range of this iterator.
    ///
    /// Invariants: the returned [`Node`] is guaranteed to be a leaf node.
    #[inline]
    fn previous_leaf(&self) -> &'a Arc<Node<N, L>> {
        let mut path_idx = self.path.len() - 1;

        let mut node = loop {
            let (node, child_idx) = self.path[path_idx];

            // Every node in the path is an internal node.
            let inode = node.get_internal();

            if child_idx > 0 {
                break inode.child(child_idx - 1);
            } else {
                path_idx -= 1;
            }
        };

        loop {
            match &**node {
                Node::Internal(inode) => {
                    node = inode.last();
                    continue;
                },

                Node::Leaf(_) => break node,
            }
        }
    }

    /// Yields the next unit in the iterating range in the general case, i.e.
    /// when the `TreeSlice` is not totally contained in `self.leaf_node` and
    /// it's not the remainder.
    #[inline]
    fn next_unit_in_range(&mut self) -> (TreeSlice<'a, N, L>, L::BaseMetric) {
        debug_assert_eq!(self.start_slice.measure::<M>(), M::zero());
        debug_assert!(self.units_total > self.units_yielded);

        // A previous call to `next_unit_in_leaf()` might've left the start
        // slice empty. If it is we move to the next leaf before continuing.
        if self.start_slice.is_empty() {
            let leaf_slice = self.next_leaf();
            self.yielded_in_leaf = L::BaseMetric::zero();
            self.start_slice = leaf_slice;

            if self.start_slice.measure::<M>() > M::zero() {
                return self.next_unit_in_leaf();
            }
        }

        let start_slice = self.start_slice;
        let start_summary = self.start_slice.summarize();

        let (leaf, mut root, mut offset, mut summary) =
            self.next_leaf_with_measure();

        let is_immediately_after =
            summary.measure::<L::BaseMetric>().is_zero();

        offset += self.yielded_in_leaf;
        summary += start_summary;

        let slice = {
            let contains_last_slice =
                self.base_yielded + summary.base_len() + leaf.base_len()
                    > self.base_total;

            if contains_last_slice {
                self.last_slice.take().unwrap()
            } else {
                leaf.as_slice()
            }
        };

        let (mut end_slice, rest, mut advance) = M::first_unit(slice);

        self.yielded_in_leaf = advance;
        self.start_slice = rest;

        advance += summary.base_len();

        if !end_slice.is_empty() {
            summary += end_slice.summarize();
        }
        // This edge case can happen when the first unit of `slice` is empty.
        //
        // For example if `slice` is "\nfoo" the LineMetric would return "" as
        // the first unit, with an advance of 1 byte (the newline). Since the
        // `end_slice` of a `TreeSlice` can never be empty we need to go back
        // to the previous leaf.
        else {
            let previous_leaf = self.previous_leaf();

            if is_immediately_after {
                root = previous_leaf;
                offset = root.base_len() - summary.base_len();
                end_slice = start_slice;
            } else {
                let start = offset;

                let (new_root, remove_offset) =
                    tree_slice::deepest_node_containing_base_range(
                        root,
                        start,
                        start + summary.base_len(),
                    );

                root = new_root;

                offset -= remove_offset;

                // `previous_leaf` is guaranteed to be a leaf node by
                // `Self::previous_leaf()`.
                let previous_leaf = previous_leaf.get_leaf();

                end_slice = previous_leaf.as_slice();
            }
        }

        (TreeSlice { root, offset, summary, end_slice, start_slice }, advance)
    }

    /// Very similar to [`next_leaf_with_measure`](1), except it doesn't
    /// mutate any state and instead of returning the next leaf node with a
    /// non-zero `M`-length it returns the leaf node containing
    /// [`last_slice`](2), or the last leaf node in the root if that's not set.
    ///
    /// NOTE: it assumes that that leaf node is different from the current
    /// [`leaf_node`](3). That case should be handled by the caller.
    ///
    /// [1]: UnitsForward::next_leaf_with_measure()
    /// [2]: UnitsForward::last_slice
    /// [3]: UnitsForward::leaf_node
    #[allow(clippy::type_complexity)]
    #[inline]
    fn last_leaf(
        &self,
    ) -> (&'a L, &'a Arc<Node<N, L>>, L::BaseMetric, L::Summary) {
        // Step 1: find the index of deepest node in the path that fully
        // contains `range`.

        let mut range = (self.base_start + self.base_yielded)
            ..(self.base_start + self.base_total);

        let root_idx = {
            let mut root_idx = self.path.len() - 1;

            'outer: for (path_idx, &(node, child_idx)) in
                self.path.iter().enumerate()
            {
                // Every node in the path is an internal node.
                let inode = node.get_internal();

                let mut measured = L::BaseMetric::zero();

                for child in &inode.children()[..child_idx] {
                    measured += child.base_len();
                }

                for child in &inode.children()[child_idx..] {
                    let child_len = child.base_len();

                    if measured <= range.start
                        && measured + child_len >= range.end
                    {
                        range.start -= measured;
                        range.end -= measured;
                        continue 'outer;
                    } else {
                        measured += child_len;
                    }
                }

                // If none of this inode's children fully contains the range
                // then this is the root.
                root_idx = path_idx;

                break;
            }

            root_idx
        };

        // Step 2: traverse down the path starting from the node after the
        // root, increasing `before` and `summary` as you go.

        let mut before = L::BaseMetric::zero();
        let mut summary = L::Summary::empty();

        for &(node, child_idx) in &self.path[root_idx + 1..] {
            // Every node in the path is an internal node.
            let inode = node.get_internal();

            for child in &inode.children()[..child_idx] {
                before += child.base_len();
            }

            for child in &inode.children()[child_idx + 1..] {
                summary += child.summary();
            }
        }

        let (root, child_idx) = self.path[root_idx];

        // Every node in the path is an internal node.
        let inode = root.get_internal();

        let mut offset = L::BaseMetric::zero();

        for child in &inode.children()[..child_idx] {
            offset += child.base_len();
            before += child.base_len();
        }

        offset += inode.child(child_idx).base_len();

        // This will be the child of the root node that contains the last
        // slice.
        let mut node = inode.first();

        for child in &inode.children()[child_idx + 1..] {
            let child_len = child.base_len();

            if offset + child_len >= range.end {
                node = child;
                break;
            } else {
                offset += child_len;
                summary += child.summary();
            }
        }

        'outer: loop {
            match &**node {
                Node::Internal(inode) => {
                    for child in inode.children() {
                        let child_len = child.base_len();

                        if offset + child_len >= range.end {
                            node = child;
                            continue 'outer;
                        } else {
                            offset += child_len;
                            summary += child.summary();
                        }
                    }

                    unreachable!();
                },

                Node::Leaf(leaf) => {
                    return (leaf, root, before, summary);
                },
            }
        }
    }

    /// This is the analogous of [`UnitsBackward::remainder()`] when iterating
    /// forward. Check the doc comment of that function as most of it applies
    /// 1:1 to this.
    ///
    /// The most notable difference is that this function doesn't need to wrap
    /// the returned `TreeSlice` in an `Option`. This is because when we
    /// iterate forward this only gets called when we are sure there's a
    /// remainder to yield.
    #[inline]
    fn remainder(&mut self) -> (TreeSlice<'a, N, L>, L::BaseMetric) {
        debug_assert_eq!(self.units_total, self.units_yielded);
        debug_assert!(self.base_total > self.base_yielded);

        if self.start_slice.is_empty() {
            let next_slice = self.next_leaf();
            self.yielded_in_leaf = L::BaseMetric::zero();
            self.start_slice = next_slice;
        }

        // First, check if the leaf node is the root. If it is we're done.
        if self.base_total - self.base_yielded == self.start_slice.base_len() {
            return (
                TreeSlice {
                    root: self.leaf_node,
                    offset: self.yielded_in_leaf,
                    start_slice: self.start_slice,
                    end_slice: self.start_slice,
                    summary: self.start_slice.summarize(),
                },
                self.start_slice.base_len(),
            );
        }

        let start_slice = self.start_slice;

        let (last_leaf, root, mut before, mut summary) = self.last_leaf();

        summary += start_slice.summarize();

        let end_slice = match self.last_slice.take() {
            Some(slice) => slice,
            None => last_leaf.as_slice(),
        };

        summary += end_slice.summarize();

        before += self.yielded_in_leaf;

        let advance = summary.base_len();

        (
            TreeSlice {
                root,
                offset: before,
                summary,
                start_slice,
                end_slice,
            },
            advance,
        )
    }
}

struct UnitsBackward<'a, const N: usize, L: Leaf, M: Metric<L::Summary>> {
    /// Whether `Self` has been initialized by calling
    /// [`initialize`](UnitsBackward::initialize()).
    is_initialized: bool,

    /// The path from the root node down to `leaf_node`. All the nodes in the
    /// path are guaranteed to be internal nodes, and the second item in each
    /// tuple represents the child index of next node in the path, or the index
    /// of the leaf node for the last node.
    path: Vec<(&'a Arc<Node<N, L>>, usize)>,

    /// The current leaf node.
    leaf_node: &'a Arc<Node<N, L>>,

    /// How much of `leaf_node`'s base length has already been yielded.
    yielded_in_leaf: L::BaseMetric,

    /// The `end_slice` of the next `TreeSlice` that'll be returned by
    /// [`previous`](Self::previous()).
    end_slice: L::Slice<'a>,

    /// The first slice in the yielding range. It's only set if we're iterating
    /// over a `TreeSlice`.
    first_slice: Option<L::Slice<'a>>,

    /// The last slice in the yielding range and its summary. It's only set if
    /// we're iterating over a `TreeSlice`.
    last_slice: Option<L::Slice<'a>>,

    /// The start of the yielding range as an offset into the root.
    base_start: L::BaseMetric,

    /// The base length of all the advances which are yet to be yielded.
    base_remaining: L::BaseMetric,

    /// The `M`-units which are yet to be yielded.
    units_remaining: M,
}

impl<const N: usize, L: Leaf, M: Metric<L::Summary>> Clone
    for UnitsBackward<'_, N, L, M>
{
    #[inline]
    fn clone(&self) -> Self {
        Self { path: self.path.clone(), ..*self }
    }
}

impl<'a, const ARITY: usize, L: Leaf, M: Metric<L::Summary>>
    From<&'a Tree<ARITY, L>> for UnitsBackward<'a, ARITY, L, M>
where
    for<'d> L::Slice<'d>: Default,
{
    #[inline]
    fn from(tree: &'a Tree<ARITY, L>) -> UnitsBackward<'a, ARITY, L, M> {
        Self {
            is_initialized: false,
            path: Vec::with_capacity(tree.root().depth()),
            leaf_node: tree.root(),
            yielded_in_leaf: L::BaseMetric::zero(),
            end_slice: L::Slice::default(),
            first_slice: None,
            last_slice: None,
            base_start: L::BaseMetric::zero(),
            base_remaining: tree.base_len(),
            units_remaining: tree.root().measure::<M>(),
        }
    }
}

impl<'a, const ARITY: usize, L: Leaf, M: Metric<L::Summary>>
    From<&TreeSlice<'a, ARITY, L>> for UnitsBackward<'a, ARITY, L, M>
where
    for<'d> L::Slice<'d>: Default,
{
    #[inline]
    fn from(
        tree_slice: &TreeSlice<'a, ARITY, L>,
    ) -> UnitsBackward<'a, ARITY, L, M> {
        Self {
            is_initialized: false,
            path: Vec::with_capacity(tree_slice.root().depth()),
            leaf_node: tree_slice.root(),
            yielded_in_leaf: L::BaseMetric::zero(),
            end_slice: L::Slice::default(),
            first_slice: Some(tree_slice.start_slice),
            last_slice: Some(tree_slice.end_slice),
            base_start: tree_slice.offset,
            base_remaining: tree_slice.base_len(),
            units_remaining: tree_slice.measure::<M>(),
        }
    }
}

impl<'a, const N: usize, L: Leaf, M: DoubleEndedUnitMetric<L>>
    UnitsBackward<'a, N, L, M>
{
    /// Initializes `Self` by populating the path down to the internal node
    /// containing the leaf node at `base_start + base_remaining`, which is set
    /// to `leaf_node`.
    ///
    /// Also sets `yielded_in_leaf`, `end_slice` and `end_summary`.
    #[inline]
    fn initialize(&mut self) {
        debug_assert!(!self.is_initialized);

        self.is_initialized = true;

        // The leaf node is actually the root at this point.
        let mut node = self.leaf_node;

        let last_slice_offset = self.base_start + self.base_remaining;

        let mut offset = L::BaseMetric::zero();

        'outer: loop {
            match &**node {
                Node::Internal(inode) => {
                    for (idx, child) in inode.children().iter().enumerate() {
                        let child_len = child.base_len();

                        if offset + child_len >= last_slice_offset {
                            self.path.push((node, idx));
                            node = child;
                            continue 'outer;
                        } else {
                            offset += child_len;
                        }
                    }

                    unreachable!();
                },

                Node::Leaf(leaf) => {
                    self.leaf_node = node;

                    match self.last_slice.take() {
                        Some(slice) => {
                            let range_end =
                                self.base_start + self.base_remaining;
                            self.yielded_in_leaf =
                                offset + leaf.base_len() - range_end;
                            self.end_slice = slice;
                        },

                        None => {
                            self.end_slice = leaf.as_slice();
                        },
                    };

                    return;
                },
            }
        }
    }

    /// Returns the leaf node before the current `leaf_node`, **without**
    /// checking if there is one in the valid range of this iterator.
    #[inline]
    fn previous_leaf(&mut self) -> &'a L {
        let mut node = loop {
            let &mut (node, ref mut child_idx) = self.path.last_mut().unwrap();

            // Every node in the path is an internal node.
            let inode = node.get_internal();

            if *child_idx > 0 {
                *child_idx -= 1;
                break inode.child(*child_idx);
            } else {
                self.path.pop();
            }
        };

        loop {
            match &**node {
                Node::Internal(inode) => {
                    self.path.push((node, inode.len() - 1));
                    node = inode.last();
                    continue;
                },

                Node::Leaf(leaf) => {
                    self.leaf_node = node;
                    return leaf;
                },
            }
        }
    }

    /// Very similar to [`previous_leaf_with_measure`](1), except it doesn't
    /// mutate any state and instead of returning the previous leaf node with a
    /// non-zero `M`-length it returns the leaf node containing
    /// [`first_slice`](2), or the first leaf node in the root if that's not
    /// set.
    ///
    /// NOTE: it assumes that that leaf node is different from the current
    /// [`leaf_node`](3). That case should be handled by the caller.
    ///
    /// [1]: UnitsBackward::previous_leaf_with_measure()
    /// [2]: UnitsBackward::first_slice
    /// [3]: UnitsBackward::leaf_node
    #[allow(clippy::type_complexity)]
    #[inline]
    fn first_leaf(
        &self,
    ) -> (&'a L, &'a Arc<Node<N, L>>, L::BaseMetric, L::Summary) {
        // Step 1: find the index of deepest node in the path that fully
        // contains `range`.

        let mut range =
            self.base_start..(self.base_start + self.base_remaining);

        let root_idx = {
            let mut root_idx = self.path.len() - 1;

            'outer: for (path_idx, &(node, child_idx)) in
                self.path.iter().enumerate()
            {
                // Every node in the path is an internal node.
                let inode = node.get_internal();

                let mut offset = L::BaseMetric::zero();

                for child in &inode.children()[..=child_idx] {
                    let child_len = child.base_len();

                    if offset <= range.start && offset + child_len >= range.end
                    {
                        range.start -= offset;
                        range.end -= offset;
                        continue 'outer;
                    } else {
                        offset += child_len;
                    }
                }

                // If none of this inode's children fully contains the range
                // then this is the root.
                root_idx = path_idx;

                break;
            }

            root_idx
        };

        // Step 2: traverse down the path starting from the node after the
        // root, increasing `after` and `summary` as you go.

        let mut after = L::BaseMetric::zero();
        let mut summary = L::Summary::empty();

        for &(node, child_idx) in &self.path[root_idx + 1..] {
            // Every node in the path is an internal node.
            let inode = node.get_internal();

            for child in &inode.children()[..child_idx] {
                summary += child.summary();
            }

            for child in &inode.children()[child_idx + 1..] {
                after += child.base_len();
            }
        }

        let (root, child_idx) = self.path[root_idx];

        // Every node in the path is an internal node.
        let inode = root.get_internal();

        for child in &inode.children()[child_idx + 1..] {
            after += child.base_len();
        }

        // This will be the child of the root node that contains the first
        // slice.
        let mut node = inode.first();

        let mut offset = L::BaseMetric::zero();

        let mut children = inode.children()[..child_idx].iter();

        while let Some(child) = children.next() {
            let child_len = child.base_len();

            if offset + child_len > range.start {
                for child in children {
                    summary += child.summary();
                }
                node = child;
                break;
            } else {
                offset += child_len;
            }
        }

        'outer: loop {
            match &**node {
                Node::Internal(inode) => {
                    let mut children = inode.children().iter();

                    while let Some(child) = children.next() {
                        let child_len = child.base_len();

                        if offset + child_len > range.start {
                            for child in children {
                                summary += child.summary();
                            }
                            node = child;
                            continue 'outer;
                        } else {
                            offset += child_len;
                        }
                    }

                    unreachable!();
                },

                Node::Leaf(leaf) => {
                    return (leaf, root, after, summary);
                },
            }
        }
    }

    /// Yields the first unit in the range. This function is used by
    ///
    /// - [`Self::remainder()`] if there are no units in the iterating range,
    ///   in which case it'll yield the whole range;
    ///
    /// - by [`Self::previous()`] when there's one final unit to yield.
    #[inline]
    fn first(&mut self) -> (TreeSlice<'a, N, L>, L::BaseMetric) {
        debug_assert!(self.base_remaining > L::BaseMetric::zero());

        let (_, end_slice, mut advance) = M::last_unit(self.end_slice);

        // First, check if the current leaf node is the root. If it is we're
        // done.
        if self.base_remaining == advance {
            let offset = self.leaf_node.base_len()
                - self.yielded_in_leaf
                - end_slice.base_len();

            return (
                TreeSlice {
                    root: self.leaf_node,
                    offset,
                    summary: end_slice.summarize(),
                    start_slice: end_slice,
                    end_slice,
                },
                advance,
            );
        }

        // This edge case can happen when the last unit of `self.end_slice` is
        // empty.
        //
        // For example if `self.end_slice` is "\n" the LineMetric would return
        // "" as the last unit, with an advance of 1 byte (the newline). Since
        // the `end_slice` of a `TreeSlice` can never be empty we need to go
        // back to the previous leaf.
        if end_slice.is_empty() {
            let previous_leaf = self.previous_leaf();

            self.base_remaining -= advance;

            let contains_first_slice =
                previous_leaf.base_len() > self.base_remaining;

            if contains_first_slice {
                self.end_slice = self.first_slice.take().unwrap();
            } else {
                self.end_slice = previous_leaf.as_slice();
            };

            self.yielded_in_leaf = L::BaseMetric::zero();

            let (first, first_advance) = self.first();

            self.base_remaining += advance;

            advance += first_advance;

            return (first, advance);
        };

        let (first_leaf, root, after, mut summary) = self.first_leaf();

        advance += summary.base_len();

        summary += end_slice.summarize();

        let start_slice = match self.first_slice.take() {
            Some(slice) => slice,
            None => first_leaf.as_slice(),
        };

        let start_summary = start_slice.summarize();

        advance += start_summary.base_len();

        summary += start_summary;

        let offset = root.base_len() - after - self.yielded_in_leaf - advance;

        (TreeSlice { root, offset, start_slice, end_slice, summary }, advance)
    }

    /// Yields the previous unit in the current `self.leaf_node`. To do this
    /// correctly `self.end_slice` cannot have any `M`-remainder and it needs
    /// to contain at least 2 `M`-units.
    #[inline]
    fn previous_unit_in_leaf(
        &mut self,
    ) -> (TreeSlice<'a, N, L>, L::BaseMetric) {
        debug_assert!(self.end_slice.measure::<M>() > M::one());
        debug_assert!(self.units_remaining > M::zero());

        let (rest, slice, advance) = M::last_unit(self.end_slice);

        debug_assert!(!rest.is_empty());

        self.yielded_in_leaf += advance;
        self.end_slice = rest;

        (
            TreeSlice {
                root: self.leaf_node,
                offset: rest.base_len(),
                summary: slice.summarize(),
                end_slice: slice,
                start_slice: slice,
            },
            advance,
        )
    }

    /// Traverses the path to reach the previous leaf node with a non-zero
    /// `M`-length.
    ///
    /// Returns a `(leaf, root, after, summary)` tuple where:
    ///
    /// - `leaf` is that leaf node;
    ///
    /// - `root` is the deepest internal node containing both `leaf` and the
    ///   current `self.leaf_node` in its subtree;
    ///
    /// - `after` is the total base length of all the nodes from the last leaf
    ///   in `root`'s subtree to the leaf after the current `self.leaf_node`.
    ///   If `self.leaf_node` if the last leaf in `root`'s subtree this will be
    ///   zero;
    ///
    /// - `summary` is the total summary of all the nodes between (but not
    ///   including) `leaf` and `self.leaf_node`. If `leaf` is the leaf node
    ///   immediately before `self.leaf`, then it will be empty.
    ///
    /// NOTE: it assumes that such a leaf node exists. If that's not the case
    /// this function may panic or return a leaf node outside of the valid
    /// range for this iterator.
    ///
    /// NOTE: after calling this function the path will contain the path from
    /// the root down to the internal node containing `leaf`, and
    /// `self.leaf_node` will be set to `leaf`.
    #[allow(clippy::type_complexity)]
    #[inline]
    fn previous_leaf_with_measure(
        &mut self,
    ) -> (&'a L, &'a Arc<Node<N, L>>, L::BaseMetric, L::Summary) {
        debug_assert!(self.units_remaining > M::zero());

        let mut after = L::BaseMetric::zero();
        let mut summary = L::Summary::empty();

        // Step 1: pop nodes off the path until we find a node with some
        // `M`-units that we haven't yielded yet.

        'outer: loop {
            let (node, child_idx) = self.path.pop().unwrap();

            // Every node in the path is an internal node.
            let inode = node.get_internal();

            for child in &inode.children()[child_idx + 1..] {
                after += child.base_len();
            }

            for (idx, child) in
                inode.children()[..child_idx].iter().enumerate().rev()
            {
                if child.measure::<M>() > M::zero() {
                    self.path.push((node, idx));
                    break 'outer;
                } else {
                    summary += child.summary();
                }
            }
        }

        let &(inode, child_idx) = self.path.last().unwrap();

        // Step 2: push nodes on the path until we get to the first leaf node
        // with a positive M-length. Once we get there we're done.

        // Every node in the path is an internal node.
        let mut node = &inode.get_internal().children()[child_idx];

        'outer: loop {
            match &**node {
                Node::Internal(inode) => {
                    for (idx, child) in
                        inode.children().iter().enumerate().rev()
                    {
                        if child.measure::<M>() > M::zero() {
                            self.path.push((node, idx));
                            node = child;
                            continue 'outer;
                        } else {
                            summary += child.summary();
                        }
                    }

                    unreachable!();
                },

                Node::Leaf(leaf) => {
                    debug_assert!(leaf.measure::<M>() > M::zero());

                    self.leaf_node = node;
                    return (leaf, inode, after, summary);
                },
            }
        }
    }

    /// Returns the leaf node after the current `leaf_node`, **without**
    /// checking if there is one in the valid range of this iterator.
    ///
    /// Invariants: the returned node is guaranteed to be a leaf node.
    #[inline]
    fn next_leaf(&self) -> &'a Arc<Node<N, L>> {
        let mut path_idx = self.path.len() - 1;

        let mut node = loop {
            let (node, mut child_idx) = self.path[path_idx];

            // Every node in the path is an internal node.
            let inode = node.get_internal();

            child_idx += 1;

            if child_idx < inode.len() {
                break inode.child(child_idx);
            } else {
                path_idx -= 1;
            }
        };

        loop {
            match &**node {
                Node::Internal(inode) => {
                    node = inode.first();
                    continue;
                },

                Node::Leaf(_) => break node,
            }
        }
    }

    /// Yields the previous unit in the iterating range. This is the function
    /// that gets called in the general case, i.e. when the next unit is not
    /// the first, the last and it's not contained in `self.leaf_node`. The
    /// root of the returned `TreeSlice` is a node in the path so it's
    /// guaranteed to be an internal node.
    ///
    /// NOTE: this uses [`previous_leaf_with_measure`][1] internally so it
    /// should only be called when `self.units_yielded < self.units_total`.
    ///
    /// [1]: UnitsBackward::previous_leaf_with_measure()
    #[inline]
    fn previous_unit_in_range(
        &mut self,
    ) -> (TreeSlice<'a, N, L>, L::BaseMetric) {
        debug_assert!(self.units_remaining > M::zero());

        let (_, end_slice, mut advance) = M::last_unit(self.end_slice);

        // This edge case can happen when the last unit of `self.end_slice` is
        // empty.
        //
        // For example if `self.end_slice` is "\n" the LineMetric would return
        // "" as the last unit, with an advance of 1 byte (the newline). Since
        // the `end_slice` of a `TreeSlice` can never be empty we need to go
        // back to the previous leaf.
        if end_slice.is_empty() {
            let previous_leaf = self.previous_leaf();

            self.base_remaining -= advance;

            let contains_first_slice =
                previous_leaf.base_len() > self.base_remaining;

            if contains_first_slice {
                self.end_slice = self.first_slice.take().unwrap();
            } else {
                self.end_slice = previous_leaf.as_slice();
            };

            self.yielded_in_leaf = L::BaseMetric::zero();

            let (slice, slice_advance) = match self.remainder() {
                Some(remainder) => remainder,
                _ => {
                    let (_, empty) = M::remainder(self.end_slice);

                    debug_assert!(empty.is_empty(),);

                    (
                        TreeSlice {
                            root: self.leaf_node,
                            offset: self.leaf_node.base_len(),
                            start_slice: empty,
                            end_slice: empty,
                            summary: L::Summary::empty(),
                        },
                        L::BaseMetric::zero(),
                    )
                },
            };

            self.base_remaining += advance;

            advance += slice_advance;

            return (slice, advance);
        }

        let (leaf, mut root, after, mut summary) =
            self.previous_leaf_with_measure();

        let is_immediately_before = summary.base_len().is_zero();

        advance += summary.measure::<L::BaseMetric>();

        summary += end_slice.summarize();

        let slice = {
            let contains_first_slice =
                advance + leaf.base_len() > self.base_remaining;

            if contains_first_slice {
                self.first_slice.take().unwrap()
            } else {
                leaf.as_slice()
            }
        };

        let (rest, mut start_slice) = M::remainder(slice);

        let start_summary = start_slice.summarize();

        advance += start_summary.base_len();

        let mut offset =
            root.base_len() - after - self.yielded_in_leaf - advance;

        self.yielded_in_leaf = start_summary.base_len();
        self.end_slice = rest;

        if !start_slice.is_empty() {
            summary += start_summary;
        }
        // This edge case can happen when the remainder of `slice` is empty.
        //
        // For example if `slice` is "foo\n" the LineMetric would return "" as
        // the remainder. Since the `start_slice` of a `TreeSlice` can never be
        // empty we need to go back to the next leaf.
        else {
            let next_leaf = self.next_leaf();

            if is_immediately_before {
                root = next_leaf;
                offset = L::BaseMetric::zero();
                start_slice = end_slice;
            } else {
                let start = offset;

                let (new_root, remove_offset) =
                    tree_slice::deepest_node_containing_base_range(
                        root,
                        start,
                        start + summary.base_len(),
                    );

                root = new_root;

                offset -= remove_offset;

                // `next_leaf` is guaranteed to be a leaf node by
                // `Self::next_leaf()`.
                let next_leaf = next_leaf.get_leaf();

                start_slice = next_leaf.as_slice();
            }
        }

        (TreeSlice { root, offset, summary, end_slice, start_slice }, advance)
    }

    /// Yields the remainder of the yielding range when dividing by `M`-units,
    /// i.e. the TreeSlice in the `units_total..base_total` range.
    ///
    /// To give a concrete example let's consider the string "a\nb\nc". Its
    /// `LineMetric` is 2, but the 2nd unit of that metric ends at "b\n", even
    /// though the last line (i.e. the first line this iterator yields) should
    /// be "c".
    ///
    /// This is because there's some text in the `LineMetric(2)..ByteMetric(5)`
    /// range. Calling this function will yield the `TreeSlice` in that range,
    /// but only if its base length is positive (hence the `Option`).
    ///
    /// For example if the string was "a\n\b\n" the range would be
    /// `LineMetric(2)..ByteMetric(4)`, which doesn't contain any text. In that
    /// case this function would return `None`.
    ///
    /// It also follows that if `M` is the `BaseMetric` this function will
    /// always return `None`.
    #[inline]
    fn remainder(&mut self) -> Option<(TreeSlice<'a, N, L>, L::BaseMetric)> {
        debug_assert!(self.base_remaining > L::BaseMetric::zero());

        if self.end_slice.measure::<M>() > M::zero() {
            let (rest, slice) = M::remainder(self.end_slice);

            let summary = slice.summarize();

            if !slice.is_empty() {
                self.yielded_in_leaf += summary.base_len();
                self.end_slice = rest;

                Some((
                    TreeSlice {
                        root: self.leaf_node,
                        offset: rest.base_len(),
                        start_slice: slice,
                        end_slice: slice,
                        summary,
                    },
                    slice.base_len(),
                ))
            } else {
                None
            }
        } else if self.units_remaining > M::zero() {
            Some(self.previous_unit_in_range())
        } else {
            Some(self.first())
        }
    }
}
