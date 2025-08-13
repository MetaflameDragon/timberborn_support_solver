use std::{
    cmp::Ordering,
    fmt::{Debug, Formatter},
    hash::{Hash, Hasher},
    marker::PhantomData,
};

use petgraph::adj::{DefaultIx, IndexType};

// Might be useful to have a typed index? delete if not needed maybe
pub struct TypedIx<T, Ix = DefaultIx>(Ix, PhantomData<T>);

impl<T, Ix: Clone> Clone for TypedIx<T, Ix> {
    fn clone(&self) -> Self {
        Self(self.0.clone(), self.1)
    }
}

impl<T, Ix: Copy> Copy for TypedIx<T, Ix> {}

impl<T, Ix: Default> Default for TypedIx<T, Ix> {
    fn default() -> Self {
        Self(Ix::default(), Default::default())
    }
}

impl<T, Ix: Hash> Hash for TypedIx<T, Ix> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl<T, Ix: Ord> Ord for TypedIx<T, Ix> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.cmp(&other.0)
    }
}

impl<T, Ix: PartialOrd> PartialOrd for TypedIx<T, Ix> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.0.partial_cmp(&other.0)
    }
}

impl<T, Ix: PartialEq> PartialEq for TypedIx<T, Ix> {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq(&other.0)
    }
}

impl<T, Ix: Eq> Eq for TypedIx<T, Ix> {}

impl<T, Ix: Debug> Debug for TypedIx<T, Ix> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "TypedIx<{}>({:?})", std::any::type_name::<T>(), self.0)
    }
}

unsafe impl<Ix: IndexType, T: 'static> IndexType for TypedIx<T, Ix> {
    fn new(x: usize) -> Self {
        Self(Ix::new(x), Default::default())
    }

    fn index(&self) -> usize {
        self.0.index()
    }

    fn max() -> Self {
        Self(<Ix as IndexType>::max(), Default::default())
    }
}
