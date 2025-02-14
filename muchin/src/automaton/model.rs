use super::{
    Action, ActionDebugInfo, AnyAction, Dispatcher, ModelState, SerializableAction, State,
};
use bincode::{deserialize_from, serialize_into};
use colored::Colorize;
use log::debug;
use serde::{Deserialize, Serialize};
use std::{
    any::Any,
    fs::File,
    io::{BufReader, BufWriter},
};

/// The following code enables polymorphic handling of different *model* types
/// and their actions in a state-machine setup. A *model* is defined in terms of
/// the type of actions it can process, and uses dynamic dispatch to call the
/// correct processing method.
///
/// A model implements one of the `PureModel`, and `EffectuflModel` traits.
/// Each trait defines a specific `Action` associated type, and a corresponding
/// processing method (`process_pure` or `process_effectful`).
///
/// Models implementing the `PureModel` or `EffectfulModel` traits are wrapped
/// by the `Pure` and `effectful` structs. These are used to implement the
/// `PrivateModel` trait that provides `into_vtable*` methods so models can be
/// converted into the `AnyModel` type.
///
/// Finally, the `AnyModel` type is a central struct that can hold any model and
/// handles actions via its virtual method table. It provides the methods
/// (`process_pure`, `process_effectful`) to process different kinds of actions.
pub struct AnyModel<Substates: ModelState> {
    model: Box<dyn Any>,
    vtable: ModelVTable<Substates>,
}

impl<Substates: ModelState> AnyModel<Substates> {
    pub fn process_pure(
        &mut self,
        state: &mut State<Substates>,
        action: AnyAction,
        dispatcher: &mut Dispatcher,
    ) {
        (self.vtable.process_pure)(state, action, dispatcher)
    }

    pub fn process_effectful(&mut self, action: AnyAction, dispatcher: &mut Dispatcher) {
        (self.vtable.process_effectful)(&mut self.model, action, dispatcher)
    }

    pub fn serialize_into(&mut self, writer: &mut BufWriter<File>, action: &AnyAction) {
        (self.vtable.serialize_into)(writer, action)
    }

    pub fn deserialize_from(&mut self, reader: &mut BufReader<File>) -> AnyAction {
        (self.vtable.deserialize_from)(reader)
    }
}

struct ModelVTable<Substates: ModelState> {
    /// `Pure` actions can access the state-machine state only
    process_pure: fn(state: &mut State<Substates>, action: AnyAction, dispatcher: &mut Dispatcher),
    /// `Effectful` actions access the state of the `EffectfulModel` (external) state
    /// but they can't access the state-machine state
    process_effectful: fn(state: &mut Box<dyn Any>, action: AnyAction, dispatcher: &mut Dispatcher),
    serialize_into: fn(writer: &mut BufWriter<File>, action: &AnyAction),
    deserialize_from: fn(reader: &mut BufReader<File>) -> AnyAction,
}

pub trait PrivateModel
where
    Self: Sized + 'static,
{
    fn into_vtable<Substates: ModelState>(self: Box<Self>) -> AnyModel<Substates> {
        let model = self;
        let vtable = ModelVTable {
            process_pure: Self::process_pure,
            process_effectful: Self::process_effectful,
            serialize_into: Self::serialize_into,
            deserialize_from: Self::deserialize_from,
        };
        AnyModel { model, vtable }
    }

    fn into_vtable2<Substates: ModelState>() -> AnyModel<Substates> {
        let model = Box::new(()); // placeholder
        let vtable = ModelVTable {
            process_pure: Self::process_pure,
            process_effectful: Self::process_effectful,
            serialize_into: Self::serialize_into,
            deserialize_from: Self::deserialize_from,
        };
        AnyModel { model, vtable }
    }

    fn process_pure<Substates: ModelState>(
        _state: &mut State<Substates>,
        _action: AnyAction,
        _dispatcher: &mut Dispatcher,
    ) {
        unreachable!()
    }

    fn process_effectful(
        _state: &mut Box<dyn Any>,
        _action: AnyAction,
        _dispatcher: &mut Dispatcher,
    ) {
        unreachable!()
    }

    fn serialize_into(_writer: &mut BufWriter<File>, _action: &AnyAction) {
        unreachable!()
    }

    fn deserialize_from(_reader: &mut BufReader<File>) -> AnyAction {
        unreachable!()
    }
}

pub trait PureModel
where
    Self: Sized + 'static,
{
    type Action: Clone
        + Eq
        + Action
        + Serialize
        + for<'a> Deserialize<'a>
        + type_uuid::TypeUuid
        + std::fmt::Debug
        + Sized
        + 'static;

    fn process_pure<Substates: ModelState>(
        state: &mut State<Substates>,
        action: Self::Action,
        dispatcher: &mut Dispatcher,
    );
}

pub struct Pure<T: PureModel>(T);

impl<T: PureModel> PrivateModel for Pure<T> {
    fn process_pure<Substates: ModelState>(
        state: &mut State<Substates>,
        action: AnyAction,
        dispatcher: &mut Dispatcher,
    ) {
        let downcasted_action = action
            .ptr
            .downcast::<T::Action>()
            .expect("action not found");

        let ActionDebugInfo {
            location_file,
            location_line,
            depth,
            action_id,
            caller,
            callback,
        } = action.dbginfo;

        let pad = "  ".repeat(depth);

        if callback {
            debug!(
                "{}: ←{} {}::{}\n\t\t {} {}",
                state.get_current_instance(),
                pad,
                action.type_name.bright_cyan(),
                format!("{:?}", downcasted_action).bright_cyan(),
                pad,
                format!(
                    "from {}:{} ({}→{})",
                    location_file, location_line, caller, action_id
                )
                .bright_black()
            );
        } else {
            let from = if depth == 0 {
                format!("from DISPATCHER TICK ({}→{})", caller, action_id)
            } else {
                format!(
                    "from {}:{} ({}→{})",
                    location_file, location_line, caller, action_id
                )
            };

            debug!(
                "{}: →{} {}::{:?}\n\t\t {} {}",
                state.get_current_instance(),
                pad,
                action.type_name,
                downcasted_action,
                pad,
                from.bright_black()
            );
        }

        dispatcher.depth = depth;
        dispatcher.caller = action_id;
        T::process_pure(state, *downcasted_action, dispatcher)
    }

    fn serialize_into(writer: &mut BufWriter<File>, action: &AnyAction) {
        let downcasted_action = action
            .ptr
            .downcast_ref::<T::Action>()
            .expect("action not found");

        serialize_into(writer.get_mut(), &action.uuid).expect("UUID serialization failed");
        serialize_into(
            writer,
            &SerializableAction {
                action: downcasted_action.clone(),
                dbginfo: action.dbginfo.clone(),
            },
        )
        .expect("Action serialization failed");
    }

    fn deserialize_from(reader: &mut BufReader<File>) -> AnyAction {
        let uuid: type_uuid::Bytes =
            deserialize_from(reader.get_mut()).expect("UUID deserialization failed");

        debug!("Deserialized {:?}", uuid);

        let deserialized_action: SerializableAction<T::Action> =
            deserialize_from(reader).expect("Action deserialization failed");

        debug!("Deserialized {:?}", deserialized_action);

        let mut action: AnyAction = deserialized_action.action.into();
        action.dbginfo = deserialized_action.dbginfo;
        action
    }
}

pub trait EffectfulModel
where
    Self: Sized + 'static,
{
    type Action: Clone
        + Eq
        + Action
        + Serialize
        + for<'a> Deserialize<'a>
        + type_uuid::TypeUuid
        + std::fmt::Debug
        + Sized
        + 'static;

    fn process_effectful(&mut self, action: Self::Action, dispatcher: &mut Dispatcher);
}

pub struct Effectful<T: EffectfulModel>(pub T);

impl<T: EffectfulModel> PrivateModel for Effectful<T> {
    fn process_effectful(state: &mut Box<dyn Any>, action: AnyAction, dispatcher: &mut Dispatcher) {
        let state = state
            .downcast_mut::<Self>()
            .expect("model's state not found");

        let downcasted_action = action
            .ptr
            .downcast::<T::Action>()
            .expect("action not found");

        let ActionDebugInfo {
            location_file,
            location_line,
            depth,
            action_id,
            caller,
            callback: false,
        } = action.dbginfo
        else {
            panic!("Can't dispatch_back() an effectful action")
        };

        let pad = "  ".repeat(depth);
        debug!(
            "→{} {}::{}\n\t\t {} {}",
            pad,
            action.type_name.bright_yellow(),
            format!("{:?}", downcasted_action).bright_yellow(),
            pad,
            format!(
                "from {}:{} ({}→{})",
                location_file, location_line, caller, action_id
            )
            .bright_black()
        );
        dispatcher.depth = depth;
        dispatcher.caller = action_id;
        state.0.process_effectful(*downcasted_action, dispatcher)
    }

    fn serialize_into(writer: &mut BufWriter<File>, action: &AnyAction) {
        let downcasted_action = action
            .ptr
            .downcast_ref::<T::Action>()
            .expect("action not found");

        serialize_into(writer.get_mut(), &action.uuid).expect("UUID serialization failed");
        serialize_into(
            writer,
            &SerializableAction {
                action: downcasted_action.clone(),
                dbginfo: action.dbginfo.clone(),
            },
        )
        .expect("Action serialization failed");
    }

    fn deserialize_from(reader: &mut BufReader<File>) -> AnyAction {
        let uuid: type_uuid::Bytes =
            deserialize_from(reader.get_mut()).expect("UUID deserialization failed");

        debug!("Deserialized {:?}", uuid);

        let deserialized_action: SerializableAction<T::Action> =
            deserialize_from(reader).expect("Action deserialization failed");

        debug!("Deserialized {:?}", deserialized_action);

        let mut action: AnyAction = deserialized_action.action.into();
        action.dbginfo = deserialized_action.dbginfo;
        action
    }
}
