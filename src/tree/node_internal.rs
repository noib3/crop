use core::ops::RangeBounds;

use super::traits::*;
use super::{Arc, ExactChain, Node};
use crate::range_bounds_to_start_end;

#[derive(Clone)]
pub(super) struct Inode<const N: usize, L: Leaf> {
    children: Children<N, L>,
    summary: L::Summary,
    depth: usize,
    leaf_count: usize,
}

impl<const N: usize, L: Leaf> core::fmt::Debug for Inode<N, L> {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        if !f.alternate() {
            f.debug_struct("Inode")
                .field("children", &self.children)
                .field("summary", &self.summary)
                .field("depth", &self.depth)
                .field("leaf_count", &self.leaf_count)
                .finish()
        } else {
            pretty_print_inode(self, &mut String::new(), "", 0, f)
        }
    }
}

impl<const N: usize, L: Leaf> Inode<N, L> {
    /// Appends the node at the right depth.
    ///
    /// If all the nodes on the right side of the subtree up to the one to
    /// which `node` should be appended are already full this will return a new
    /// inode at the same depth as `self` to be inserted right after `self`.
    #[inline]
    pub(super) fn append_at_depth(
        &mut self,
        mut node: Arc<Node<N, L>>,
    ) -> Option<Self>
    where
        L: BalancedLeaf + Clone,
    {
        debug_assert!(node.depth() < self.depth());

        if self.depth() > node.depth() + 1 {
            debug_assert!(self.depth() >= 2);

            let extra = self.with_child_mut(self.len() - 1, |last| {
                let last = Arc::make_mut(last);
                // SAFETY: this inode's depth is >= 2 so its children are
                // also inodes.
                let last = unsafe { last.as_mut_internal_unchecked() };
                last.append_at_depth(node)
            })?;

            node = Arc::new(Node::Internal(extra));
        }

        debug_assert_eq!(self.depth(), node.depth() + 1);

        if node.is_underfilled() {
            self.with_child_mut(self.len() - 1, |last| {
                Arc::make_mut(last).balance(Arc::make_mut(&mut node));
            });

            if node.is_empty() {
                return None;
            }
        }

        if !self.is_full() {
            self.push(node);
            None
        } else {
            let mut other =
                Self::from_children(self.drain(Self::min_children() + 1..));

            other.push(node);

            debug_assert!(!self.is_underfilled());
            debug_assert!(!other.is_underfilled());

            Some(other)
        }
    }

    pub(super) fn assert_invariants(&self) {
        assert!(
            self.len() >= Self::min_children(),
            "An internal node of depth {} was supposed to contain at least \
             {} children but actually contains {}",
            self.depth(),
            Self::min_children(),
            self.len()
        );

        assert!(
            self.len() <= Self::max_children(),
            "An internal node of depth {} was supposed to contain at most {} \
             children but actually contains {}",
            self.depth(),
            Self::max_children(),
            self.len()
        );

        let actual_leaves =
            self.children().iter().map(|c| c.leaf_count()).sum::<usize>();

        assert_eq!(
            self.leaf_count,
            actual_leaves,
            "An internal node of depth {} thought it contained {} leaves in \
             its subtree, but actually contains {}",
            self.depth(),
            self.leaf_count,
            actual_leaves
        );

        for child in self.children() {
            assert_eq!(
                self.depth(),
                child.depth() + 1,
                "An internal node at depth {} contains a node of depth {}",
                self.depth(),
                child.depth()
            );
        }
    }

    /// Balances itself with another inode at the same depth. Note that `other`
    /// can be left empty if the children of the two inodes can fit in a single
    /// inode.
    ///
    /// # Panics
    ///
    /// Panics if `other` is at a different depth.
    #[inline]
    pub(super) fn balance(&mut self, other: &mut Self) {
        debug_assert_eq!(self.depth(), other.depth());

        if !self.is_underfilled() && !other.is_underfilled() {
            return;
        }

        if self.len() + other.len() <= Self::max_children() {
            for child in other.drain(..) {
                self.push(child)
            }

            return;
        } else if self.len() > other.len() {
            let move_right = Self::min_children() - other.len();

            for (insert_idx, child) in
                self.drain(self.len() - move_right..).enumerate()
            {
                other.insert(insert_idx, child);
            }
        } else {
            let move_left = other.len() - Self::min_children();

            for child in other.drain(..move_left) {
                self.push(child)
            }
        }

        debug_assert!(
            self.len() >= Self::min_children()
                && self.len() <= Self::max_children()
        );

        debug_assert!(
            other.len() >= Self::min_children()
                && other.len() <= Self::max_children()
        );
    }

    /// Balances the child at `child_idx` with the previous child, unless
    /// `child_idx` is 0 in which case it'll be balanced with the second child.
    ///
    /// Note that if the balancing causes one of the children to be empty that
    /// child will be removed.
    ///
    /// # Panics
    ///
    /// Panics if this inode contains a single child.
    #[inline]
    pub(super) fn balance_child(&mut self, child_idx: usize)
    where
        L: BalancedLeaf + Clone,
    {
        debug_assert!(self.len() > 1);

        if !self.child(child_idx).is_underfilled() {
            return;
        }

        let left_idx = child_idx.saturating_sub(1);
        let right_idx = left_idx + 1;

        let (left, right) = self.two_mut(left_idx, right_idx);

        Arc::make_mut(left).balance(Arc::make_mut(right));

        if right.is_empty() {
            self.remove(right_idx);
        }
    }

    // ```
    // ChunkSummary { b
    // ├── ChunkSummary
    // │   └── ""
    // ├── ChunkSummary
    // │   ├── "s \nn"
    // │   ├── "ec t"
    // │   ├── "urpi"
    // │   └── "s fe"
    // ├── ChunkSummary
    // │   ├── "ugia"
    // │   ├── "t se"
    // │   ├── "mper"
    // │   └── ". Na"
    // └── ChunkSummary
    //     ├── "m at"
    //     ├── " nu"
    //     └── "t a"
    // ```

    /// Recursively balances the first child all the way down to the deepest
    /// inode.
    ///
    /// # Panics
    ///
    /// Panics if the `Arc` enclosing the first child has a strong counter > 1.
    #[inline]
    pub(super) fn balance_left_side(&mut self)
    where
        L: BalancedLeaf + Clone,
    {
        self.balance_first_child_with_second();

        let first_is_underfilled = self.with_child_mut(0, |first| {
            if let Node::Internal(first) = Arc::get_mut(first).unwrap() {
                first.balance_left_side();
                first.is_underfilled()
            } else {
                false
            }
        });

        if first_is_underfilled && self.len() > 1 {
            self.balance_first_child_with_second();
        }
    }

    /// Recursively balances the last child all the way down to the deepest
    /// inode.
    ///
    /// # Panics
    ///
    /// Panics if the `Arc` enclosing the last child has a strong counter > 1.
    #[inline]
    pub(super) fn balance_right_side(&mut self)
    where
        L: BalancedLeaf + Clone,
    {
        self.balance_last_child_with_penultimate();

        let last_is_underfilled =
            self.with_child_mut(self.len() - 1, |last| {
                if let Node::Internal(last) = Arc::get_mut(last).unwrap() {
                    last.balance_right_side();
                    last.is_underfilled()
                } else {
                    false
                }
            });

        if last_is_underfilled && self.len() > 1 {
            self.balance_last_child_with_penultimate();
        }
    }

    /// Balances the first child using the contents of the second child,
    /// possibly merging them together if necessary.
    ///
    /// Note that when the first and second children are leaves this inode's
    /// [`leaf_count()`] may decrease by 1.
    ///
    /// # Panics
    ///
    /// Panics if:
    ///
    /// - this inode has only one child (the second child is assumed to exist);
    ///
    /// - the `Arc` enclosing the first child has a strong counter > 1. This
    /// function assumes that there are zero `Arc::clone`s of the first child.
    #[inline]
    pub(super) fn balance_first_child_with_second(&mut self)
    where
        L: BalancedLeaf + Clone,
    {
        debug_assert!(self.len() >= 2);

        // Check for early returns.
        if !self.first().is_underfilled() {
            return;
        }

        let (first, second) = self.two_mut(0, 1);

        match (Arc::get_mut(first).unwrap(), Arc::make_mut(second)) {
            (Node::Internal(first), Node::Internal(second)) => {
                // Move the second child's children to the first child, then
                // remove the second child.
                if first.len() + second.len() <= Self::max_children() {
                    first.children.append(&mut second.children);
                    first.leaf_count += second.leaf_count;
                    first.summary += second.summary();
                    self.children.remove(1);
                }
                // Move the minimum number of children from the second child
                // to the first child, keeping both.
                else {
                    let missing = Self::min_children() - first.len();

                    for _ in 0..missing {
                        first.push(second.remove(0));
                    }

                    debug_assert!(!first.is_underfilled());
                    debug_assert!(!second.is_underfilled());
                }
            },

            (Node::Leaf(first), Node::Leaf(second)) => {
                first.balance(second);

                if second.is_empty() {
                    self.leaf_count -= 1;
                    self.children.remove(1);
                }
            },

            _ => {
                // SAFETY: the first and second children are siblings so they
                // must be of the same kind.
                unsafe { core::hint::unreachable_unchecked() }
            },
        }
    }

    /// Balances the last child using the contents of the penultimate (i.e.
    /// second to last) child, possibly merging them together if necessary.
    ///
    /// Note that when the penultimate and last children are leaves this
    /// inode's [`leaf_count()`] may decrease by 1.
    ///
    /// # Panics
    ///
    /// Panics if:
    ///
    /// - this inode has only one child (the penultimate child is assumed to
    /// exist);
    ///
    /// - the `Arc` enclosing the last child has a strong counter > 1. This
    /// function assumes that there are zero `Arc::clone`s of the last child.
    #[inline]
    pub(super) fn balance_last_child_with_penultimate(&mut self)
    where
        L: BalancedLeaf + Clone,
    {
        debug_assert!(self.len() >= 2);

        // Check for early returns.
        if !self.last().is_underfilled() {
            return;
        }

        let last_idx = self.len() - 1;

        let (penultimate, last) = self.two_mut(last_idx - 1, last_idx);

        match (Arc::make_mut(penultimate), Arc::get_mut(last).unwrap()) {
            (Node::Internal(penultimate), Node::Internal(last)) => {
                // Move the last child's children to the penultimate child,
                // then remove the last child.
                if penultimate.len() + last.len() <= Self::max_children() {
                    penultimate.children.append(&mut last.children);
                    penultimate.leaf_count += last.leaf_count;
                    penultimate.summary += last.summary();
                    self.children.remove(last_idx);
                }
                // Move the minimum number of children from the penultimate
                // child to the last child, keeping both.
                else {
                    let missing = Self::min_children() - last.len();

                    for _ in 0..missing {
                        let last_idx = penultimate.len() - 1;
                        last.insert(0, penultimate.remove(last_idx));
                    }

                    debug_assert!(!penultimate.is_underfilled());
                    debug_assert!(!last.is_underfilled());
                }
            },

            (Node::Leaf(penultimate), Node::Leaf(last)) => {
                penultimate.balance(last);

                if last.is_empty() {
                    self.leaf_count -= 1;
                    self.children.remove(last_idx);
                }
            },

            _ => {
                // SAFETY: the penultimate and last children are siblings so
                // they must be of the same kind.
                unsafe { core::hint::unreachable_unchecked() }
            },
        }
    }

    #[inline]
    pub fn base_measure(&self) -> L::BaseMetric {
        self.measure::<L::BaseMetric>()
    }

    #[inline]
    pub(super) fn child(&self, child_idx: usize) -> &Arc<Node<N, L>> {
        &self.children()[child_idx]
    }

    #[inline]
    pub(super) fn children(&self) -> &[Arc<Node<N, L>>] {
        self.children.as_slice()
    }

    #[inline]
    fn children_mut(&mut self) -> &mut [Arc<Node<N, L>>] {
        self.children.as_slice_mut()
    }

    #[inline]
    pub(super) fn depth(&self) -> usize {
        self.depth
    }

    /// Removes all the nodes after `child_offset`, returning them and leaving
    /// the inode with `child_offset` children.
    #[inline]
    pub(super) fn drain<R>(&mut self, idx_range: R) -> Drain<'_, N, L>
    where
        R: RangeBounds<usize>,
    {
        let (start, end) = range_bounds_to_start_end(idx_range, 0, self.len());

        debug_assert!(start <= end);
        debug_assert!(end <= self.len());

        for child in &self.children.as_slice()[start..end] {
            self.summary -= child.summary();
            self.leaf_count -= child.leaf_count();
        }

        self.children.drain(start..end)
    }

    #[inline]
    pub(super) fn empty() -> Self {
        Self {
            children: Children::empty(),
            depth: 1,
            leaf_count: 0,
            summary: Default::default(),
        }
    }

    /// # Panics
    ///
    /// Panics if the inode is empty.
    #[inline]
    pub(super) fn first(&self) -> &Arc<Node<N, L>> {
        &self.children()[0]
    }

    /// Creates a new inode from its children.
    ///
    /// # Panics
    ///
    /// Panics if `children` yields zero nodes, more than `max_children` nodes
    /// or nodes at different depths.
    #[inline]
    pub(super) fn from_children<I>(children: I) -> Self
    where
        I: IntoIterator<Item = Arc<Node<N, L>>>,
    {
        let mut inode = Self::empty();

        for child in children.into_iter() {
            inode.push(child);
        }

        inode
    }

    /// Constructs a new inode from an arbitrarily long sequence of nodes.
    ///
    /// Note that unlike [`Self::from_children()`] the `nodes` iterator is
    /// allowed to yield more that `max_children` nodes without causing a
    /// panic.
    ///
    /// # Panics
    ///
    /// Panics if `nodes` yields less than 2 nodes or if it yields nodes at
    /// different depths.
    #[inline]
    pub(super) fn from_nodes<I>(nodes: I) -> Self
    where
        I: IntoIterator<Item = Arc<Node<N, L>>>,
        I::IntoIter: ExactSizeIterator,
    {
        let nodes = nodes.into_iter();

        debug_assert!(nodes.len() > 1);

        if nodes.len() <= Self::max_children() {
            return Self::from_children(nodes);
        }

        let mut nodes = ChildSegmenter::new(nodes)
            .map(Node::Internal)
            .map(Arc::new)
            .collect::<Vec<_>>();

        while nodes.len() > Self::max_children() {
            nodes = ChildSegmenter::new(nodes.into_iter())
                .map(Node::Internal)
                .map(Arc::new)
                .collect();
        }

        debug_assert!(nodes.len() > 1);

        Self::from_children(nodes)
    }

    #[inline]
    pub(super) fn is_underfilled(&self) -> bool {
        self.len() < Self::min_children()
    }

    /// Inserts a child node at `child_offset`, so that the new child will have
    /// `child_offset` sibiling nodes to its left. Note that if this inode was
    /// empty its depth will then be `child.depth() + 1`.
    ///
    /// # Panics
    ///
    /// Panics if the inode is already full or if `child` is a depth different
    /// than `self.depth() - 1` if the inode already contained some children.
    #[inline]
    pub(super) fn insert(
        &mut self,
        child_offset: usize,
        child: Arc<Node<N, L>>,
    ) {
        if self.is_empty() {
            self.depth = child.depth() + 1;
        }

        debug_assert!(!self.is_full());
        debug_assert_eq!(child.depth() + 1, self.depth());

        self.leaf_count += child.leaf_count();
        self.summary += child.summary();
        self.children.insert(child_offset, child);
    }

    /// TODO: docs
    #[inline]
    pub(super) fn insert_at_depth(
        &mut self,
        child_offset: usize,
        node: Arc<Node<N, L>>,
    ) where
        L: BalancedLeaf + Clone,
    {
        debug_assert!(!self.is_empty());
        debug_assert!(child_offset <= self.len());
        debug_assert!(self.depth() >= 2);
        debug_assert!(node.depth() < self.depth() - 1);

        if child_offset > 0 {
            let extra = self.with_child_mut(child_offset - 1, |previous| {
                let previous = {
                    let n = Arc::make_mut(previous);
                    // SAFETY: this inode's depth is >= 2 so its children are
                    // also inodes.
                    unsafe { n.as_mut_internal_unchecked() }
                };
                previous.append_at_depth(node)
            });

            if let Some(extra) = extra {
                self.insert(child_offset, Arc::new(Node::Internal(extra)));
            }
        } else {
            let extra = self.with_child_mut(0, |first| {
                let first = {
                    let n = Arc::make_mut(first);
                    // SAFETY: this inode's depth is >= 2 so its children are
                    // also inodes.
                    unsafe { n.as_mut_internal_unchecked() }
                };

                first.prepend_at_depth(node)
            });

            if let Some(extra) = extra {
                self.insert(0, Arc::new(Node::Internal(extra)));
            }
        }
    }

    /// Inserts a sequence of child nodes at the given offset, so that the
    /// first child yielded by the iterator will have `child_offset` siblings
    /// nodes to its left.
    ///
    /// If the input iterator yields more children than its possible to fit in
    /// this inode this function will return an iterator over other inodes at
    /// the same depth of this inode and all with a valid number of children.
    ///
    /// Finally, this function may split this inode's children if necessary,
    /// meaning the childen nodes on the right side of the split (i.e. in the
    /// range `(child_offset + 1)..inode.len()`) will be the in last inode
    /// yielded by the output iterator.
    ///
    /// # Panics
    ///
    /// Panics if `children` yields nodes at depths other than
    /// `self.depth() - 1`.
    #[inline]
    pub(super) fn insert_children<I>(
        &mut self,
        mut child_offset: usize,
        children: I,
    ) -> Option<impl ExactSizeIterator<Item = Self>>
    where
        I: IntoIterator<Item = Arc<Node<N, L>>>,
        I::IntoIter: ExactSizeIterator,
    {
        let mut children = children.into_iter();

        if self.len() + children.len() <= Self::max_children() {
            for child in children {
                debug_assert_eq!(self.depth(), child.depth() + 1);
                self.insert(child_offset, child);
                child_offset += 1;
            }

            return None;
        }

        let mut last_children =
            self.drain(child_offset..).collect::<Vec<_>>().into_iter();

        debug_assert!(
            self.len() + children.len() + last_children.len()
                > Self::max_children()
        );

        while self.is_underfilled() {
            let next = if let Some(next) = children.next() {
                next
            } else {
                last_children.next().unwrap()
            };
            self.push(next);
        }

        let first_children =
            if children.len() + last_children.len() >= Self::min_children() {
                Vec::new()
            } else {
                let missing = Self::min_children()
                    - (children.len() + last_children.len());

                self.drain(self.len() - missing..).collect()
            };

        debug_assert!(!self.is_underfilled());

        debug_assert!(
            first_children.len() + children.len() + last_children.len()
                >= Self::min_children()
        );

        Some(ChildSegmenter::new(
            first_children
                .into_iter()
                .exact_chain(children)
                .exact_chain(last_children),
        ))
    }

    #[inline]
    pub(super) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub(super) fn is_full(&self) -> bool {
        self.len() == Self::max_children()
    }

    #[inline]
    pub(super) fn last(&self) -> &Arc<Node<N, L>> {
        let last_idx = self.len() - 1;
        &self.children()[last_idx]
    }

    /// The number of children contained in this internal node.
    #[inline]
    pub(super) fn len(&self) -> usize {
        self.children.len()
    }

    #[inline]
    pub(super) fn leaf_count(&self) -> usize {
        self.leaf_count
    }

    #[inline]
    pub(super) const fn max_children() -> usize {
        N
    }

    #[inline]
    pub fn measure<M: Metric<L>>(&self) -> M {
        M::measure(self.summary())
    }

    #[inline]
    pub(super) const fn min_children() -> usize {
        let min = N / 2;
        assert!(min >= 2);
        min
    }

    /// Prepends the node at the right depth.
    ///
    /// If all the nodes on the left side of the subtree up to the one to
    /// which `node` should be prepended are already full this will return a
    /// new inode at the same depth as `self` to be inserted right before
    /// `self`.
    #[inline]
    pub(super) fn prepend_at_depth(
        &mut self,
        mut node: Arc<Node<N, L>>,
    ) -> Option<Self>
    where
        L: BalancedLeaf + Clone,
    {
        debug_assert!(node.depth() < self.depth());

        if self.depth() > node.depth() + 1 {
            debug_assert!(self.depth() >= 2);

            let extra = self.with_child_mut(0, |first| {
                let first = Arc::make_mut(first);
                // SAFETY: this inode's depth is >= 2 so its children are
                // also inodes.
                let first = unsafe { first.as_mut_internal_unchecked() };
                first.prepend_at_depth(node)
            })?;

            node = Arc::new(Node::Internal(extra));
        }

        debug_assert_eq!(self.depth(), node.depth() + 1);

        if node.is_underfilled() {
            self.with_child_mut(0, |first| {
                Arc::make_mut(&mut node).balance(Arc::make_mut(first));
            });

            if self.first().is_empty() {
                self.swap(0, node);
                return None;
            }
        }

        if !self.is_full() {
            self.insert(0, node);
            None
        } else {
            let new_self =
                Self::from_children(self.drain(Self::min_children()..));
            let mut other = core::mem::replace(self, new_self);
            other.insert(0, node);
            debug_assert!(!self.is_underfilled());
            debug_assert!(!other.is_underfilled());
            Some(other)
        }
    }

    /// Pushes a new node to this inode's children. Note that if this inode was
    /// empty its depth will then be `child.depth() + 1`.
    ///
    /// # Panics
    ///
    /// Panics if the inode is already full or if `child` is a depth different
    /// than `self.depth() - 1` if the inode already contained some children.
    #[inline]
    pub(super) fn push(&mut self, child: Arc<Node<N, L>>) {
        if self.is_empty() {
            self.depth = child.depth() + 1;
        }

        debug_assert!(!self.is_full());
        debug_assert_eq!(child.depth() + 1, self.depth());

        self.leaf_count += child.leaf_count();
        self.summary += child.summary();
        self.children.push(child);
    }

    /// Removes the child at `child_idx`, returning it.
    ///
    /// # Panics
    ///
    /// Panics if `child_idx` is greater or equal to the length of this inode.
    #[inline]
    pub(super) fn remove(&mut self, child_idx: usize) -> Arc<Node<N, L>> {
        debug_assert!(child_idx < self.len());
        let child = self.children.remove(child_idx);
        self.leaf_count -= child.leaf_count();
        self.summary -= child.summary();
        child
    }

    #[inline]
    pub(super) fn summary(&self) -> &L::Summary {
        &self.summary
    }

    /// Swaps the child at `child_idx` with a new child.
    ///
    /// # Panics
    ///
    /// Panics if `child_idx` is greater or equal to the length of this inode
    /// or if the new child is at a depth different than `self.depth() - 1`.
    #[inline]
    pub(super) fn swap(
        &mut self,
        child_idx: usize,
        new_child: Arc<Node<N, L>>,
    ) {
        debug_assert!(child_idx < self.len());
        debug_assert_eq!(new_child.depth() + 1, self.depth());

        let to_swap = &self.children.as_slice()[child_idx];
        self.summary -= to_swap.summary();
        self.leaf_count -= to_swap.leaf_count();

        self.summary += new_child.summary();
        self.leaf_count += new_child.leaf_count();
        self.children_mut()[child_idx] = new_child;
    }

    /// Returns mutable references to the child nodes at `first_idx` and
    /// `second_idx`, respectively.
    ///
    /// # Panics
    ///
    /// Will panic if `first_idx >= second_idx`  and if
    /// `second_idx >= self.len()`.
    #[inline]
    fn two_mut(
        &mut self,
        first_idx: usize,
        second_idx: usize,
    ) -> (&mut Arc<Node<N, L>>, &mut Arc<Node<N, L>>) {
        debug_assert!(first_idx < second_idx);
        debug_assert!(second_idx < self.len());

        let split_at = first_idx + 1;
        let (first, second) = self.children_mut().split_at_mut(split_at);
        (&mut first[first_idx], &mut second[second_idx - split_at])
    }

    /// Calls a function taking a mutable reference to the child at `child_idx`
    /// making sure this inode's summary and leaf count are updated correctly
    /// in case that child's summary or leaf count change as a result of
    /// calling `fun`.
    #[inline]
    pub(super) fn with_child_mut<F, T>(
        &mut self,
        child_idx: usize,
        fun: F,
    ) -> T
    where
        F: FnOnce(&mut Arc<Node<N, L>>) -> T,
    {
        let child = &mut self.children.as_slice_mut()[child_idx];

        self.summary -= child.summary();
        self.leaf_count -= child.leaf_count();

        let ret = fun(child);

        self.summary += child.summary();
        self.leaf_count += child.leaf_count();

        ret
    }
}

/// Takes an iterator of `n` nodes (with `n >= min_children`) at depth `d`
/// and gives back inodes of depth `d + 1` that are all guaranteed to have
/// between `min_children` and `max_children` children.
struct ChildSegmenter<const N: usize, L, Children>
where
    L: Leaf,
    Children: ExactSizeIterator<Item = Arc<Node<N, L>>>,
{
    children: Children,
}

impl<const N: usize, L, Children> ChildSegmenter<N, L, Children>
where
    L: Leaf,
    Children: ExactSizeIterator<Item = Arc<Node<N, L>>>,
{
    /// # Panics
    ///
    /// Panics if `children` yields less than `min_children` children.
    #[inline]
    fn new(children: Children) -> Self {
        debug_assert!(children.len() >= Inode::<N, L>::min_children());
        Self { children }
    }
}

impl<const N: usize, L, Children> Iterator for ChildSegmenter<N, L, Children>
where
    L: Leaf,
    Children: ExactSizeIterator<Item = Arc<Node<N, L>>>,
{
    type Item = Inode<N, L>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let min_children = Inode::<N, L>::min_children();
        let max_children = Inode::<N, L>::max_children();
        let remaining = self.children.len();

        debug_assert!(remaining == 0 || remaining >= min_children);

        let take = if remaining == 0 {
            return None;
        } else if remaining > max_children {
            if remaining - max_children >= min_children {
                max_children
            } else {
                remaining - min_children
            }
        } else {
            remaining
        };

        debug_assert!(take >= min_children && take <= max_children);

        debug_assert!(remaining >= take);

        Some(Inode::from_children(self.children.by_ref().take(take)))
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let exact = self.len();
        (exact, Some(exact))
    }
}

impl<const N: usize, L, Children> ExactSizeIterator
    for ChildSegmenter<N, L, Children>
where
    L: Leaf,
    Children: ExactSizeIterator<Item = Arc<Node<N, L>>>,
{
    #[inline]
    fn len(&self) -> usize {
        let remaining = self.children.len();
        let max_children = Inode::<N, L>::max_children();
        remaining / max_children + ((remaining % max_children != 0) as usize)
    }
}

/// Recursively prints a tree-like representation of this node.
///
/// Called by the `Debug` impl of [`Inode`] when using the pretty-print
/// modifier (i.e. `{:#?}`).
#[inline]
fn pretty_print_inode<const N: usize, L: Leaf>(
    inode: &Inode<N, L>,
    shifts: &mut String,
    ident: &str,
    last_shift_byte_len: usize,
    f: &mut core::fmt::Formatter,
) -> core::fmt::Result {
    writeln!(
        f,
        "{}{}{:?}",
        &shifts[..shifts.len() - last_shift_byte_len],
        ident,
        inode.summary()
    )?;

    for (i, child) in inode.children().iter().enumerate() {
        let is_last = i + 1 == inode.len();
        let ident = if is_last { "└── " } else { "├── " };
        match &**child {
            Node::Internal(inode) => {
                let shift = if is_last { "    " } else { "│   " };
                shifts.push_str(shift);
                pretty_print_inode(inode, shifts, ident, shift.len(), f)?;
                shifts.truncate(shifts.len() - shift.len());
            },
            Node::Leaf(leaf) => {
                writeln!(f, "{}{}{:#?}", &shifts, ident, &leaf)?;
            },
        }
    }

    Ok(())
}

use inner_children::{Children, Drain};

mod inner_children {
    use std::mem::{self, MaybeUninit};
    use std::ops::Range;
    use std::ptr;

    use super::*;

    pub(super) struct Children<const N: usize, L: Leaf> {
        children: [MaybeUninit<Arc<Node<N, L>>>; N],
        len: u8,
    }

    impl<const N: usize, L: Leaf> std::fmt::Debug for Children<N, L> {
        #[inline]
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.debug_list().entries(self.as_slice()).finish()
        }
    }

    impl<const N: usize, L: Leaf> Children<N, L> {
        #[inline(always)]
        pub(super) fn append(&mut self, other: &mut Self) {
            debug_assert!(self.len() + other.len() <= N);

            unsafe {
                ptr::copy(
                    other.children.as_ptr(),
                    self.children.as_mut_ptr().add(self.len()),
                    other.len(),
                );
            }

            other.len = 0;
        }

        #[inline(always)]
        pub(super) fn as_slice(&self) -> &[Arc<Node<N, L>>] {
            unsafe { mem::transmute(&self.children[..self.len()]) }
        }

        #[inline(always)]
        pub(super) fn as_slice_mut(&mut self) -> &mut [Arc<Node<N, L>>] {
            unsafe { mem::transmute(&mut self.children[..self.len as _]) }
        }

        #[inline(always)]
        pub(super) fn drain(
            &mut self,
            idx_range: Range<usize>,
        ) -> Drain<'_, N, L> {
            Drain {
                children: self,
                start: idx_range.start,
                yielded: 0,
                end: idx_range.end,
            }
        }

        #[inline(always)]
        pub(super) fn insert(
            &mut self,
            offset: usize,
            child: Arc<Node<N, L>>,
        ) {
            debug_assert!(offset <= self.len());
            debug_assert!(self.len() < N);

            unsafe {
                let ptr = self.children.as_mut_ptr();

                ptr::copy(
                    ptr.add(offset),
                    ptr.add(offset + 1),
                    self.len() - offset,
                );
            };

            self.children[offset].write(child);

            self.len += 1;
        }

        #[inline(always)]
        pub(super) fn len(&self) -> usize {
            self.len as _
        }

        #[inline(always)]
        pub(super) fn empty() -> Self {
            Self {
                // SAFETY: An uninitialized `[MaybeUninit<_>; LEN]` is valid.
                children: unsafe { MaybeUninit::uninit().assume_init() },
                len: 0,
            }
        }

        #[inline(always)]
        pub(super) fn push(&mut self, child: Arc<Node<N, L>>) {
            debug_assert!(self.len() < N);
            self.children[self.len()].write(child);
            self.len += 1;
        }

        #[inline(always)]
        pub(super) fn remove(&mut self, idx: usize) -> Arc<Node<N, L>> {
            debug_assert!(idx < self.len());

            let child =
                unsafe { ptr::read(&self.children[idx]).assume_init() };

            unsafe {
                let ptr = self.children.as_mut_ptr();

                ptr::copy(
                    ptr.add(idx + 1),
                    ptr.add(idx),
                    self.len() - idx - 1,
                );
            }

            self.len -= 1;

            child
        }
    }

    impl<const N: usize, L: Leaf> Drop for Children<N, L> {
        #[inline(always)]
        fn drop(&mut self) {
            for child in &mut self.children[..self.len as usize] {
                unsafe { ptr::drop_in_place(child.as_mut_ptr()) };
            }
        }
    }

    impl<const N: usize, L: Leaf> Clone for Children<N, L> {
        #[inline(always)]
        fn clone(&self) -> Self {
            let mut cloned = Self::empty();

            for (new, this) in
                cloned.children[..self.len()].iter_mut().zip(self.as_slice())
            {
                new.write(Arc::clone(this));
            }

            cloned.len = self.len;

            cloned
        }
    }

    pub(in crate::tree) struct Drain<'a, const N: usize, L: Leaf> {
        children: &'a mut Children<N, L>,
        start: usize,
        yielded: usize,
        end: usize,
    }

    impl<'a, const N: usize, L: Leaf> Iterator for Drain<'a, N, L> {
        type Item = Arc<Node<N, L>>;

        #[inline(always)]
        fn next(&mut self) -> Option<Self::Item> {
            if self.start + self.yielded == self.end {
                return None;
            }

            let child = unsafe {
                ptr::read(&self.children.children[self.start + self.yielded])
                    .assume_init()
            };

            self.yielded += 1;

            Some(child)
        }
    }

    impl<'a, const N: usize, L: Leaf> ExactSizeIterator for Drain<'a, N, L> {
        #[inline(always)]
        fn len(&self) -> usize {
            (self.end - self.start) - self.yielded
        }
    }

    impl<'a, const N: usize, L: Leaf> Drop for Drain<'a, N, L> {
        #[inline(always)]
        fn drop(&mut self) {
            // Drop the children that haven't been yielded.
            for child in &mut self.children.children
                [self.start + self.yielded..self.end]
            {
                unsafe { ptr::drop_in_place(child.as_mut_ptr()) };
            }

            // Close any gap left by yielding the range.
            if self.end < self.children.len() {
                unsafe {
                    let ptr = self.children.children.as_mut_ptr();

                    ptr::copy(
                        ptr.add(self.end),
                        ptr.add(self.start),
                        self.children.len() - self.end,
                    );
                }
            }

            self.children.len -= (self.end - self.start) as u8;
        }
    }
}
