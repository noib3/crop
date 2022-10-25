use std::borrow::Cow;
use std::fmt;

use super::{Inode, Leaf, Summarize};

pub(super) enum Node<const FANOUT: usize, Chunk: Summarize> {
    Internal(Inode<FANOUT, Chunk>),
    Leaf(Leaf<Chunk>),
}

impl<const FANOUT: usize, Chunk: Summarize> fmt::Debug
    for Node<FANOUT, Chunk>
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if !f.alternate() {
            match self {
                Self::Internal(inode) => {
                    f.debug_tuple("Internal").field(&inode).finish()
                },
                Self::Leaf(leaf) => {
                    f.debug_tuple("Leaf").field(&leaf).finish()
                },
            }
        } else {
            match self {
                Self::Internal(inode) => write!(f, "{:#?}", inode),
                Self::Leaf(leaf) => write!(f, "{:#?}", leaf),
            }
        }
    }
}

impl<const FANOUT: usize, Chunk: Summarize> Node<FANOUT, Chunk> {
    pub(super) fn is_internal(&self) -> bool {
        matches!(self, Node::Internal(_))
    }

    pub(super) fn is_leaf(&self) -> bool {
        matches!(self, Node::Leaf(_))
    }

    /// TODO: docs
    pub(super) fn summarize(&self) -> Cow<'_, Chunk::Summary> {
        match self {
            Node::Internal(inode) => {
                let mut nodes = inode.children().into_iter();

                match (nodes.next(), nodes.next()) {
                    (Some(first), Some(second)) => {
                        let mut summary = first.summarize().into_owned();
                        summary += &*second.summarize();
                        for node in nodes {
                            summary += &*node.summarize();
                        }
                        Cow::Owned(summary)
                    },

                    (Some(first), None) => first.summarize(),

                    (None, Some(_)) => unreachable!(),

                    (None, None) => Cow::Owned(Chunk::Summary::default()),
                }
            },

            Node::Leaf(leaf) => Cow::Borrowed(&leaf.summary),
        }
    }
}
