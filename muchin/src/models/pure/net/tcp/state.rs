use super::action::{ConnectionEvent, Event, ListenerEvent, TcpPollEvents};
use crate::automaton::{Objects, Redispatch, Timeout, TimeoutAbsolute, Uid};
use crate::models::effectful::mio::action::MioEvent;
use core::panic;
use serde::{Deserialize, Serialize};
use std::rc::Rc;

pub trait EventUpdater {
    type Event;
    fn update_events(&mut self, uid: Uid, event: &MioEvent);
    fn events(&self) -> &Self::Event;
    fn events_mut(&mut self) -> &mut Self::Event;
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Listener {
    pub address: String,
    pub on_success: Redispatch<Uid>,
    pub on_error: Redispatch<(Uid, String)>,
    pub events: Option<ListenerEvent>,
}

impl Listener {
    pub fn new(
        address: String,
        on_success: Redispatch<Uid>,
        on_error: Redispatch<(Uid, String)>,
    ) -> Self {
        Self {
            address,
            on_success,
            on_error,
            events: None,
        }
    }
}

impl EventUpdater for Listener {
    type Event = ListenerEvent;

    fn update_events(&mut self, _uid: Uid, event: &MioEvent) {
        let new_event = match event {
            MioEvent { error: true, .. } => ListenerEvent::Error,
            MioEvent {
                read_closed,
                write_closed,
                ..
            } if *read_closed || *write_closed => ListenerEvent::Closed,
            _ => ListenerEvent::AcceptPending,
        };

        self.events = self
            .events
            .take()
            .map_or(Some(new_event.clone()), |curr_event| match curr_event {
                ListenerEvent::Closed | ListenerEvent::Error => Some(curr_event),
                ListenerEvent::AcceptPending | ListenerEvent::AllAccepted => Some(new_event),
            });
    }

    fn events(&self) -> &ListenerEvent {
        self.events
            .as_ref()
            .expect("Attempt to fetch events but not initialized yet")
    }

    fn events_mut(&mut self) -> &mut ListenerEvent {
        self.events
            .as_mut()
            .expect("Attempt to fetch events but not initialized yet")
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct PollRequest {
    pub objects: Vec<Uid>,
    pub timeout: Timeout,
    pub on_success: Redispatch<(Uid, TcpPollEvents)>,
    pub on_error: Redispatch<(Uid, String)>,
}

impl PollRequest {
    pub fn new(
        objects: Vec<Uid>,
        timeout: Timeout,
        on_success: Redispatch<(Uid, TcpPollEvents)>,
        on_error: Redispatch<(Uid, String)>,
    ) -> Self {
        Self {
            objects,
            timeout,
            on_success,
            on_error,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ConnectionType {
    Incoming {
        listener: Uid,
        on_success: Redispatch<Uid>,
        on_would_block: Redispatch<Uid>,
        on_error: Redispatch<(Uid, String)>,
    },
    Outgoing {
        on_success: Redispatch<Uid>,
        on_timeout: Redispatch<Uid>,
        on_error: Redispatch<(Uid, String)>,
    },
}

impl ConnectionType {
    pub fn on_success(&self) -> &Redispatch<Uid> {
        match self {
            ConnectionType::Incoming { on_success, .. } => on_success,
            ConnectionType::Outgoing { on_success, .. } => on_success,
        }
    }

    pub fn on_error(&self) -> &Redispatch<(Uid, String)> {
        match self {
            ConnectionType::Incoming { on_error, .. } => on_error,
            ConnectionType::Outgoing { on_error, .. } => on_error,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ConnectionStatus {
    Pending,
    PendingCheck,
    Established,
    CloseRequestInternal,
    CloseRequestNotify { on_success: Redispatch<Uid> },
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Connection {
    pub status: ConnectionStatus,
    pub conn_type: ConnectionType,
    pub timeout: TimeoutAbsolute,
    pub events: Option<ConnectionEvent>,
}

impl Connection {
    pub fn new(conn_type: ConnectionType, timeout: TimeoutAbsolute) -> Self {
        let status = match conn_type {
            ConnectionType::Outgoing { .. } => ConnectionStatus::Pending,
            ConnectionType::Incoming { .. } => ConnectionStatus::Established,
        };

        Self {
            status,
            conn_type,
            timeout,
            events: None,
        }
    }
}

impl EventUpdater for Connection {
    type Event = ConnectionEvent;

    fn update_events(&mut self, _uid: Uid, event: &MioEvent) {
        let new_event = match event {
            MioEvent { error: true, .. } => ConnectionEvent::Error,
            MioEvent {
                read_closed,
                write_closed,
                ..
            } if *read_closed || *write_closed => ConnectionEvent::Closed,
            MioEvent {
                readable, writable, ..
            } => ConnectionEvent::Ready {
                can_recv: *readable,
                can_send: *writable,
            },
        };

        self.events = self
            .events
            .take()
            .map_or(Some(new_event.clone()), |curr_event| match curr_event {
                ConnectionEvent::Closed | ConnectionEvent::Error => Some(curr_event),
                ConnectionEvent::Ready {
                    can_recv: curr_recv,
                    can_send: curr_send,
                } => match new_event {
                    ConnectionEvent::Ready { can_recv, can_send } => {
                        let updated_event = ConnectionEvent::Ready {
                            can_recv: curr_recv | can_recv,
                            can_send: curr_send | can_send,
                        };
                        Some(updated_event)
                    }
                    _ => Some(new_event),
                },
            });
    }

    fn events(&self) -> &ConnectionEvent {
        self.events
            .as_ref()
            .expect("Attempt to fetch events but not initialized yet")
    }

    fn events_mut(&mut self) -> &mut ConnectionEvent {
        self.events
            .as_mut()
            .expect("Attempt to fetch events but not initialized yet")
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SendRequest {
    pub connection: Uid,
    #[serde(
        serialize_with = "crate::automaton::serialize_rc_bytes",
        deserialize_with = "crate::automaton::deserialize_rc_bytes"
    )]
    pub data: Rc<[u8]>,
    pub bytes_sent: usize,
    pub send_on_poll: bool,
    pub timeout: TimeoutAbsolute,
    pub on_success: Redispatch<Uid>,
    pub on_timeout: Redispatch<Uid>,
    pub on_error: Redispatch<(Uid, String)>,
}

impl SendRequest {
    pub fn new(
        connection: Uid,
        data: Rc<[u8]>,
        send_on_poll: bool,
        timeout: TimeoutAbsolute,
        on_success: Redispatch<Uid>,
        on_timeout: Redispatch<Uid>,
        on_error: Redispatch<(Uid, String)>,
    ) -> Self {
        Self {
            connection,
            data,
            bytes_sent: 0,
            send_on_poll,
            timeout,
            on_success,
            on_timeout,
            on_error,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RecvRequest {
    pub connection: Uid,
    pub buffered_data: Vec<u8>,
    pub remaining_bytes: usize,
    pub recv_on_poll: bool,
    pub timeout: TimeoutAbsolute,
    pub on_success: Redispatch<(Uid, Vec<u8>)>,
    pub on_timeout: Redispatch<(Uid, Vec<u8>)>,
    pub on_error: Redispatch<(Uid, String)>,
}

impl RecvRequest {
    pub fn new(
        connection: Uid,
        count: usize,
        recv_on_poll: bool,
        timeout: TimeoutAbsolute,
        on_success: Redispatch<(Uid, Vec<u8>)>,
        on_timeout: Redispatch<(Uid, Vec<u8>)>,
        on_error: Redispatch<(Uid, String)>,
    ) -> Self {
        Self {
            connection,
            buffered_data: Vec::new(),
            remaining_bytes: count,
            recv_on_poll,
            timeout,
            on_success,
            on_timeout,
            on_error,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Status {
    New,
    InitError {
        instance: Uid,
    },
    InitPollCreate {
        instance: Uid,
        poll: Uid,
        on_success: Redispatch<Uid>,
        on_error: Redispatch<(Uid, String)>,
    },
    InitEventsCreate {
        instance: Uid,
        poll: Uid,
        events: Uid,
        on_success: Redispatch<Uid>,
    },
    Ready {
        instance: Uid,
        poll: Uid,
        events: Uid,
    },
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TcpState {
    pub status: Status,
    listener_objects: Objects<Listener>,
    connection_objects: Objects<Connection>,
    poll_request_objects: Objects<PollRequest>,
    send_request_objects: Objects<SendRequest>,
    recv_request_objects: Objects<RecvRequest>,
}

impl TcpState {
    pub fn new() -> Self {
        Self {
            status: Status::New,
            listener_objects: Objects::<Listener>::new(),
            connection_objects: Objects::<Connection>::new(),
            poll_request_objects: Objects::<PollRequest>::new(),
            send_request_objects: Objects::<SendRequest>::new(),
            recv_request_objects: Objects::<RecvRequest>::new(),
        }
    }

    pub fn is_ready(&self) -> bool {
        matches!(self.status, Status::Ready { .. })
    }

    pub fn new_listener(
        &mut self,
        uid: Uid,
        address: String,
        on_success: Redispatch<Uid>,
        on_error: Redispatch<(Uid, String)>,
    ) {
        if self
            .listener_objects
            .insert(uid, Listener::new(address, on_success, on_error))
            .is_some()
        {
            panic!("Attempt to re-use existing {:?}", uid)
        }
    }

    pub fn new_poll(
        &mut self,
        uid: Uid,
        objects: Vec<Uid>,
        timeout: Timeout,
        on_success: Redispatch<(Uid, TcpPollEvents)>,
        on_error: Redispatch<(Uid, String)>,
    ) {
        assert!(objects
            .iter()
            .all(|uid| self.listener_objects.contains_key(uid)
                || self.connection_objects.contains_key(uid)));

        if self
            .poll_request_objects
            .insert(
                uid,
                PollRequest::new(objects, timeout, on_success, on_error),
            )
            .is_some()
        {
            panic!("Attempt to re-use existing {:?}", uid)
        }
    }

    pub fn new_connection(
        &mut self,
        connection: Uid,
        conn_type: ConnectionType,
        timeout: TimeoutAbsolute,
    ) {
        if self
            .connection_objects
            .insert(connection, Connection::new(conn_type, timeout))
            .is_some()
        {
            panic!("Attempt to re-use existing {:?}", connection)
        }
    }

    pub fn has_connection(&self, uid: &Uid) -> bool {
        self.connection_objects.contains_key(uid)
    }

    pub fn new_send_request(
        &mut self,
        uid: Uid,
        connection: Uid,
        data: Rc<[u8]>,
        send_on_poll: bool,
        timeout: TimeoutAbsolute,
        on_success: Redispatch<Uid>,
        on_timeout: Redispatch<Uid>,
        on_error: Redispatch<(Uid, String)>,
    ) {
        if self
            .send_request_objects
            .insert(
                uid,
                SendRequest::new(
                    connection,
                    data,
                    send_on_poll,
                    timeout,
                    on_success,
                    on_timeout,
                    on_error,
                ),
            )
            .is_some()
        {
            panic!("Attempt to re-use existing {:?}", uid)
        }
    }

    pub fn new_recv_request(
        &mut self,
        uid: Uid,
        connection: Uid,
        count: usize,
        recv_on_poll: bool,
        timeout: TimeoutAbsolute,
        on_success: Redispatch<(Uid, Vec<u8>)>,
        on_timeout: Redispatch<(Uid, Vec<u8>)>,
        on_error: Redispatch<(Uid, String)>,
    ) {
        if self
            .recv_request_objects
            .insert(
                uid,
                RecvRequest::new(
                    connection,
                    count,
                    recv_on_poll,
                    timeout,
                    on_success,
                    on_timeout,
                    on_error,
                ),
            )
            .is_some()
        {
            panic!("Attempt to re-use existing {:?}", uid)
        }
    }

    pub fn get_listener(&self, uid: &Uid) -> &Listener {
        self.listener_objects
            .get(uid)
            .expect(&format!("Listener object {:?} not found", uid))
    }

    pub fn get_listener_mut(&mut self, uid: &Uid) -> &mut Listener {
        self.listener_objects
            .get_mut(uid)
            .expect(&format!("Listener object {:?} not found", uid))
    }

    pub fn remove_listener(&mut self, uid: &Uid) {
        self.listener_objects.remove(uid).expect(&format!(
            "Attempt to remove an inexistent Listener {:?}",
            uid
        ));
    }

    pub fn get_connection(&self, uid: &Uid) -> &Connection {
        self.connection_objects
            .get(uid)
            .expect(&format!("Connection object {:?} not found", uid))
    }

    pub fn get_connection_mut(&mut self, uid: &Uid) -> &mut Connection {
        self.connection_objects
            .get_mut(uid)
            .expect(&format!("Connection object {:?} not found", uid))
    }

    pub fn remove_connection(&mut self, uid: &Uid) {
        //info!("|TCP| removing connection {:?}", uid);

        self.recv_request_objects
            .retain(|_, req| req.connection != *uid);

        self.send_request_objects
            .retain(|_, req| req.connection != *uid);

        self.connection_objects.remove(uid).expect(&format!(
            "Attempt to remove an inexistent Connection {:?}",
            uid
        ));
    }

    pub fn get_poll_request(&self, uid: &Uid) -> &PollRequest {
        self.poll_request_objects
            .get(uid)
            .expect(&format!("PollRequest object {:?} not found", uid))
    }

    pub fn remove_poll_request(&mut self, uid: &Uid) {
        self.poll_request_objects.remove(uid).expect(&format!(
            "Attempt to remove an inexistent PollRequest {:?}",
            uid
        ));
    }

    pub fn get_send_request(&self, uid: &Uid) -> &SendRequest {
        self.send_request_objects
            .get(uid)
            .expect(&format!("SendRequest object {:?} not found", uid))
    }

    pub fn get_send_request_mut(&mut self, uid: &Uid) -> &mut SendRequest {
        self.send_request_objects
            .get_mut(uid)
            .expect(&format!("SendRequest object {:?} not found", uid))
    }

    pub fn pending_send_requests(&self) -> Vec<(&Uid, &SendRequest)> {
        self.send_request_objects
            .iter()
            .filter(|(_, request)| request.send_on_poll)
            .collect()
    }

    pub fn remove_send_request(&mut self, uid: &Uid) {
        self.send_request_objects.remove(uid).expect(&format!(
            "Attempt to remove an inexistent SendRequest {:?}",
            uid
        ));
    }

    pub fn get_recv_request(&self, uid: &Uid) -> &RecvRequest {
        self.recv_request_objects
            .get(uid)
            .expect(&format!("RecvRequest object {:?} not found", uid))
    }

    pub fn get_recv_request_mut(&mut self, uid: &Uid) -> &mut RecvRequest {
        self.recv_request_objects
            .get_mut(uid)
            .expect(&format!("RecvRequest object {:?} not found", uid))
    }

    pub fn pending_recv_requests(&self) -> Vec<(&Uid, &RecvRequest)> {
        self.recv_request_objects
            .iter()
            .filter(|(_, request)| request.recv_on_poll)
            .collect()
    }

    pub fn remove_recv_request(&mut self, uid: &Uid) {
        self.recv_request_objects.remove(uid).expect(&format!(
            "Attempt to remove an inexistent RecvRequest {:?}",
            uid
        ));
    }

    pub fn pending_connections_mut(&mut self) -> Vec<(&Uid, &mut Connection)> {
        self.connection_objects
            .iter_mut()
            .filter(|(_, conn)| match conn.status {
                ConnectionStatus::Pending | ConnectionStatus::PendingCheck => true,
                _ => false,
            })
            .collect()
    }

    pub fn get_events(&self, uid: &Uid) -> Option<(Uid, Event)> {
        if let Some(listener) = self.listener_objects.get(&uid) {
            listener
                .events
                .as_ref()
                .and_then(|event| Some((*uid, Event::Listener(event.clone()))))
        } else if let Some(connection) = self.connection_objects.get(&uid) {
            connection
                .events
                .as_ref()
                .and_then(|event| Some((*uid, Event::Connection(event.clone()))))
        } else {
            panic!("Received event for unknown object {:?}", uid)
        }
    }

    pub fn update_events(&mut self, event: &MioEvent) {
        let uid = event.token;

        if let Some(listener) = self.listener_objects.get_mut(&uid) {
            listener.update_events(uid, event)
        } else if let Some(connection) = self.connection_objects.get_mut(&uid) {
            connection.update_events(uid, event)
        } else {
            panic!("Received event for unknown object {:?}", uid)
        }
    }
}
