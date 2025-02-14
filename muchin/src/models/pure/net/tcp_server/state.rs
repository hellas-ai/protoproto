use crate::automaton::{Objects, Redispatch, Uid};
use std::{collections::BTreeSet, mem};

#[derive(Debug)]
pub struct Listener {
    pub max_connections: usize,
    pub on_success: Redispatch<Uid>,
    pub on_error: Redispatch<(Uid, String)>,
    pub on_new_connection: Redispatch<(Uid, Uid)>,
    pub on_connection_closed: Redispatch<(Uid, Uid)>,
    pub on_listener_closed: Redispatch<Uid>,
    pub connections: BTreeSet<Uid>,
}

impl Listener {
    pub fn new(
        max_connections: usize,
        on_success: Redispatch<Uid>,
        on_error: Redispatch<(Uid, String)>,
        on_new_connection: Redispatch<(Uid, Uid)>,
        on_connection_closed: Redispatch<(Uid, Uid)>,
        on_listener_closed: Redispatch<Uid>,
    ) -> Self {
        Self {
            max_connections,
            on_new_connection,
            on_success,
            on_error,
            on_connection_closed,
            on_listener_closed,
            connections: BTreeSet::new(),
        }
    }

    pub fn remove_connection(&mut self, uid: &Uid) {
        self.connections.remove(uid);
    }
}

#[derive(Debug)]
pub struct SendRequest {
    pub connection: Uid,
    pub on_success: Redispatch<Uid>,
    pub on_timeout: Redispatch<Uid>,
    pub on_error: Redispatch<(Uid, String)>,
}

#[derive(Debug)]
pub struct RecvRequest {
    pub connection: Uid,
    pub on_success: Redispatch<(Uid, Vec<u8>)>,
    pub on_timeout: Redispatch<(Uid, Vec<u8>)>,
    pub on_error: Redispatch<(Uid, String)>,
}

#[derive(Debug)]
pub struct PollRequest {
    pub on_success: Redispatch<Uid>,
    pub on_error: Redispatch<(Uid, String)>,
}

#[derive(Debug)]
pub struct TcpServerState {
    pub listeners: Objects<Listener>,
    pub send_requests: Objects<SendRequest>,
    pub recv_requests: Objects<RecvRequest>,
    pub poll_request: Option<PollRequest>,
}

impl TcpServerState {
    pub fn new() -> Self {
        Self {
            listeners: Objects::<Listener>::new(),
            send_requests: Objects::<SendRequest>::new(),
            recv_requests: Objects::<RecvRequest>::new(),
            poll_request: None,
        }
    }

    pub fn set_poll_request(&mut self, request: PollRequest) {
        assert!(self.poll_request.is_none());
        self.poll_request = Some(request);
    }

    pub fn take_poll_request(&mut self) -> PollRequest {
        mem::take(&mut self.poll_request).expect("Take attempt on inexistent PollRequest")
    }

    pub fn new_send_request(
        &mut self,
        uid: &Uid,
        connection: Uid,
        on_success: Redispatch<Uid>,
        on_timeout: Redispatch<Uid>,
        on_error: Redispatch<(Uid, String)>,
    ) {
        if self
            .send_requests
            .insert(
                *uid,
                SendRequest {
                    connection,
                    on_success,
                    on_timeout,
                    on_error,
                },
            )
            .is_some()
        {
            panic!("Attempt to re-use existing {:?}", uid)
        }
    }

    pub fn take_send_request(&mut self, uid: &Uid) -> SendRequest {
        self.send_requests
            .remove(uid)
            .expect(&format!("Take attempt on inexistent SendRequest {:?}", uid))
    }

    pub fn new_recv_request(
        &mut self,
        uid: &Uid,
        connection: Uid,
        on_success: Redispatch<(Uid, Vec<u8>)>,
        on_timeout: Redispatch<(Uid, Vec<u8>)>,
        on_error: Redispatch<(Uid, String)>,
    ) {
        if self
            .recv_requests
            .insert(
                *uid,
                RecvRequest {
                    connection,
                    on_success,
                    on_timeout,
                    on_error,
                },
            )
            .is_some()
        {
            panic!("Attempt to re-use existing {:?}", uid)
        }
    }

    pub fn take_recv_request(&mut self, uid: &Uid) -> RecvRequest {
        self.recv_requests
            .remove(uid)
            .expect(&format!("Take attempt on inexistent SendRequest {:?}", uid))
    }

    pub fn new_connection(&mut self, connection: Uid, listener: Uid) {
        self.get_listener_mut(&listener)
            .connections
            .insert(connection);
    }

    pub fn get_connection_listener_mut(&mut self, connection: &Uid) -> (&Uid, &mut Listener) {
        self.listeners
            .iter_mut()
            .find(|(_, listener)| listener.connections.contains(connection))
            .expect(&format!(
                "Listener not found for connection {:?}",
                connection
            ))
    }

    pub fn new_listener(
        &mut self,
        listener: Uid,
        max_connections: usize,
        on_success: Redispatch<Uid>,
        on_error: Redispatch<(Uid, String)>,
        on_new_connection: Redispatch<(Uid, Uid)>,
        on_connection_closed: Redispatch<(Uid, Uid)>,
        on_listener_closed: Redispatch<Uid>,
    ) {
        if self
            .listeners
            .insert(
                listener,
                Listener::new(
                    max_connections,
                    on_success,
                    on_error,
                    on_new_connection,
                    on_connection_closed,
                    on_listener_closed,
                ),
            )
            .is_some()
        {
            panic!("Attempt to re-use existing {:?}", listener)
        }
    }

    pub fn get_listener(&self, listener: &Uid) -> &Listener {
        self.listeners
            .get(listener)
            .expect(&format!("Listener object {:?} not found", listener))
    }

    pub fn get_listener_mut(&mut self, listener: &Uid) -> &mut Listener {
        self.listeners
            .get_mut(listener)
            .expect(&format!("Listener object {:?} not found", listener))
    }

    pub fn remove_listener(&mut self, listener: &Uid) -> Listener {
        self.listeners.remove(listener).expect(&format!(
            "Attempt to remove an inexistent Listener {:?}",
            listener
        ))
    }
}
