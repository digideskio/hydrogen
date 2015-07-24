// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the
// terms of the Mozilla Public License, v.
// 2.0. If a copy of the MPL was not
// distributed with this file, You can
// obtain one at
// http://mozilla.org/MPL/2.0/.
//
// This Source Code Form is "Incompatible
// With Secondary Licenses", as defined by
// the Mozilla Public License, v. 2.0.


use std::thread;
use std::thread::JoinHandle;
use std::sync::Arc;
use std::sync::mpsc::{channel, Sender, Receiver};

use super::types::*;
use super::FpWrapper;
use super::socket::Socket;

#[allow(dead_code)]
pub struct WorkerThread {
    /// Handle to this process
    prox: JoinHandle<()>,
    /// Sender to this threads receiver
    prox_tx: Sender<(Arc<FpWrapper>, SocketList, Socket, Vec<u8>)>
}


impl WorkerThread {

    /// Creates a new worker thread
    pub fn new() -> WorkerThread {
        let (tx, rx): (
            Sender<(Arc<FpWrapper>, SocketList, Socket, Vec<u8>)>,
            Receiver<(Arc<FpWrapper>, SocketList, Socket, Vec<u8>)>)
            = channel();

        let prox = thread::Builder::new()
            .name("WorkerThread".to_string())
            .spawn(move || {
                WorkerThread::start(rx);
            }).unwrap();

        WorkerThread {
            prox: prox,
            prox_tx: tx
        }
    }

    /// Returns a clone of this thread's Sender<T>
    pub fn sender(&self) -> Sender<(Arc<FpWrapper>, SocketList, Socket, Vec<u8>)> {
        self.prox_tx.clone()
    }

    /// Starts the worker thread
    fn start(rx: Receiver<(Arc<FpWrapper>, SocketList, Socket, Vec<u8>)>) {
        for (task, sockets, socket, buffer) in rx.iter() {
            task.run(sockets, socket, buffer);
        }
    }
}