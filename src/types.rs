// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the
// terms of the Mozilla Public License, v.
// 2.0. If a copy of the MPL was not
// distributed with this file, You can
// obtain one at
// http://mozilla.org/MPL/2.0/.


//! Various types used throughout the server crate


use std::sync::{Arc, Mutex};
use std::collections::LinkedList;
use std::sync::mpsc::{Sender, Receiver};

use socket::Socket;

/// Thread safe, Arc, locked LinkedList of Sockets
pub type SocketList = Arc<Mutex<LinkedList<Socket>>>;

/// Sender for SocketList type
pub type SocketListSender = Sender<SocketList>;

/// Receiver for SocketList type
pub type SocketListReceiver = Receiver<SocketList>;

/// Function type for passing epoll events into
pub type EventFunction = Arc<Fn(SocketList, Socket, Vec<u8>) + 'static + Send + Sync>;

/// Tuple representing the normal event params
pub type EventTuple = (SocketList, Socket, Vec<u8>);