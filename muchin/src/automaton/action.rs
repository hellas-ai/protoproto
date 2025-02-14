use linkme::distributed_slice;
use serde::{Deserialize, Serialize};

use std::{
    any::{Any, TypeId},
    borrow::Cow,
    collections::VecDeque,
    fmt,
    fs::{File, OpenOptions},
    io::{BufReader, BufWriter},
    ops::Deref,
    panic::Location,
    rc::Rc,
};
use type_uuid::TypeUuidDynamic;

#[derive(PartialEq, Eq, Serialize, Deserialize, Clone, Debug)]
pub enum Timeout {
    Millis(u64),
    Never,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum TimeoutAbsolute {
    Millis(u128),
    Never,
}

pub fn serialize_rc_bytes<S>(data: &Rc<[u8]>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let vec: Vec<u8> = data.deref().to_vec();
    vec.serialize(serializer)
}

pub fn deserialize_rc_bytes<'de, D>(deserializer: D) -> Result<Rc<[u8]>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let vec: Vec<u8> = Deserialize::deserialize(deserializer)?;
    Ok(Rc::from(vec.into_boxed_slice()))
}

/// Actions fall into 2 categories:
///
/// 1. `Pure`: these are both dispatched and processed by `PureModel`s.
///    They can change the state-machine state but they don't cause any other
///    side-effects.
///
/// 2. `Effectful`: these are handled by `EffectfulModel`s to communicate to the
///    "external world". `EffectfulModel`s don't access the state-machine state
///    but they have their own (minimal) state.
#[derive(Serialize, Deserialize, Debug)]
#[repr(u8)]
pub enum ActionKind {
    Pure = 0,
    Effectful = 1,
}

pub trait Action
where
    Self: TypeUuidDynamic + fmt::Debug + 'static,
{
    const KIND: ActionKind;
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub struct ActionDebugInfo {
    pub location_file: String,
    pub location_line: u32,
    pub depth: usize,
    pub action_id: u64,
    /// action id of caller action
    pub caller: u64,
    /// Was the action dispatched with dispatch_back.
    pub callback: bool,
}

pub struct AnyAction {
    pub uuid: type_uuid::Bytes,
    pub kind: ActionKind,
    pub ptr: Box<dyn Any>,
    // For printing/debug purpose only
    pub type_name: &'static str,
    pub dbginfo: ActionDebugInfo,
}

impl<T: Action> From<T> for AnyAction {
    fn from(v: T) -> Self {
        Self {
            uuid: v.uuid(),
            kind: T::KIND,
            ptr: Box::new(v),
            type_name: std::any::type_name::<T>(),
            dbginfo: ActionDebugInfo {
                location_file: String::new(),
                location_line: 0,
                depth: 0,
                action_id: 0,
                caller: 0,
                callback: false,
            },
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SerializableAction<T: Clone + type_uuid::TypeUuid + std::fmt::Debug + Sized + 'static> {
    pub action: T,
    pub dbginfo: ActionDebugInfo,
}

#[derive(PartialEq, Eq, Clone, Serialize, Deserialize)]
pub struct Redispatch<R> {
    #[serde(skip)]
    fun_ptr: Option<fn(R) -> AnyAction>,
    pub fun_name: Cow<'static, str>,
}

impl<R: 'static> Redispatch<R> {
    pub fn new(name: &'static str, ptr: fn(R) -> AnyAction) -> Self {
        Self {
            fun_ptr: Some(ptr),
            fun_name: Cow::Borrowed(name),
        }
    }

    pub fn make(&self, result: R) -> AnyAction {
        if let Some(fun) = self.fun_ptr {
            return fun(result);
        }

        // We reach this point only when `Redispatch` was deserialized
        for (name, fun) in CALLBACKS {
            if name == &self.fun_name {
                return fun(std::any::type_name::<R>(), Box::new(result));
            }
        }

        panic!("callback function {} not found", self.fun_name)
    }
}

impl<R> fmt::Debug for Redispatch<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "...")
    }
}

pub struct Dispatcher {
    queue: VecDeque<AnyAction>,
    halt: bool,

    /// This is a caller-defined function that produces and dispatches an action
    /// when the action queue is empty. To the state-machine, the "tick" action is
    /// analogous to the clock-cycle of a CPU.
    tick: fn() -> AnyAction,

    // The following fields are for debugging purposes:
    /// The nesting level into the action chain: if action `A` has `depth=1` and
    /// its handler dispatches `B`,then `B` has `depth=2`.
    pub depth: usize,
    /// Every dispatched action has an unique `action_id` for the lifetime of
    /// the state-machine. In combination with an action's `caller` we can
    /// reconstruct the flow graph of all actions.
    pub action_id: u64,
    /// The action's `caller` is the `action_id` of the action that was being
    /// handled at the moment the current action was dispatched.
    pub caller: u64,

    /// Recording output file
    pub record_file: Option<BufWriter<File>>,
    /// Replay input file
    pub replay_file: Option<BufReader<File>>,
}

pub struct IfPure<const K: u8>;
pub trait True {}
impl True for IfPure<0> {}

pub trait False {}
impl False for IfPure<1> {}

impl Dispatcher {
    pub fn new(tick: fn() -> AnyAction) -> Self {
        Self {
            queue: VecDeque::with_capacity(1024),
            halt: false,
            tick,
            depth: 0,
            action_id: 0,
            caller: 0,
            record_file: None,
            replay_file: None,
        }
    }

    pub fn halt(&mut self) {
        self.halt = true;
    }

    pub fn unhalt(&mut self) {
        self.halt = false;
    }

    pub fn is_halted(&self) -> bool {
        self.halt
    }

    pub fn next_action(&mut self) -> AnyAction {
        self.queue.pop_front().unwrap_or_else(|| {
            let mut any_action = (self.tick)();

            any_action.dbginfo.action_id = self.action_id;
            any_action.dbginfo.caller = 0;
            self.depth = 0;
            self.action_id += 1;
            self.caller = 0;
            any_action
        })
    }

    pub fn record(&mut self, filename: &str) {
        assert!(self.record_file.is_none());
        self.record_file = Some(BufWriter::new(
            OpenOptions::new()
                .create(true)
                .write(true)
                .append(false)
                .open(filename)
                .expect(&format!("Recorder: failed to open file: {}", filename)),
        ));
    }

    pub fn open_recording(&mut self, filename: &str) {
        assert!(self.replay_file.is_none());
        self.replay_file = Some(BufReader::new(
            File::open(filename).expect(&format!("Replayer: failed to open file: {}", filename)),
        ));
    }

    pub fn is_replayer(&self) -> bool {
        self.replay_file.is_some()
    }

    #[track_caller]
    pub fn dispatch<A: Action>(&mut self, action: A)
    where
        A: Sized + 'static,
        IfPure<{ A::KIND as u8 }>: True,
    {
        let location = Location::caller();
        self.dispatch_common(action, *location)
    }

    #[track_caller]
    pub fn dispatch_effect<A: Action>(&mut self, action: A)
    where
        A: Sized + 'static,
        IfPure<{ A::KIND as u8 }>: False,
    {
        let location = Location::caller();
        self.dispatch_common(action, *location)
    }

    fn dispatch_common<A: Action>(&mut self, action: A, location: Location)
    where
        A: Sized + 'static,
    {
        assert_ne!(TypeId::of::<A>(), TypeId::of::<AnyAction>());
        let mut any_action: AnyAction = action.into();

        any_action.dbginfo = ActionDebugInfo {
            location_file: location.file().to_string(),
            location_line: location.line(),
            depth: self.depth + 1,
            action_id: self.action_id,
            caller: self.caller,
            callback: false,
        };
        self.action_id += 1;
        self.queue.push_back(any_action);
    }

    #[track_caller]
    pub fn dispatch_back<R: Clone>(&mut self, on_result: &Redispatch<R>, result: R)
    where
        R: Sized + 'static,
    {
        let location = Location::caller();
        let mut any_action = on_result.make(result);

        any_action.dbginfo = ActionDebugInfo {
            location_file: location.file().to_string(),
            location_line: location.line(),
            depth: self.depth.saturating_sub(1),
            action_id: self.action_id,
            caller: self.caller,
            callback: true,
        };
        self.action_id += 1;
        self.queue.push_back(any_action);
    }
}

#[distributed_slice]
pub static CALLBACKS: [(&str, fn(&str, Box<dyn Any>) -> AnyAction)];

#[macro_export]
macro_rules! _callback {
    ($gensym:ident, $arg:tt, $arg_type:ty, $body:expr) => {{
        use crate::automaton::Redispatch;
        use crate::automaton::{AnyAction, CALLBACKS};
        use linkme::distributed_slice;

        paste::paste! {
            #[allow(unused)] // $arg is marked as unused, but it's used in `$body`
            fn convert_impl($arg: $arg_type) -> AnyAction {
                ($body).into()
            }

            fn $gensym(call_type: &str, args: Box<dyn std::any::Any>) -> AnyAction {
                #[distributed_slice(CALLBACKS)]
                static CALLBACK_DESERIALIZE: (&str, fn(&str, Box<dyn std::any::Any>) -> AnyAction) = (
                    stringify!($gensym),
                    $gensym,
                );

                let $arg = *args.downcast::<$arg_type>()
                    .expect(&format!(
                        "Invalid argument type: {}, expected: {}",
                        call_type,
                        stringify!($arg_type)));

                convert_impl($arg)
            }
        }

        Redispatch::<$arg_type>::new(stringify!($gensym), convert_impl)
    }};
}

#[macro_export]
macro_rules! callback {
    (|($($var:ident : $typ:ty),+)| $body:expr) => {
        gensym::gensym! { crate::_callback!(($($var),+), ($($typ),+), $body) }
    };
    (|$var:ident : $typ:ty| $body:expr) => {
        gensym::gensym! { crate::_callback!($var, $typ, $body) }
    };
}
