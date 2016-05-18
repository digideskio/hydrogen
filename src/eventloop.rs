// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the
// Mozilla Public License, v. 2.0. If a copy of the MPL was not
// distributed with this file, You can obtain one at
// http://mozilla.org/MPL/2.0/.


use std::io::{Error, ErrorKind};
use std::time::Duration;
use std::os::unix::io::{RawFd, AsRawFd};

use libc;

use epoll::EpollInstance;


pub struct EventLoop {
    epoll_instance: EpollInstance,
    max_events: usize,
    max_wait_time: i32
}

impl EventLoop {

    pub fn new(max_events: usize, max_wait_time: i32) -> Result<EventLoop, Error> {
        let epoll_instance = try!(EpollInstance::new());

        return Ok(EventLoop {
            epoll_instance: epoll_instance,
            max_events: max_events,
            max_wait_time: max_wait_time
        });
    }

    pub fn register(&mut self, fd: RawFd, events: *mut libc::epoll_event) -> Result<(), Error> {
        self.epoll_instance.add_fd(fd, events)
    }

    pub fn reregister(&mut self, fd: RawFd, events: *mut libc::epoll_event) -> Result<(), Error> {
        self.epoll_instance.update_flags_for_fd(fd, events)
    }

    pub fn deregister(&mut self, fd: RawFd) -> Result<(), Error> {
        self.epoll_instance.remove_fd(fd)
    }

    pub fn run(&mut self) -> Result<Vec<libc::epoll_event>, Error> {
        let mut events_buf = Vec::<libc::epoll_event>::with_capacity(self.max_events);
        unsafe { events_buf.set_len(self.max_events); }

        let events_buf_ptr = events_buf.as_mut_ptr();
        match self.epoll_instance.wait(events_buf_ptr, self.max_events, self.max_wait_time) {
            Ok(num_events) => {
                let mut events = unsafe {
                    Vec::<libc::epoll_event>::from_raw_parts(events_buf_ptr,
                                                             num_events,
                                                             self.max_events)
                };
                
                Ok(events)
            }
            Err(e) => Err(e)
        }
    }
}
