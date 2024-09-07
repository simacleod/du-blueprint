use std::{array, fmt::Debug};

use parry3d_f64::math::Point;

use crate::squarion::*;

#[derive(Debug)]
pub enum SvoNode<T> {
    Leaf(T),
    Internal(T, Box<[SvoNode<T>; 8]>),
}

pub enum SvoReturn<T> {
    Leaf(T),
    Internal(T),
}

impl<T> SvoNode<T> {
    fn from_fn<F>(range: &RangeZYX, func: &F) -> Self
    where
        F: Fn(&RangeZYX) -> SvoReturn<T>,
    {
        assert!(range.size.min() != 0);
        match func(range) {
            SvoReturn::Leaf(v) => SvoNode::Leaf(v),
            SvoReturn::Internal(v) => SvoNode::Internal(
                v,
                Box::new(range.split_at_center().map(|o| Self::from_fn(&o, func))),
            ),
        }
    }

    pub fn cata<F, R>(&self, range: &RangeZYX, func: &mut F) -> R
    where
        F: FnMut(&RangeZYX, &T, Option<[R; 8]>) -> R,
    {
        match self {
            SvoNode::Leaf(v) => func(range, v, None),
            SvoNode::Internal(v, children) => {
                let octants = range.split_at_center();
                let results = array::from_fn(|i| children[i].cata(&octants[i], func));
                func(range, v, Some(results))
            }
        }
    }

    fn into_cata<F, R>(self, range: &RangeZYX, func: &mut F) -> R
    where
        F: FnMut(&RangeZYX, T, Option<[R; 8]>) -> R,
    {
        match self {
            SvoNode::Leaf(v) => func(range, v, None),
            SvoNode::Internal(v, children) => {
                let octants = range.split_at_center();
                // This is the only good way to move out of an array. It's kinda dumb.
                let mut i = 0;
                let results = children.map(|c| {
                    let result = c.into_cata(&octants[i], func);
                    i += 1;
                    result
                });
                func(range, v, Some(results))
            }
        }
    }

}

pub struct Svo<T> {
    pub root: SvoNode<T>,
    pub range: RangeZYX,
}

impl<T> Svo<T> {
    pub fn from_fn<F>(origin: Point<i32>, extent: usize, func: &F) -> Self
    where
        F: Fn(&RangeZYX) -> SvoReturn<T>,
    {
        assert!(extent.is_power_of_two());
        let range = RangeZYX::with_extent(origin, extent as i32);
        Self {
            root: SvoNode::from_fn(&range, func),
            range,
        }
    }

    pub fn cata<F, R>(&self, mut func: F) -> R
    where
        F: FnMut(&RangeZYX, &T, Option<[R; 8]>) -> R,
    {
        self.root.cata(&self.range, &mut func)
    }

    pub fn into_map<F, R>(self, mut func: F) -> Svo<R>
    where
        F: FnMut(T) -> R,
    {
        Svo {
            root: self.root.into_cata(&self.range, &mut |_, v, cs| match cs {
                Some(cs) => SvoNode::Internal(func(v), Box::new(cs)),
                None => SvoNode::Leaf(func(v)),
            }),
            range: self.range,
        }
    }

}

impl SvoNode<Option<VoxelCellData>> {
    /// Checks if the current SvoNode is empty.
    pub fn is_empty(&self) -> bool {
        match self {
            SvoNode::Leaf(None) => true, // A leaf with no data is considered empty
            SvoNode::Internal(None, children) => {
                children.iter().all(|child| child.is_empty()) // Internal node is empty if all children are empty
            }
            _ => false, // Any node with data is not empty
        }
    }

    /// Recursively prunes empty grids in the SvoNode
    fn prune_empty_grids(self) -> SvoNode<Option<VoxelCellData>> {
        match self {
            // If it's a leaf with no data, return None (pruned)
            SvoNode::Leaf(Some(cell_data)) => {
                if cell_data.grid.is_empty() {
                    SvoNode::Leaf(None) // Prune if the grid is empty
                } else {
                    SvoNode::Leaf(Some(cell_data)) // Keep the data if grid is not empty
                }
            }

            // Internal node with children, recursively prune children
            SvoNode::Internal(Some(cell_data), children) => {
                let pruned_children: Box<[SvoNode<Option<VoxelCellData>>; 8]> =
                    Box::new(children.map(|child| child.prune_empty_grids()));

                // If all children are pruned, return None
                if pruned_children.iter().all(|child| child.is_empty()) {
                    SvoNode::Leaf(None) // Prune internal node if all children are empty
                } else {
                    SvoNode::Internal(Some(cell_data), pruned_children) // Keep node if at least one child is not empty
                }
            }

            // If the node is already None, just return it
            SvoNode::Leaf(None) | SvoNode::Internal(None, _) => SvoNode::Leaf(None),
        }
    }
}

impl Svo<Option<VoxelCellData>> {
    /// Prunes empty grids from the root node downwards.
    pub fn prune_empty_grids(self) -> Svo<Option<VoxelCellData>> {
        Svo {
            root: self.root.prune_empty_grids(),
            range: self.range, // Keep the range unchanged
        }
    }
}