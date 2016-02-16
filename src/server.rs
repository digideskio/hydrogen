// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the
// Mozilla Public License, v. 2.0. If a copy of the MPL was not
// distributed with this file, You can obtain one at
// http://mozilla.org/MPL/2.0/.


use std::io::Error;
use std::ops::DerefMut;
use std::sync::{Arc, Mutex};
use std::{mem, thread};
use std::net::{TcpStream, TcpListener};
use std::os::unix::io::{RawFd, AsRawFd, IntoRawFd};
use std::collections::LinkedList;

use libc;
use errno::errno;
use epoll;
use epoll::util::*;
use epoll::EpollEvent;
use libc::{c_int, c_void};
use config::Config;
use openssl::ssl::{SslStream, SslContext};

use stats;
use types::*;
use resources::ResourcePool;
use ss::nonblocking::plain::Plain;
use ss::nonblocking::secure::Secure;
use ss::{Socket, Stream, SRecv, SSend, TcpOptions, SocketOptions};


// We need to be able to access our resource pool from several methods
static mut pool: *mut ResourcePool = 0 as *mut ResourcePool;

// Global SslContext
static mut ssl_context: *mut SslContext = 0 as *mut SslContext;

// When added to epoll, these will be the conditions of kernel notification:
//
// EPOLLET  - Fd is in EdgeTriggered mode (notification on state changes)
// EPOLLIN  - Data is available in kerndl buffer
const EVENTS: u32 = event_type::EPOLLET | event_type::EPOLLIN;


/// Starts the epoll wait and incoming connection listener threads.
pub fn begin(config: Config, handler: Box<EventHandler>) {
    // Master socket list
    let sockets = Arc::new(Mutex::new(LinkedList::<Stream>::new()));

    // Resource pool
    let mut rp = ResourcePool::new(config.workers);
    unsafe {
        pool = &mut rp;
    }

    // Wrap our event handler into something that can be safely shared
    // between threads.
    let e_handler = Handler(Box::into_raw(handler));

    // Epoll instance
    let result = epoll::create1(0);
    if result.is_err() {
        let err = result.unwrap_err();
        error!("Unable to create epoll instance: {}", err);
        panic!()
    }
    let epfd = result.unwrap();

    // Epoll wait thread
    let epfd2 = epfd.clone();
    let streams2 = sockets.clone();
    thread::Builder::new()
        .name("Epoll Wait".to_string())
        .spawn(move || {
            event_loop(epfd2, streams2, e_handler);
        })
        .unwrap();

    // New connection thread
    let epfd3 = epfd.clone();
    let streams3 = sockets.clone();
    let prox = thread::Builder::new()
        .name("TCP Incoming Listener".to_string())
        .spawn(move || {
           listen(config, epfd3, streams3);
        })
        .unwrap();

    // Stay alive forever, or at least we hope
    let _ = prox.join();
}

/// Incoming connection listening thread
fn listen(config: Config, epfd: RawFd, streams: StreamList) {
    // Setup server and listening port
    let listener_result = try_setup_tcp_listener(&config);
    if listener_result.is_err() {
        error!("Setting up server: {}", listener_result.unwrap_err());
        return;
    }

    // If we're using SSL, setup our context reference
    if config.ssl.is_some() {
        setup_ssl_context(&config);
    }

    // Begin listening for new connections
    let listener = listener_result.unwrap();
    for accept_result in listener.incoming() {
        match accept_result {
            Ok(tcp_stream) => handle_new_connection(tcp_stream, &config, epfd, streams.clone()),
            Err(e) => error!("Accepting connection: {}", e)
        }
    }

    drop(listener);
}

fn try_setup_tcp_listener(config: &Config) -> Result<TcpListener, Error> {
    let create_result = TcpListener::bind((&config.addr[..], config.port));
    if create_result.is_err() {
        return create_result;
    }

    let listener = create_result.unwrap();
    let server_fd = listener.as_raw_fd();

    // Set the SO_REUSEADDR so that restarts after crashes do not take 5min to unbind
    // the initial port
    unsafe {
        let optval: c_int = 1;
        let opt_result = libc::setsockopt(server_fd,
                                          libc::SOL_SOCKET,
                                          libc::SO_REUSEADDR,
                                          &optval as *const _ as *const c_void,
                                          mem::size_of::<c_int>() as u32);
        if opt_result < 0 {
            return Err(Error::from_raw_os_error(errno().0 as i32));
        }
    }

    Ok(listener)
}

fn setup_ssl_context(config: &Config) {
    unsafe {
        ssl_context = Box::into_raw(Box::new(config.ssl.clone().unwrap()));
    }
}

fn handle_new_connection(tcp_stream: TcpStream, config: &Config, epfd: RawFd, streams: StreamList) {
    // Update our total opened file descriptors
    stats::fd_opened();

    // Create and configure a new socket
    let mut socket = Socket::new(tcp_stream.into_raw_fd());
    let result = setup_new_socket(&mut socket);
    if result.is_err() {
        close_fd(socket.as_raw_fd());
        return;
    }

    // Setup our stream
    let stream = match config.ssl {
        Some(_) => {
            let sock_fd = socket.as_raw_fd();
            let ssl_result = unsafe { SslStream::accept(&(*ssl_context), socket) };
            match ssl_result {
                Ok(ssl_stream) => {
                    let secure_stream = Secure::new(ssl_stream);
                    Stream::new(Box::new(secure_stream))
                }
                Err(ssl_error) => {
                    error!("Creating SslStream: {}", ssl_error);
                    close_fd(sock_fd);
                    return;
                }
            }
        }
        None => {
            let plain_text = Plain::new(socket);
            Stream::new(Box::new(plain_text))
        }
    };

    // Add stream to our server
    let fd = stream.as_raw_fd();
    add_stream_to_master_list(stream, streams.clone());
    add_to_epoll(epfd, fd, streams.clone());
}

fn setup_new_socket(socket: &mut Socket) -> Result<(), ()> {
    let result = socket.set_nonblocking();
    if result.is_err() {
        error!("Setting fd to nonblocking: {}", result.unwrap_err());
        return Err(());
    }

    let result = socket.set_tcp_nodelay(true);
    if result.is_err() {
        error!("Setting tcp_nodelay: {}", result.unwrap_err());
        return Err(());
    }

    let result = socket.set_tcp_keepalive(true);
    if result.is_err() {
        error!("Setting tcp_keepalive: {}", result.unwrap_err());
        return Err(());
    }

    Ok(())
}

/// Event loop for handling all epoll events
fn event_loop(epfd: RawFd, streams: StreamList, handler: Handler) {
    let mut events = Vec::<EpollEvent>::with_capacity(100);
    unsafe {
        events.set_len(100);
    }

    loop {
        match epoll::wait(epfd, &mut events[..], -1) {
            Ok(num_events) => {
                for x in 0..num_events as usize {
                    handle_epoll_event(epfd, &events[x], streams.clone(), handler.clone());
                }
            }
            Err(e) => {
                error!("Error on epoll::wait(): {}", e);
                panic!()
            }
        };
    }
}

/// Finds the stream the epoll event is associated with and parses the event type
/// to hand off to specific handlers
fn handle_epoll_event(epfd: RawFd, event: &EpollEvent, streams: StreamList, handler: Handler) {
    const READ_EVENT: u32 = event_type::EPOLLIN;

    // Locate the stream the event was for
    let mut stream;
    {
        // Mutex lock
        // Find the stream the event was for
        let mut guard = match streams.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                warn!("StreamList Mutex was poisoned, using anyway");
                poisoned.into_inner()
            }
        };
        let list = guard.deref_mut();

        let mut found = false;
        let mut index = 1usize;
        for s in list.iter() {
            if s.as_raw_fd() == event.data as RawFd {
                found = true;
                break;
            }
            index += 1;
        }

        if !found {
            let fd = event.data as RawFd;
            remove_fd_from_epoll(epfd, fd);
            close_fd(fd);
            return;
        }

        if index == 1 {
            stream = list.pop_front().unwrap();
        } else {
            let mut split = list.split_off(index - 1);
            stream = split.pop_front().unwrap();
            list.append(&mut split);
        }
    } // Mutex unlock

    if (event.events & READ_EVENT) > 0 {
        let _ = handle_read_event(epfd, &mut stream, handler).map(|_| {
            add_stream_to_master_list(stream, streams.clone());
        });
    } else {
        let fd = stream.as_raw_fd();
        remove_fd_from_epoll(epfd, fd);
        close_fd(fd);

        let stream_fd = stream.as_raw_fd();
        unsafe {
            (*pool).run(move || {
                let Handler(ptr) = handler;
                (*ptr).on_stream_closed(stream_fd);
            });
        }
    }
}

/// Reads all available data on the stream.
///
/// If a complete message(s) is available, each message will be routed through the
/// resource pool.
///
/// If an error occurs during the read, the stream is dropped from the server.
fn handle_read_event(epfd: RawFd, stream: &mut Stream, handler: Handler) -> Result<(), ()> {
    match stream.recv() {
        Ok(_) => {
            let mut rx_queue = stream.drain_rx_queue();
            for payload in rx_queue.iter_mut() {
                // Check if this is a request for stats
                if payload.len() == 6 && payload[0] == 0x04 && payload[1] == 0x04 {
                    let u8ptr: *const u8 = &payload[2] as *const _;
                    let f32ptr: *const f32 = u8ptr as *const _;
                    let sec = unsafe { *f32ptr };
                    let stream_cpy = stream.clone();
                    unsafe {
                        (*pool).run(move || {
                            let mut s = stream_cpy.clone();
                            let result = stats::as_serialized_buffer(sec);
                            if result.is_ok() {
                                let _ = s.send(&result.unwrap()[..]);
                            }
                        });
                    }
                    return Ok(());
                }

                // TODO - Refactor once better function passing traits are available in stable.
                let handler_cpy = handler.clone();
                let stream_cpy = stream.clone();
                let payload_cpy = payload.clone();
                unsafe {
                    (*pool).run(move || {
                        let Handler(ptr) = handler_cpy;
                        (*ptr).on_data_received(stream_cpy.clone(), payload_cpy.clone());
                    });
                }
            }
            Ok(())
        }
        Err(_) => {
            remove_fd_from_epoll(epfd, stream.as_raw_fd());
            close_fd(stream.as_raw_fd());

            let stream_fd = stream.as_raw_fd();
            unsafe {
                (*pool).run(move || {
                    let Handler(ptr) = handler;
                    (*ptr).on_stream_closed(stream_fd.clone());
                });
            }

            Err(())
        }
    }
}

/// Inserts the stream back into the master list of streams
fn add_stream_to_master_list(stream: Stream, streams: StreamList) {
    let mut guard = match streams.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            warn!("StreamList Mutex failed, using anyway...");
            poisoned.into_inner()
        }
    };
    let stream_list = guard.deref_mut();
    stream_list.push_back(stream);
    stats::conn_recv();
}

/// Adds a new fd to the epoll instance
fn add_to_epoll(epfd: RawFd, fd: RawFd, streams: StreamList) {
    let result = epoll::ctl(epfd,
                            ctl_op::ADD,
                            fd,
                            &mut EpollEvent {
                                data: fd as u64,
                                events: EVENTS,
                            });

    if result.is_err() {
        let e = result.unwrap_err();
        error!("poll::CtrlError during add: {}", e);
        remove_fd_from_list(fd, streams.clone());
        close_fd(fd);
    }
}

/// Removes a fd from the epoll instance
fn remove_fd_from_epoll(epfd: RawFd, fd: RawFd) {
    // In kernel versions before 2.6.9, the EPOLL_CTL_DEL operation required
    // a non-null pointer in event, even though this argument is ignored.
    // Since Linux 2.6.9, event can be specified as NULL when using
    // EPOLL_CTL_DEL. We'll be as backwards compatible as possible.
    let _ = epoll::ctl(epfd,
                       ctl_op::DEL,
                       fd,
                       &mut EpollEvent {
                           data: 0 as u64,
                           events: 0 as u32,
                       })
                .map_err(|e| warn!("Epoll CtrlError during del: {}", e));
}

/// Removes stream with fd from master list
fn remove_fd_from_list(fd: RawFd, streams: StreamList) {
    let mut guard = match streams.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            warn!("StreamList Mutex was poisoned, using anyway");
            poisoned.into_inner()
        }
    };
    let list = guard.deref_mut();

    let mut found = false;
    let mut index = 1usize;
    for s in list.iter() {
        if s.as_raw_fd() == fd {
            found = true;
            break;
        }
        index += 1;
    }

    if !found {
        trace!("fd: {} not found in list", fd);
        return;
    }

    if index == 1 {
        list.pop_front();
    } else {
        let mut split = list.split_off(index - 1);
        split.pop_front();
        list.append(&mut split);
    }

    stats::conn_lost();
}

/// Closes a fd with the kernel
fn close_fd(fd: RawFd) {
    unsafe {
        let result = libc::close(fd);
        if result < 0 {
            error!("Error closing fd: {}",
                   Error::from_raw_os_error(result as i32));
            return;
        }
    }
    stats::fd_closed();
}
