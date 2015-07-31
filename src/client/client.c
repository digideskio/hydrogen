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


#include <stdio.h>


// Rust function prototypes
int send_to_writer(void *w_tx, const char *buffer, void *k_tx);
void kill_client(void *k_tx);


// Writer Sender<T> given to us from Rust
void *write_tx;

// Sender<T> given to us from Rust to stop the client
void *stop_tx;


// Registers the address of Rust's Sender<T> used to signal the write
// thread there is a message to send
extern void register_writer_tx(void *tx)
{
    printf("%s\n", "registering writer channel");
    write_tx = tx;
}

// Registers the address of Rust's Sender<T> used to signal the lib
// to disconnect and exit
extern void register_stop_tx(void *tx)
{
    printf("%s\n", "registering kill channel");
    stop_tx = tx;
}

// Sends buffer to Rust
extern int nate_send(const char *buffer)
{
    return send_to_writer(write_tx, buffer, stop_tx);
}

// Informs Rust something went wrong and to stop the client
extern void drop()
{
    kill_client(stop_tx);
}
