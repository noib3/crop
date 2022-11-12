use std::fmt::{self, Debug};
use std::ops::{AddAssign, Range};
use std::sync::Arc;

use super::{Inode, Leaves, Metric, Node, TreeSlice};

/// TODO: docs
pub trait Summarize: Debug + Clone {
    type Summary: Debug
        + Default
        + Clone
        + for<'a> AddAssign<&'a Self::Summary>;

    /// TODO: docs
    fn summarize(&self) -> Self::Summary;
}

/// TODO: docs
pub struct Tree<const FANOUT: usize, Leaf: Summarize> {
    root: Arc<Node<FANOUT, Leaf>>,
}

impl<const N: usize, Leaf: Summarize> Debug for Tree<N, Leaf> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if !f.alternate() {
            f.debug_struct("Tree").field("root", &self.root).finish()
        } else {
            let punctuation =
                if self.root.is_internal() { " —" } else { ":" };

            write!(f, "root{} {:#?}", punctuation, self.root)
        }
    }
}

/// TODO: docs
impl<const FANOUT: usize, Leaf: Summarize> Tree<FANOUT, Leaf> {
    /// # Panics
    ///
    /// This function will panic if the iterator is empty.
    #[inline]
    pub fn from_leaves<I>(leaves: I) -> Self
    where
        I: IntoIterator<Item = Leaf>,
        I::IntoIter: ExactSizeIterator,
    {
        let mut leaves = leaves.into_iter();

        if leaves.len() == 0 {
            panic!(
                "Cannot construct a Tree<{}, {}> from an empty iterator",
                FANOUT,
                std::any::type_name::<Leaf>(),
            )
        }

        if leaves.len() == 1 {
            let leaf = super::Leaf::from_value(leaves.next().unwrap());
            return Tree { root: Arc::new(Node::Leaf(leaf)) };
        }

        Tree { root: Arc::new(Node::Internal(Inode::from_leaves(leaves))) }
    }

    /// TODO: docs
    #[inline]
    pub fn slice<M>(&self, range: Range<M>) -> TreeSlice<'_, FANOUT, Leaf>
    where
        M: Metric<Leaf>,
    {
        assert!(M::zero() <= range.start);
        assert!(range.start <= range.end);
        assert!(range.end <= M::measure(self.summary()));

        if range.start == range.end {
            TreeSlice::empty()
        } else {
            TreeSlice::from_range_in_node(&*self.root, range)
        }
    }

    /// Returns an iterator over the leaves of this tree.
    #[inline]
    pub fn leaves(&self) -> Leaves<'_, Leaf> {
        let mut leaves = Leaves::new();
        leaves.push_node_subtree(&*self.root);
        leaves
    }

    /// TODO: docs
    #[inline]
    pub fn summary(&self) -> &Leaf::Summary {
        self.root.summary()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Copy, Clone, Default, Debug, Eq, PartialEq)]
    pub struct Count(usize);

    impl<'a> AddAssign<&'a Self> for Count {
        fn add_assign(&mut self, rhs: &'a Self) {
            self.0 += rhs.0;
        }
    }

    impl Summarize for usize {
        type Summary = Count;

        fn summarize(&self) -> Self::Summary {
            Count(*self)
        }
    }

    impl Metric<usize> for usize {
        fn zero() -> Self {
            0
        }

        fn measure(count: &Count) -> Self {
            count.0
        }
    }

    #[test]
    fn easy() {
        let tree = Tree::<4, usize>::from_leaves(0..20);
        assert_eq!(Count(190), *tree.summary());
    }

    #[test]
    fn pretty_print() {
        let _tree = Tree::<2, usize>::from_leaves(0..10);
        // println!("{:#?}", tree);
        // panic!("")
    }

    #[test]
    fn slice() {
        let tree = Tree::<2, usize>::from_leaves(0..10);
        let slice = tree.slice(4..6);
        println!("{:#?}", tree);
        assert_eq!(Count(3), *slice.summary());
    }
}
