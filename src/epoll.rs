// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the
// Mozilla Public License, v. 2.0. If a copy of the MPL was not
// distributed with this file, You can obtain one at
// http://mozilla.org/MPL/2.0/.


use std::ptr;
use std::io::{Error, ErrorKind};
use std::os::unix::io::{RawFd, AsRawFd};

use libc;
use errno::errno;


/// Light wrapper around an `epoll_create(1)'d fd.`
pub struct EpollInstance {
    fd: RawFd,
    num_fds: usize
}

impl EpollInstance {
    /// Attempts to create a new epoll instance.
    pub fn new() -> Result<EpollInstance, Error> {
        // Since Linux 2.6.8, the size argument is ignored, but must be greater than zero.
        let size: libc::c_int = 1;
        let result = unsafe { libc::epoll_create(size) };

        if result < 0 {
            return Err(Error::from_raw_os_error(errno().0 as i32));
        }

        return Ok(EpollInstance {
            fd: result,
            num_fds: 0
        });
    }

    /// Attempts to add the passed fd to this instance.
    pub fn add_fd(&mut self, fd: RawFd, events: *mut libc::epoll_event) -> Result<(), Error> {
        ctl(self.fd, libc::EPOLL_CTL_ADD, fd, events).map(|_| self.num_fds += 1)
    }

    /// Attempts to remove the passed fd from this instance.
    pub fn remove_fd(&mut self, fd: RawFd) -> Result<(), Error> {
        // In kernel versions before 2.6.9, the EPOLL_CTL_DEL operation required a non-null
        // pointer in event, even though this argument is ignored. Since Linux 2.6.9, event
        // can be specified as NULL when using EPOLL_CTL_DEL.
        ctl(self.fd, libc::EPOLL_CTL_DEL, fd, ptr::null_mut())
    }

    /// Attempts to adjust the event mask on the passed fd for this instance.
    pub fn update_flags_for_fd(&mut self,
                               fd: RawFd,
                               events: *mut libc::epoll_event)
                               -> Result<(), Error>
    {
        ctl(self.fd, libc::EPOLL_CTL_MOD, fd, events)
    }

    pub fn wait(&mut self,
                events_buf: *mut libc::epoll_event,
                max_events: usize,
                timeout: i32)
                -> Result<usize, Error>
    {
        let result = unsafe {
            libc::epoll_wait(self.fd, events_buf, max_events as libc::c_int, timeout)
        };

        if result < 0 {
            return Err(Error::from_raw_os_error(errno().0 as i32));
        }

        return Ok(result as usize);
    }
}

impl AsRawFd for EpollInstance {
    fn as_raw_fd(&self) -> RawFd {
        self.fd
    }
}

#[inline]
fn ctl(epfd: libc::c_int,
       op: libc::c_int,
       fd: libc::c_int,
       event: *mut libc::epoll_event)
       -> Result<(), Error>
{
    let result = unsafe { libc::epoll_ctl(epfd, op, fd, event) };

    if result < 0 {
        return Err(Error::from_raw_os_error(errno().0 as i32));
    }

    return Ok(());
}

