use serde::{Deserialize, Serialize};
use std::{any::Any, collections::BTreeMap};

/// `Uid` serves as a unique identifier (UID) for referencing objects across
/// different Models.
///
/// Although it is theoretically possible to re-use a Uid value once the object
/// it was originally associated with gets freed, this optimization is usually
/// not necessary. Instead, we use a 64-bit counter, which, while capable of
/// wrapping around, is practically unlikely to overflow within the program's
/// lifetime, thus providing unique values.
#[derive(PartialEq, PartialOrd, Eq, Ord, Clone, Copy, Serialize, Deserialize, Debug)]
pub struct Uid(u64);

impl Default for Uid {
    fn default() -> Self {
        Uid(0)
    }
}

/// Conversion implementations for `Uid` to `usize` and vice versa.
/// Safe cast as `usize` is at least 64 bits on 64-bit platforms.
impl From<Uid> for usize {
    fn from(item: Uid) -> usize {
        item.0 as usize
    }
}

impl From<usize> for Uid {
    fn from(item: usize) -> Self {
        Uid(item as u64)
    }
}

/// Conversion implementations for `Uid` to `u64` and vice versa.
impl From<Uid> for u64 {
    fn from(item: Uid) -> u64 {
        item.0
    }
}

impl From<u64> for Uid {
    fn from(item: u64) -> Self {
        Uid(item)
    }
}

impl Uid {
    /// Function to generate the next `Uid`. This increments the internal
    /// counter and returns the new value.
    pub fn next(&mut self) -> Uid {
        let ret = Uid(self.0);
        self.0 = self.0.wrapping_add(1);
        assert_ne!(self.0, 0);
        ret
    }
}

/// `Objects` is a type alias for a `BTreeMap` that maps `Uid`s to object
/// instances of a given type. This is used by Models to maintain a collection
/// of objects that can be uniquely identified and accessed using their `Uid`s.
pub type Objects<T> = BTreeMap<Uid, T>;

/// `State` struct encapsulates the state-machine's state.
///
/// This includes a `Uid` generator, a vector of one or more `Substates`
/// instances, and the `current_instance` field holding the index of the
/// currently active instance in the state-machine's main loop.
///
/// In most scenarios, the `Substates` vector contains a single element.
/// Multiple elements mainly occur in testing scenarios where multiple
/// instances are simulated.
pub struct State<Substates: ModelState> {
    pub uid_source: Uid,
    pub substates: Vec<Substates>,
    current_instance: usize,
}

/// The `ModelState` trait provides an interface for Models to access their own
/// state or the states of their dependencies (other models) from the main
/// `State`. This trait relies on runtime type information (RTTI) to correctly
/// identify and return a reference to the desired field in Substates.
///
/// Models are defined in terms of their own state type, and each Model's state
/// is a field in the `Substates` struct, which is defined by the top-most Model.
/// This struct must contain one field for every Model dependency (for
/// Pure/InputModels only).
///
/// The `ModelState` trait might seem a bit complicated, but it's a crucial
/// part of this state-machine setup. It provides flexibility and helps keep
/// things organized. In simple terms, it allows different parts of our program
/// (the models) to access and manage their own specific data (state) within the
/// larger system (the state-machine). This trait makes it easier to add,
/// remove, or change parts of the system without disrupting the whole thing.
///
/// A derive macro is provided, which simplifies a lot of the work for us.
/// This macro automates the process of implementing the `ModelState` trait for
/// a struct. The macro iterates over all named fields. For each field, it
/// generates code that tries to downcast the field to the requested type (T).
/// If the downcast succeeds, it returns a reference (or mutable reference) to
/// the field. If the downcast fails, it proceeds to the next field.
pub trait ModelState {
    fn state<T: 'static + Any>(&self) -> &T;
    fn state_mut<T: 'static + Any>(&mut self) -> &mut T;
}

impl<Substates: ModelState> State<Substates> {
    pub fn new() -> Self {
        Self {
            uid_source: Uid::default(),
            substates: Vec::new(),
            current_instance: 0,
        }
    }

    /// Generates a new unique identifier (`Uid`).
    pub fn new_uid(&mut self) -> Uid {
        self.uid_source.next()
    }

    pub fn get_current_instance(&self) -> usize {
        self.current_instance
    }

    pub fn set_current_instance(&mut self, instance: usize) {
        self.current_instance = instance;
    }

    /// Returns a reference to the state of the currently active substate.
    pub fn substate<T: 'static + Any>(&self) -> &T {
        self.substates[self.current_instance].state()
    }

    /// Returns a mutable reference to the state of the currently active substate.
    pub fn substate_mut<T: 'static + Any>(&mut self) -> &mut T {
        self.substates[self.current_instance].state_mut()
    }
}
