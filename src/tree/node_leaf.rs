use super::{Leaf, Metric};

#[derive(Clone, Default)]
pub(super) struct Lnode<L: Leaf> {
    pub(super) value: L,
    pub(super) summary: L::Summary,
}

impl<L: Leaf> From<(L, L::Summary)> for Lnode<L> {
    #[inline]
    fn from((value, summary): (L, L::Summary)) -> Self {
        Self { value, summary }
    }
}

impl<L: Leaf> std::fmt::Debug for Lnode<L> {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        if !f.alternate() {
            f.debug_struct("Lnode")
                .field("value", &self.value)
                .field("summary", &self.summary)
                .finish()
        } else {
            write!(f, "{:?} — {:?}", self.value, self.summary)
        }
    }
}

impl<L: Leaf> From<L> for Lnode<L> {
    #[inline]
    fn from(value: L) -> Self {
        Self { summary: value.summarize(), value }
    }
}

impl<L: Leaf> Lnode<L> {
    #[inline]
    pub(super) fn as_slice(&self) -> &L::Slice {
        self.value.borrow()
    }

    #[inline]
    pub fn base_measure(&self) -> L::BaseMetric {
        self.measure::<L::BaseMetric>()
    }

    #[inline]
    pub fn measure<M: Metric<L>>(&self) -> M {
        M::measure(self.summary())
    }

    #[inline]
    pub(super) fn new(value: L, summary: L::Summary) -> Self {
        Self { value, summary }
    }

    #[inline]
    pub(super) fn is_big_enough(&self) -> bool {
        L::is_leaf_big_enough(self.value(), self.summary())
    }

    #[inline]
    pub(super) fn summary(&self) -> &L::Summary {
        &self.summary
    }

    #[inline]
    pub(super) fn value(&self) -> &L {
        &self.value
    }
}
