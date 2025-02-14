use super::action::{MioEvent, PollResult, TcpAcceptResult, TcpReadResult, TcpWriteResult};
use crate::automaton::Timeout;
use crate::automaton::{Objects, Uid};
use mio::net::{TcpListener, TcpStream};
use mio::{Events, Interest, Poll, Token};
use std::cell::RefCell;
use std::io::{self, Read, Write};
use std::time::Duration;

pub struct MioState {
    poll_objects: RefCell<Objects<Poll>>,
    events_objects: RefCell<Objects<Events>>,
    tcp_listener_objects: RefCell<Objects<TcpListener>>,
    tcp_connection_objects: RefCell<Objects<TcpStream>>,
}

impl MioState {
    pub fn new() -> Self {
        Self {
            poll_objects: RefCell::new(Objects::<Poll>::new()),
            events_objects: RefCell::new(Objects::<Events>::new()),
            tcp_listener_objects: RefCell::new(Objects::<TcpListener>::new()),
            tcp_connection_objects: RefCell::new(Objects::<TcpStream>::new()),
        }
    }

    fn new_poll(&mut self, uid: Uid, obj: Poll) {
        if self.poll_objects.borrow_mut().insert(uid, obj).is_some() {
            panic!("Attempt to re-use existing {:?}", uid)
        }
    }

    fn new_events(&mut self, uid: Uid, obj: Events) {
        if self.events_objects.borrow_mut().insert(uid, obj).is_some() {
            panic!("Attempt to re-use existing {:?}", uid)
        }
    }

    fn new_tcp_listener(&mut self, uid: Uid, obj: TcpListener) {
        if self
            .tcp_listener_objects
            .borrow_mut()
            .insert(uid, obj)
            .is_some()
        {
            panic!("Attempt to re-use existing {:?}", uid)
        }
    }

    fn new_tcp_connection(&mut self, uid: Uid, obj: TcpStream) {
        if self
            .tcp_connection_objects
            .borrow_mut()
            .insert(uid, obj)
            .is_some()
        {
            panic!("Attempt to re-use existing {:?}", uid)
        }
    }

    pub fn poll_create(&mut self, uid: Uid) -> Result<(), String> {
        match Poll::new() {
            Ok(poll_obj) => {
                self.new_poll(uid, poll_obj);
                Ok(())
            }
            Err(error) => Err(error.to_string()),
        }
    }

    pub fn poll_register_tcp_server(
        &mut self,
        poll: &Uid,
        tcp_listener: Uid,
    ) -> Result<(), String> {
        let mut tcp_listener_objects = self.tcp_listener_objects.borrow_mut();

        let listener = tcp_listener_objects
            .get_mut(&tcp_listener)
            .expect(&format!("TcpListener object {:?} not found", tcp_listener));

        if let Some(poll) = self.poll_objects.borrow().get(poll) {
            match poll
                .registry()
                .register(listener, Token(tcp_listener.into()), Interest::READABLE)
            {
                Ok(_) => Ok(()),
                Err(error) => Err(error.to_string()),
            }
        } else {
            panic!("Poll object not found {:?}", poll)
        }
    }

    pub fn poll_register_tcp_connection(
        &mut self,
        poll: &Uid,
        connection: Uid,
    ) -> Result<(), String> {
        let mut tcp_connection_objects = self.tcp_connection_objects.borrow_mut();
        let stream = tcp_connection_objects
            .get_mut(&connection)
            .expect(&format!("TcpConnection object not found {:?}", connection));

        match self
            .poll_objects
            .borrow()
            .get(poll)
            .expect(&format!("Poll object not found {:?}", poll))
            .registry()
            .register(
                stream,
                Token(connection.into()),
                Interest::READABLE.add(Interest::WRITABLE),
            ) {
            Ok(_) => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }

    pub fn poll_deregister_tcp_connection(
        &mut self,
        poll: &Uid,
        connection: Uid,
    ) -> Result<(), String> {
        let mut tcp_connection_objects = self.tcp_connection_objects.borrow_mut();
        let stream = tcp_connection_objects
            .get_mut(&connection)
            .expect(&format!("TcpConnection object not found {:?}", connection));

        match self
            .poll_objects
            .borrow()
            .get(poll)
            .expect(&format!("Poll object not found {:?}", poll))
            .registry()
            .deregister(stream)
        {
            Ok(_) => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }

    pub fn poll_events(&mut self, poll: &Uid, events: &Uid, timeout: Timeout) -> PollResult {
        let mut events_object = self.events_objects.borrow_mut();
        let events = events_object
            .get_mut(events)
            .expect(&format!("Events object not found {:?}", events));

        let timeout = match timeout {
            Timeout::Millis(ms) => Some(Duration::from_millis(ms)),
            Timeout::Never => None,
        };

        match self
            .poll_objects
            .borrow_mut()
            .get_mut(poll)
            .expect(&format!("Poll object not found {:?}", poll))
            .poll(events, timeout)
        {
            Err(err) if err.kind() == io::ErrorKind::Interrupted => PollResult::Interrupted,
            Err(err) => PollResult::Error(err.to_string()),
            Ok(_) => {
                let events = events
                    .iter()
                    .map(|event| MioEvent {
                        token: event.token().0.into(),
                        readable: event.is_readable(),
                        writable: event.is_writable(),
                        error: event.is_error(),
                        read_closed: event.is_read_closed(),
                        write_closed: event.is_write_closed(),
                        priority: event.is_priority(),
                        aio: event.is_aio(),
                        lio: event.is_lio(),
                    })
                    .collect();
                //info!("|MIO| poll events: {:?}", events);
                PollResult::Events(events)
            }
        }
    }

    pub fn events_create(&mut self, uid: Uid, capacity: usize) {
        self.new_events(uid, Events::with_capacity(capacity));
    }

    pub fn tcp_listen(&mut self, uid: Uid, address: String) -> Result<(), String> {
        match address.parse() {
            Ok(address) => match TcpListener::bind(address) {
                Ok(tcp_listener) => {
                    self.new_tcp_listener(uid, tcp_listener);
                    Ok(())
                }
                Err(error) => Err(error.to_string()),
            },
            Err(error) => Err(error.to_string()),
        }
    }

    pub fn tcp_accept(&mut self, connection: Uid, listener: &Uid) -> TcpAcceptResult {
        let accept_result = {
            let tcp_listener_objects = self.tcp_listener_objects.borrow();
            let tcp_listener = tcp_listener_objects
                .get(listener)
                .expect(&format!("TcpListener object {:?} not found", listener));

            tcp_listener.accept()
        };

        match accept_result {
            Ok((stream, _address)) => {
                self.new_tcp_connection(connection, stream);
                TcpAcceptResult::Success
            }

            Err(error) => {
                if error.kind() == std::io::ErrorKind::WouldBlock {
                    TcpAcceptResult::WouldBlock
                } else {
                    TcpAcceptResult::Error(error.to_string())
                }
            }
        }
    }

    pub fn tcp_connect(&mut self, connection: Uid, address: String) -> Result<(), String> {
        match address.parse() {
            Ok(address) => match TcpStream::connect(address) {
                Ok(stream) => {
                    self.new_tcp_connection(connection, stream);
                    Ok(())
                }
                Err(error) => Err(error.to_string()),
            },
            Err(error) => Err(error.to_string()),
        }
    }

    pub fn tcp_close(&mut self, connection: &Uid) {
        let mut tcp_connection_objects = self.tcp_connection_objects.borrow_mut();

        tcp_connection_objects.remove(connection).expect(&format!(
            "TCP connection stream object not found {:?}",
            connection
        ));
        // implict stream drop
    }

    pub fn tcp_write(&mut self, connection: &Uid, data: &[u8]) -> TcpWriteResult {
        let mut tcp_connection_objects = self.tcp_connection_objects.borrow_mut();
        let stream = tcp_connection_objects.get_mut(connection).expect(&format!(
            "TCP connection stream object not found {:?}",
            connection
        ));

        match stream.write(data) {
            Ok(written) => {
                if written < data.len() {
                    TcpWriteResult::WrittenPartial(written)
                } else {
                    TcpWriteResult::WrittenAll
                }
            }
            Err(error) => match error.kind() {
                io::ErrorKind::Interrupted => TcpWriteResult::Interrupted,
                io::ErrorKind::WouldBlock => TcpWriteResult::WouldBlock,
                _ => TcpWriteResult::Error(error.to_string()),
            },
        }
    }

    pub fn tcp_read(&mut self, connection: &Uid, len: usize) -> TcpReadResult {
        assert_ne!(len, 0);

        let mut tcp_connection_objects = self.tcp_connection_objects.borrow_mut();
        let stream = tcp_connection_objects.get_mut(connection).expect(&format!(
            "TCP connection stream object not found {:?}",
            connection
        ));

        let mut recv_buf = vec![0u8; len];

        match stream.read(&mut recv_buf) {
            Ok(read) if read > 0 => {
                if read < len {
                    recv_buf.truncate(read);
                    TcpReadResult::ReadPartial(recv_buf)
                } else {
                    TcpReadResult::ReadAll(recv_buf)
                }
            }
            Ok(_) => TcpReadResult::Error("Connection closed".to_string()),
            Err(error) => match error.kind() {
                io::ErrorKind::Interrupted => TcpReadResult::Interrupted,
                io::ErrorKind::WouldBlock => TcpReadResult::WouldBlock,
                _ => TcpReadResult::Error(error.to_string()),
            },
        }
    }

    pub fn tcp_peer_address(&mut self, connection: &Uid) -> Result<String, String> {
        let tcp_connection_objects = self.tcp_connection_objects.borrow();
        let stream = tcp_connection_objects.get(connection).expect(&format!(
            "TCP connection stream object not found {:?}",
            connection
        ));

        match stream.peer_addr() {
            Ok(addr) => Ok(addr.to_string()),
            Err(err) => Err(err.to_string()),
        }
    }
}
