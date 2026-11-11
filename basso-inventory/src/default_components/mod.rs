pub mod generated;

use basso_registry::builtin::ItemKind;


/// A trait for data components that some [`ItemKind`]s may have a default value
/// for.
///
/// Every component implements it (see `generated.rs`); the ones without
/// defaults keep this method's `None`.
pub trait DefaultableComponent: Sized {
    fn default_for_item(_item: ItemKind) -> Option<Self> {
        None
    }
}
pub fn get_default_component<T: DefaultableComponent>(item: ItemKind) -> Option<T> {
    T::default_for_item(item)
}
