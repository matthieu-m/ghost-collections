//! An Indexed Balanced Binary Tree, with externally supplied token.
//!
//! The `TripodTree` is building block for Binary Trees, out of the box, it provides:
//!
//! -   Order-Preservation: the relative order of inserted items is preserved throughout mutations.
//! -   Balancing: the tree is balanced automatically, so that at any point the left-subtree and right-subtree number
//!     of elements differ by at most a factor of 2.
//! -   Indexing: each element in the tree is indexed by a number in [0, N), where N is the number of elements,
//!     according to their order.
//!
//! The `TripodTree`, however, does not by itself establish any order, it simply preserves the order of insertion.

mod cursor;
mod iter;

pub use cursor::{Cursor, CursorMut};
pub use iter::Iter;

use core::{
    cell::Cell,
    cmp,
    mem,
    ops::{Bound, Range, RangeBounds},
};

use ghost_cell::{GhostCell, GhostToken};
use static_rc::StaticRc;

#[cfg(feature = "experimental-ghost-cursor")]
use ghost_cell::GhostCursor;

/// A safe implementation of an indexed balanced binary tree.
///
/// Each node contains 1 element as well as 4 pointers: up, left, right, and the tripod pointer.
pub struct TripodTree<'brand, T> {
    root: Option<QuarterNodePtr<'brand, T>>,
}

impl<'brand, T> TripodTree<'brand, T> {
    /// Creates a new, empty, instance.
    pub const fn new() -> Self { Self { root: None, } }

    /// Creates a new instance, with a single value.
    pub fn singleton(value: T, token: &mut GhostToken<'brand>) -> Self {
        Self { root: Some(Self::from_value(value, token)) }
    }

    /// Creates an iterator over the entire tree, from front to back.
    ///
    /// #   Complexity
    ///
    /// The complexity of this method itself is O(1).
    ///
    /// The complexity of calling `next` on the resulting iterator is O(log N) in the number of elements.
    pub fn iter<'a>(&'a self, token: &'a GhostToken<'brand>) -> Iter<'a, 'brand, T> {
        Iter::new(token, self)
    }

    /// Creates an iterator over the specified range, from front to back.
    ///
    /// If the start bound is greater than the end bound, this is empty.
    ///
    /// #   Complexity
    ///
    /// The complexity of this method itself is O(1).
    ///
    /// The complexity of calling `next` on the resulting iterator is O(log N) in the number of elements.
    pub fn iter_range<'a, R>(&'a self, range: R, token: &'a GhostToken<'brand>) -> Iter<'a, 'brand, T>
    where
        R: RangeBounds<usize>,
    {
        let range = self.into_range(range, token);

        Iter::range(token, self, range)
    }

    /// Creates a cursor pointing to the root element.
    pub fn cursor<'a>(&'a self, token: &'a GhostToken<'brand>) -> Cursor<'a, 'brand, T> {
        Cursor::new(token, self)
    }

    /// Creates a mutable cursor pointing to the root element.
    pub fn cursor_mut<'a>(&'a mut self, token: &'a mut GhostToken<'brand>) -> CursorMut<'a, 'brand, T> {
        CursorMut::new(token, self)
    }

    /// Creates a cursor pointing to the front element.
    pub fn cursor_front<'a>(&'a self, token: &'a GhostToken<'brand>) -> Cursor<'a, 'brand, T> {
        Cursor::new_front(token, self)
    }

    /// Creates a mutable cursor pointing to the front element.
    pub fn cursor_front_mut<'a>(&'a mut self, token: &'a mut GhostToken<'brand>) -> CursorMut<'a, 'brand, T> {
        CursorMut::new_front(token, self)
    }

    /// Creates a cursor pointing to the back element.
    pub fn cursor_back<'a>(&'a self, token: &'a GhostToken<'brand>) -> Cursor<'a, 'brand, T> {
        Cursor::new_back(token, self)
    }

    /// Creates a mutable cursor pointing to the back element.
    pub fn cursor_back_mut<'a>(&'a mut self, token: &'a mut GhostToken<'brand>) -> CursorMut<'a, 'brand, T> {
        CursorMut::new_back(token, self)
    }

    /// Returns whether the tree is empty, or not.
    pub fn is_empty(&self) -> bool { self.root.is_none() }

    /// Returns the number of elements in the tree.
    pub fn len(&self, token: &GhostToken<'brand>) -> usize {
        self.root.as_ref().map(|node| node.borrow(token).size).unwrap_or(0)
    }

    /// Clears the tree of all elements.
    ///
    /// #   Complexity
    ///
    /// -   Time: O(N) in the number of elements.
    /// -   Space: O(1).
    ///
    /// Note: if a panic occurs, because dropping an element panics, then the tree is left is an unusable state.
    pub fn clear(&mut self, token: &mut GhostToken<'brand>) {
        if let Some(root) = self.root.take() {
            let mut tripod = root.borrow(token).deploy();

            //  O(N) iterations, performing O(1) work each.
            loop {
                //  Clear the left sub-tree first.
                if let Some(left) = tripod.borrow(token).left() {
                    let left_tripod = left.borrow(token).deploy();
                    retract(tripod, token);
                    tripod = left_tripod;
                    continue;
                }

                //  And the right sub-tree afterwards.
                if let Some(right) = tripod.borrow(token).right() {
                    let right_tripod = right.borrow(token).deploy();
                    retract(tripod, token);
                    tripod = right_tripod;
                    continue;
                }

                //  Neither left nor right, time to clean and move up!
                if let Some(up) = tripod.borrow_mut(token).up.take() {
                    let up_tripod = up.borrow(token).deploy();

                    let side = tripod.borrow(token).is_child_of(up.borrow(token)).expect("Child!");
                    let child = up_tripod.borrow_mut(token).replace_child(side, up).expect("Child!");

                    retract(tripod, token);
                    Self::node_into_inner(child, token);

                    tripod = up_tripod;
                } else {
                    retract(tripod, token);
                    Self::node_into_inner(root, token);
                    break;
                }
            }
        }
    }

    /// Returns a reference to the front element, if any.
    ///
    /// #   Complexity
    ///
    /// -   Time: O(log N) in the number of elements.
    /// -   Space: O(1).
    pub fn front<'a>(&'a self, token: &'a GhostToken<'brand>) -> Option<&'a T> {
        let mut cursor = self.cursor(token);
        cursor.move_to_front();
        cursor.current()
    }

    /// Returns a reference to the back element, if any.
    ///
    /// #   Complexity
    ///
    /// -   Time: O(log N) in the number of elements.
    /// -   Space: O(1).
    pub fn back<'a>(&'a self, token: &'a GhostToken<'brand>) -> Option<&'a T> {
        let mut cursor = self.cursor(token);
        cursor.move_to_back();
        cursor.current()
    }

    /// Returns a reference to the element at the given index, if any.
    ///
    /// #   Complexity
    ///
    /// -   Time: O(log N) in the number of elements.
    /// -   Space: O(1).
    pub fn at<'a>(&'a self, at: usize, token: &'a GhostToken<'brand>) -> Option<&'a T> {
        if at >= self.len(token) {
            return None;
        }

        let mut cursor = self.cursor(token);
        cursor.move_to(at);
        cursor.current()
    }

    /// Pushes an element to the front of the list.
    ///
    /// #   Complexity
    ///
    /// -   Time: O(log N) in the number of elements.
    /// -   Space: O(1).
    pub fn push_front(&mut self, value: T, token: &mut GhostToken<'brand>) {
        let mut cursor = self.cursor_mut(token);
        cursor.move_to_front();
        cursor.insert_before(value);
    }

    /// Removes and returns the front element of the list, if any.
    ///
    /// #   Complexity
    ///
    /// -   Time: O(log N) in the number of elements.
    /// -   Space: O(1).
    pub fn pop_front(&mut self, token: &mut GhostToken<'brand>) -> Option<T> {
        let mut cursor = self.cursor_mut(token);
        cursor.move_to_front();
        cursor.remove_current()
    }

    /// Pushes an element to the back of the list.
    ///
    /// #   Complexity
    ///
    /// -   Time: O(log N) in the number of elements.
    /// -   Space: O(1).
    pub fn push_back(&mut self, value: T, token: &mut GhostToken<'brand>) {
        let mut cursor = self.cursor_mut(token);
        cursor.move_to_back();
        cursor.insert_after(value);
    }

    /// Removes and returns the back element of the list, if any.
    ///
    /// #   Complexity
    ///
    /// -   Time: O(log N) in the number of elements.
    /// -   Space: O(1).
    pub fn pop_back(&mut self, token: &mut GhostToken<'brand>) -> Option<T> {
        let mut cursor = self.cursor_mut(token);
        cursor.move_to_back();
        cursor.remove_current()
    }

    /// Moves all the elements from `other` to the back of the tree, leaving `other` empty.
    ///
    /// #   Complexity
    ///
    /// -   Time: O(log N) in the total number of elements.
    /// -   Space: O(1).
    ///
    /// No memory allocation nor deallocation occurs.
    pub fn append(&mut self, other: &mut TripodTree<'brand, T>, token: &mut GhostToken<'brand>) {
        let mut cursor = self.cursor_mut(token);
        cursor.move_to_back();
        cursor.splice_after(other);
    }

    /// Moves all the elements from `other` to the front of the tree, leaving `other` empty.
    ///
    /// #   Complexity
    ///
    /// -   Time: O(log N) in the total number of elements.
    /// -   Space: O(1).
    ///
    /// No memory allocation nor deallocation occurs.
    pub fn prepend(&mut self, other: &mut TripodTree<'brand, T>, token: &mut GhostToken<'brand>) {
        let mut cursor = self.cursor_mut(token);
        cursor.move_to_front();
        cursor.splice_before(other);
    }

    /// Splits the tree into two at the given index. Returns everything after the given index, including the index.
    ///
    /// #   Panics
    ///
    /// Panics if `at > self.len()`.
    ///
    /// #   Complexity
    ///
    /// -   Time: O(log?? N) in the number of elements.
    /// -   Space: O(1).
    ///
    /// No memory allocation nor deallocation occurs.
    pub fn split_off(&mut self, at: usize, token: &mut GhostToken<'brand>) -> TripodTree<'brand, T> {
        let length = self.len(token);
        assert!(at <= length, "{} > {}", at, length);

        let mut result = {
            let mut cursor = self.cursor_mut(token);
            cursor.move_to(at);
            cursor.split_before()
        };

        mem::swap(self, &mut result);

        result
    }

    /// Splits the tree into two according to the given range. Returns everything within the range.
    ///
    /// #   Complexity
    ///
    /// -   Time: O(log?? N) in the number of elements.
    /// -   Space: O(1).
    ///
    /// No memory allocation nor deallocation occurs.
    pub fn split<R>(&mut self, range: R, token: &mut GhostToken<'brand>) -> TripodTree<'brand, T>
    where
        R: RangeBounds<usize>,
    {
        let length = self.len(token);
        let range = self.into_range(range, token);

        //  Full Range, well that's easy.
        if range.start == 0 && range.end == length {
            return mem::replace(self, TripodTree::new());
        }

        //  Until the end.
        if range.end == length {
            return self.split_off(range.start, token);
        }

        //  From the start.
        if range.start == 0 {
            let mut result = self.split_off(range.end, token);
            mem::swap(self, &mut result);
            return result;
        }

        //  Interior range.
        let mut result = self.split_off(range.start, token);

        let mut after_end = result.split_off(range.end - range.start, token);

        let mut cursor = self.cursor_mut(token);
        cursor.move_to_back();
        cursor.splice_after(&mut after_end);

        result
    }

    //  Internal; constructs a Range<usize> suitable for the tree.
    fn into_range<R>(&self, range: R, token: &GhostToken<'brand>) -> Range<usize>
    where
        R: RangeBounds<usize>,
    {
        let length = self.len(token);

        let start = match range.start_bound() {
            Bound::Included(n) => cmp::min(*n, length),
            Bound::Excluded(n) => cmp::min(n.saturating_add(1), length),
            Bound::Unbounded => 0,
        };

        let end = match range.end_bound() {
            Bound::Included(n) => cmp::min(n.saturating_add(1), length),
            Bound::Excluded(n) => cmp::min(*n, length),
            Bound::Unbounded => length,
        };

        start..end
    }

    //  Internal; constructs a QuarterNodePtr from a value.
    fn from_value(value: T, token: &mut GhostToken<'brand>) -> QuarterNodePtr<'brand, T> {
        let tripod = Cell::new(None);
        let node = FullNodePtr::new(GhostCell::new(Node { size: 1, value, up: None, left: None, right: None, tripod, }));

        let halves = FullNodePtr::split::<2, 2>(node);
        let (up, tripod) = HalfNodePtr::split::<1, 1>(halves.0);
        let (left, right) = HalfNodePtr::split::<1, 1>(halves.1);

        up.borrow(token).retract(tripod);
        up.borrow_mut(token).left = Some(left);
        up.borrow_mut(token).right = Some(right);

        up
    }

    //  Internal; construct a Tree from QuarterNodePtr.
    fn from_quarter(node: QuarterNodePtr<'brand, T>, token: &GhostToken<'brand>) -> Self {
        let _node = node.borrow(token);
        debug_assert!(_node.up.is_none());
        debug_assert!(_node.is_aliased(_node.left.as_ref().map(|node| &**node)));
        debug_assert!(_node.is_aliased(_node.right.as_ref().map(|node| &**node)));

        Self { root: Some(node), }
    }

    //  Internal;  returns the value contained within.
    fn node_into_inner(node: QuarterNodePtr<'brand, T>, token: &mut GhostToken<'brand>) -> T {
        let full = Self::node_into_full(node, token);

        Self::full_into_inner(full)
    }

    //  Internal; returns the full pointer.
    fn node_into_full(node: QuarterNodePtr<'brand, T>, token: &mut GhostToken<'brand>) -> FullNodePtr<'brand, T> {
        let left = node.borrow_mut(token).left.take().expect("Left child - pointing to self");
        let right = node.borrow_mut(token).right.take().expect("Right child - pointing to self");
        let tripod = node.borrow_mut(token).tripod.take().expect("Tripod - pointing to self");

        let main = HalfNodePtr::join(node, tripod);
        let children = HalfNodePtr::join(left, right);

        FullNodePtr::join(main, children)
    }

    //  Internal; returns the value contained within.
    fn full_into_inner(full: FullNodePtr<'brand, T>) -> T {
        let ghost_cell = FullNodePtr::into_inner(full);
        let node = GhostNode::into_inner(ghost_cell);

        //  If the node still has a prev and next, they are leaked.
        debug_assert!(node.up.is_none());
        debug_assert!(node.left.is_none());
        debug_assert!(node.right.is_none());
        debug_assert!(node.tripod.replace(None).is_none());

        node.value
    }
}

#[cfg(feature = "experimental-ghost-cursor")]
impl<'brand, T> TripodTree<'brand, T> {
    /// Returns a mutable reference to the front element, if any.
    ///
    /// #   Complexity
    ///
    /// -   Time: O(log N) in the number of elements.
    /// -   Space: O(1).
    pub fn front_mut<'a>(&'a mut self, token: &'a mut GhostToken<'brand>) -> Option<&'a mut T> {
        let root = self.root.as_ref()?;

        let mut cursor = GhostCursor::new(token, Some(root));
        while let Ok(_) = cursor.move_mut(Node::left) {}

        cursor.into_inner().map(|node| &mut node.value)
    }

    /// Returns a mutable reference to the back element, if any.
    ///
    /// #   Complexity
    ///
    /// -   Time: O(log N) in the number of elements.
    /// -   Space: O(1).
    pub fn back_mut<'a>(&'a mut self, token: &'a mut GhostToken<'brand>) -> Option<&'a mut T> {
        let root = self.root.as_ref()?;

        let mut cursor = GhostCursor::new(token, Some(root));
        while let Ok(_) = cursor.move_mut(Node::right) {}

        cursor.into_inner().map(|node| &mut node.value)
    }

    /// Returns a mutable reference to the element at the given index, if any.
    ///
    /// #   Complexity
    ///
    /// -   Time: O(log N) in the number of elements.
    /// -   Space: O(1).
    pub fn at_mut<'a>(&'a mut self, mut at: usize, token: &'a mut GhostToken<'brand>) -> Option<&'a mut T> {
        use cmp::Ordering::*;

        if at >= self.len(token) {
            return None;
        }

        let root = self.root.as_ref()?;

        let mut cursor = GhostCursor::new(token, Some(root));

        loop {
            let index = cursor.borrow().map(|node| node.index(cursor.token())).unwrap_or(0);

            match at.cmp(&index) {
                Less => cursor.move_mut(Node::left),
                Equal => break,
                Greater => {
                    at = at - index - 1;
                    cursor.move_mut(Node::right)
                },
            }.expect("Successful move!");
        }

        cursor.into_inner().map(|node| &mut node.value)
    }

}

impl<'brand, T> Default for TripodTree<'brand, T> {
    fn default() -> Self { Self::new() }
}

/// The side of a child.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Side {
    /// The left-side child, all elements on the left-side are "before" the parent node.
    Left,
    /// The right-side child, all elements on the right-side are "after" the parent node.
    Right,
}

impl Side {
    /// The opposite side.
    pub fn opposite(self) -> Side {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
        }
    }
}

//
//  Implementation
//

struct Node<'brand, T> {
    //  The size of the subtree rooted at this node.
    size: usize,
    value: T,
    up: Option<QuarterNodePtr<'brand, T>>,
    left: Option<QuarterNodePtr<'brand, T>>,
    right: Option<QuarterNodePtr<'brand, T>>,
    tripod: Cell<Option<QuarterNodePtr<'brand, T>>>,
}

impl<'brand, T> Node<'brand, T> {
    //  Internal; gives the index of the node in the sub-tree rooted at the node.
    //
    //  Note: this is the size of the its left sub-tree.
    fn index(&self, token: &GhostToken<'brand>) -> usize {
        self.left_size(token)
    }

    //  Internal; returns the size of the left-subtree.
    fn left_size(&self, token: &GhostToken<'brand>) -> usize {
        self.left().map(|node| node.borrow(token).size).unwrap_or(0)
    }

    //  Internal; returns the size of the right-subtree.
    fn right_size(&self, token: &GhostToken<'brand>) -> usize {
        self.right().map(|node| node.borrow(token).size).unwrap_or(0)
    }

    //  Internal; returns a reference to the right node, if any.
    fn child_size(&self, side: Side, token: &GhostToken<'brand>) -> usize {
        //  In practice, the child is not, typically, empty, although this property can be violated during manipulations.
        self.child(side).map(|node| node.borrow(token).size).unwrap_or(0)
    }

    //  Internal; checks whether a referecen to a node is aliased to another.
    fn is_aliased(&self, node: Option<&GhostNode<'brand, T>>) -> bool {
        node.map(|node| self as *const _ as *const u8 == node as *const _ as *const u8).unwrap_or(false)
    }

    //  Internal; retunrns whether hte node is a child, and on which side.
    fn is_child(&self, token: &GhostToken<'brand>) -> Option<Side> {
        self.up.as_ref().and_then(|parent| self.is_child_of(parent.borrow(token)))
    }

    //  Internal; returns whether the node is a child, and which.
    fn is_child_of(&self, candidate: &Self) -> Option<Side> {
        if self.is_aliased(candidate.left()) {
            Some(Side::Left)
        } else if self.is_aliased(candidate.right()) {
            Some(Side::Right)
        } else {
            None
        }
    }

    //  Internal; returns a reference to the up node, if any.
    fn up(&self) -> Option<&GhostNode<'brand, T>> {
        let result = self.up.as_ref().map(|node| &**node);
        debug_assert!(!self.is_aliased(result), "self.up never aliases itself");
        result
    }

    //  Internal; returns a reference to the left node, if any.
    fn left(&self) -> Option<&GhostNode<'brand, T>> {
        //  In practice, the `self.left` is not, typically, empty, although this property can be violated during manipulations.
        let result = self.left.as_ref().map(|node| &**node);
        if self.is_aliased(result) { None } else { result }
    }

    //  Internal; returns a reference to the right node, if any.
    fn right(&self) -> Option<&GhostNode<'brand, T>> {
        //  In practice, the `self.right` is not, typically, empty, although this property can be violated during manipulations.
        let result = self.right.as_ref().map(|node| &**node);
        if self.is_aliased(result) { None } else { result }
    }

    //  Internal; returns a reference to the right node, if any.
    fn child(&self, side: Side) -> Option<&GhostNode<'brand, T>> {
        //  In practice, the child is not, typically, empty, although this property can be violated during manipulations.
        let result = self.child_ref(side).as_ref().map(|node| &**node);
        if self.is_aliased(result) { None } else { result }
    }

    //  Internal; replaces the appropriate child.
    fn replace_child(&mut self, side: Side, new: QuarterNodePtr<'brand, T>) -> Option<QuarterNodePtr<'brand, T>> {
        self.child_mut(side).replace(new)
    }

    //  Internal; sets the appropriate side. Panics if already set.
    fn set_child(&mut self, side: Side, new: QuarterNodePtr<'brand, T>) {
        let previous = self.replace_child(side, new);
        debug_assert!(previous.is_none(), "{:?} already set!", side);
    }

    //  Internal; takes the appropriate side, if a child.
    fn take_child(&mut self, side: Side) -> Option<QuarterNodePtr<'brand, T>> {
        if let Some(_) = self.child(side) {
            self.child_mut(side).take()
        } else {
            None
        }
    }

    //  Internal; returns a reference to the appropriate side.
    fn child_ref(&self, side: Side) -> &Option<QuarterNodePtr<'brand, T>> {
        match side {
            Side::Left => &self.left,
            Side::Right => &self.right,
        }
    }

    //  Internal; returns a mutable reference to the appropriate side.
    fn child_mut(&mut self, side: Side) -> &mut Option<QuarterNodePtr<'brand, T>> {
        match side {
            Side::Left => &mut self.left,
            Side::Right => &mut self.right,
        }
    }

    //  Internal; deploys the tripod.
    fn deploy(&self) -> QuarterNodePtr<'brand, T> { self.tripod.take().expect("Tripod not to be None") }

    //  Internal; retracts the tripod.
    fn retract(&self, tripod: QuarterNodePtr<'brand, T>) {
        let previous = self.tripod.replace(Some(tripod));
        debug_assert!(previous.is_none());
    }
}

fn retract<'brand, T>(tripod: QuarterNodePtr<'brand, T>, token: &mut GhostToken<'brand>) {
    let previous = static_rc::lift_with_mut(Some(tripod), token, |tripod, token| {
        tripod.as_ref().expect("Some").borrow_mut(token).tripod.get_mut()
    });
    debug_assert!(previous.is_none(), "Node should not have any tripod to retract it!");
}

type GhostNode<'brand, T> = GhostCell<'brand, Node<'brand, T>>;

type QuarterNodePtr<'brand, T> = StaticRc<GhostNode<'brand, T>, 1, 4>;
type HalfNodePtr<'brand, T> = StaticRc<GhostNode<'brand, T>, 2, 4>;
type FullNodePtr<'brand, T> = StaticRc<GhostNode<'brand, T>, 4, 4>;

#[cfg(test)]
mod tests {

use std::panic::{self, AssertUnwindSafe};

use super::*;

#[track_caller]
pub(super) fn assert_tree(expected: &[&str], cursor: Cursor<'_, '_, String>) {
    let flat = flatten(cursor);

    assert_eq!(expected, flat);
}

#[track_caller]
fn assert_element(expected: Option<&str>, actual: Option<&String>) {
    assert_eq!(expected, actual.map(String::as_str));
}

#[cfg(feature = "experimental-ghost-cursor")]
#[track_caller]
fn assert_element_mut(expected: Option<&str>, actual: Option<&mut String>) {
    assert_eq!(expected, actual.map(|s| &**s));
}

#[test]
fn tree_new() {
    with_tree(&[][..], |token, tree| {
        assert_tree(&[][..], tree.cursor(token));
    });
}

#[test]
fn tree_test() {
    let sample = ["Root", "Left", "Right", "LL", "LR", "RL", "RR"];

    with_tree(&sample[..], |token, tree| {
        assert_tree(&sample[..], tree.cursor(token));
    });

    let holes = ["Root", "Left", "Right", "-", "LR", "RL"];

    with_tree(&holes[..], |token, tree| {
        assert_tree(&holes[..], tree.cursor(token));
    });
}

#[test]
fn tree_access() {
    const TREE: &[&str] = &["4", "2", "6", "1", "3", "5", "7"];

    with_tree(TREE, |token, tree| {
        assert_element(Some("1"), tree.front(token));
        assert_element(Some("7"), tree.back(token));

        assert_element(Some("1"), tree.at(0, token));
        assert_element(Some("2"), tree.at(1, token));
        assert_element(Some("3"), tree.at(2, token));
        assert_element(Some("4"), tree.at(3, token));
        assert_element(Some("5"), tree.at(4, token));
        assert_element(Some("6"), tree.at(5, token));
        assert_element(Some("7"), tree.at(6, token));
        assert_element(None, tree.at(7, token));
    });
}

#[cfg(feature = "experimental-ghost-cursor")]
#[test]
fn tree_access_mut() {
    const TREE: &[&str] = &["4", "2", "6", "1", "3", "5", "7"];

    with_tree(TREE, |token, tree| {
        assert_element_mut(Some("1"), tree.front_mut(token));
        assert_element_mut(Some("7"), tree.back_mut(token));

        assert_element_mut(Some("1"), tree.at_mut(0, token));
        assert_element_mut(Some("2"), tree.at_mut(1, token));
        assert_element_mut(Some("3"), tree.at_mut(2, token));
        assert_element_mut(Some("4"), tree.at_mut(3, token));
        assert_element_mut(Some("5"), tree.at_mut(4, token));
        assert_element_mut(Some("6"), tree.at_mut(5, token));
        assert_element_mut(Some("7"), tree.at_mut(6, token));
        assert_element_mut(None, tree.at_mut(7, token));
    });
}

#[test]
fn tree_pop_push() {
    const TREE: &[&str] = &["4", "2", "6", "1", "3", "5", "7"];

    with_tree(TREE, |token, tree| {
        assert_tree(TREE, tree.cursor(token));

        assert_eq!(Some("1".to_string()), tree.pop_front(token));
        assert_tree(&["4", "2", "6", "-", "3", "5", "7"], tree.cursor(token));

        assert_eq!(Some("7".to_string()), tree.pop_back(token));
        assert_tree(&["4", "2", "6", "-", "3", "5"], tree.cursor(token));

        tree.push_front("1".to_string(), token);
        assert_tree(&["4", "2", "6", "1", "3", "5"], tree.cursor(token));

        tree.push_back("7".to_string(), token);
        assert_tree(TREE, tree.cursor(token));
    });
}

#[test]
fn tree_append() {
    const ORIGINAL: &[&str] = &["D", "B", "F", "A", "C", "E", "G"];
    const SPLICE: &[&str] = &["4", "2", "6", "1", "3", "5", "7"];

    with_tree_duo(ORIGINAL, SPLICE, |token, tree, splice| {
        tree.append(splice, token);

        //         G
        //     D       4
        //   B   F   2   6
        //  A C E - 1 3 5 7
        assert_tree(&["G", "D", "4", "B", "F", "2", "6", "A", "C", "E", "-", "1", "3", "5", "7"], tree.cursor(token));
        assert_tree(&[], splice.cursor(token));
    });
}

#[test]
fn tree_prepend() {
    const ORIGINAL: &[&str] = &["D", "B", "F", "A", "C", "E", "G"];
    const SPLICE: &[&str] = &["4", "2", "6", "1", "3", "5", "7"];

    with_tree_duo(ORIGINAL, SPLICE, |token, tree, splice| {
        tree.prepend(splice, token);

        //         A
        //     4       D
        //   2   6   B   F
        //  1 3 5 7 - C E G
        assert_tree(&["A", "4", "D", "2", "6", "B", "F", "1", "3", "5", "7", "-", "C", "E", "G"], tree.cursor(token));
        assert_tree(&[], splice.cursor(token));
    });
}

#[test]
fn tree_split_off() {
    const ORIGINAL: &[&str] = &["8", "4", "C", "2", "6", "A", "E", "1", "3", "5", "7", "9", "B", "D", "F"];

    with_tree_duo(ORIGINAL, &[], |token, tree, split| {
        *split = tree.split_off(3, token);

        assert_tree(&["2", "1", "3"], tree.cursor(token));
        //         8
        //     6       C
        //   4   7   A   E
        //  - 5 - - 9 B D F
        assert_tree(&["8", "6", "C", "4", "7", "A", "E", "-", "5", "-", "-", "9", "B", "D", "F"], split.cursor(token));
    });
}

#[test]
fn tree_split() {
    const ORIGINAL: &[&str] = &["8", "4", "C", "2", "6", "A", "E", "1", "3", "5", "7", "9", "B", "D", "F"];

    with_tree_duo(ORIGINAL, &[], |token, tree, split| {
        eprintln!("===== Split Across Root =====");

        const RANGE: Range<usize> = 3..11;

        *split = tree.split(RANGE, token);

        //     C
        //   2   E
        //  1 3 D F
        assert_tree(&["C", "2", "E", "1", "3", "D", "F"], tree.cursor(token));
        //         8
        //     6       A
        //   4   7   9   B
        //  - 5 - - - - - -
        assert_tree(&["8", "6", "A", "4", "7", "9", "B", "-", "5"], split.cursor(token));

        assert_eq!(RANGE.count(), split.len(token));
    });

    with_tree_duo(ORIGINAL, &[], |token, tree, split| {
        eprintln!("===== Split Left Sub-Tree =====");

        const RANGE: Range<usize> = 3..6;

        *split = tree.split(RANGE, token);

        //         8
        //     2       C
        //   1   3   A   E
        //  - - - 7 9 B D F
        assert_tree(&["8", "2", "C", "1", "3", "A", "E", "-", "-", "-", "7", "9", "B", "D", "F"], tree.cursor(token));
        //   5
        //  4 6
        assert_tree(&["5", "4", "6"], split.cursor(token));

        assert_eq!(RANGE.count(), split.len(token));
    });

    with_tree_duo(ORIGINAL, &[], |token, tree, split| {
        eprintln!("===== Split Right Sub-Tree =====");

        const RANGE: Range<usize> = 8..12;

        *split = tree.split(RANGE, token);

        //         4
        //     2       8
        //   1   3   6   E
        //  - - - - 5 7 D F
        assert_tree(&["4", "2", "8", "1", "3", "6", "E", "-", "-", "-", "-", "5", "7", "D", "F"], tree.cursor(token));
        //     A
        //   9   C
        //  - - B -
        assert_tree(&["A", "9", "C", "-", "-", "B"], split.cursor(token));

        assert_eq!(RANGE.count(), split.len(token));
    });
}

pub(super) fn with_tree<R, F>(flat: &[&str], fun: F) -> R
where
    F: for<'brand> FnOnce(&mut GhostToken<'brand>, &mut TripodTree<'brand, String>) -> R,
{
    GhostToken::new(|mut token| {
        let mut tree = inflate(flat, &mut token);

        let result = panic::catch_unwind(AssertUnwindSafe(|| fun(&mut token, &mut tree)));

        tree.clear(&mut token);

        result.expect("No Panic")
    })
}

pub(super) fn with_tree_duo<R, F>(first: &[&str], second: &[&str], fun: F) -> R
where
    F: for<'brand> FnOnce(&mut GhostToken<'brand>, &mut TripodTree<'brand, String>, &mut TripodTree<'brand, String>) -> R,
{
    GhostToken::new(|mut token| {
        let mut first = inflate(first, &mut token);
        let mut second = inflate(second, &mut token);

        let result = panic::catch_unwind(AssertUnwindSafe(|| fun(&mut token, &mut first, &mut second)));

        first.clear(&mut token);
        second.clear(&mut token);

        result.expect("No Panic")
    })
}

pub(super) fn inflate<'brand>(flat: &[&str], token: &mut GhostToken<'brand>) -> TripodTree<'brand, String> {
    fn set_child<'brand>(
        node: &QuarterNodePtr<'brand, String>,
        side: Side,
        child: QuarterNodePtr<'brand, String>,
        token: &mut GhostToken<'brand>)
    {
        let child_tripod = child.borrow(token).deploy();
        let child_size = child_tripod.borrow(token).size;

        let current = node.borrow_mut(token).replace_child(side, child);
        node.borrow_mut(token).size += child_size;

        child_tripod.borrow_mut(token).up = current;

        super::retract(child_tripod, token);
    }

    fn inflate_impl<'brand>(index: usize, flat: &[&str], token: &mut GhostToken<'brand>) -> Option<QuarterNodePtr<'brand, String>> {
        if index >= flat.len() || flat[index].is_empty() || flat[index] == "-" {
            return None;
        }

        let node = TripodTree::from_value(flat[index].to_string(), token);

        if let Some(left) = inflate_impl(left_child_index(index), flat, token) {
            set_child(&node, Side::Left, left, token);
        }
        if let Some(right) = inflate_impl(right_child_index(index), flat, token) {
            set_child(&node, Side::Right, right, token);
        }

        Some(node)
    }

    let mut tree = TripodTree::new();

    tree.root = inflate_impl(0, flat, token);

    tree
}

pub(super) fn flatten(mut cursor: Cursor<'_, '_, String>) -> Vec<String> {
    fn set(element: String, index: usize, flat: &mut Vec<String>) {
        if index >= flat.len() {
            flat.resize(index + 1, "-".to_string());
        }

        flat[index] = element;
    }

    fn flatten_impl(cursor: Cursor<'_, '_, String>, index: usize, flat: &mut Vec<String>) {
        let size = if let Some(current) = cursor.current() {
            set(current.clone(), index, flat);
            cursor.range().len()
        } else {
            return;
        };

        let left_size = {
            let mut clone = cursor;
            clone.move_left();
            flatten_impl(clone, left_child_index(index), flat);
            clone.range().len()
        };

        let right_size = {
            let mut clone = cursor;
            clone.move_right();
            flatten_impl(clone, right_child_index(index), flat);
            clone.range().len()
        };

        assert_eq!(
            size, 1 + left_size + right_size,
            "{} (at {:?}) != 1 + {} (at {:?}) + {} (at {:?})",
            size, cursor.current(),
            left_size, cursor.peek_left(),
            right_size, cursor.peek_right()
        );
    }

    cursor.move_to_root();

    let mut flat = vec!();
    flatten_impl(cursor, 0, &mut flat);

    flat
}

fn left_child_index(index: usize) -> usize { 2 * index + 1 }

fn right_child_index(index: usize) -> usize { 2 * index + 2 }

} // mod tests
