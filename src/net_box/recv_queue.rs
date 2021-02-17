use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{self, Cursor, Read};

use refpool::{Pool, PoolRef};
use rmp::decode;

use crate::error::Error;
use crate::fiber::{Cond, Latch};

use super::options::Options;
use super::protocol::{decode_error, decode_header, Header, Response};

pub struct RecvQueue {
    buffer: RefCell<Cursor<Vec<u8>>>,
    header: RefCell<Option<Header>>,
    cond_map: RefCell<HashMap<u64, PoolRef<Cond>>>,
    cond_pool: Pool<Cond>,
    read_completed_cond: Cond,
    lock: Latch,
}

impl RecvQueue {
    pub fn new(buffer_size: usize) -> Self {
        RecvQueue {
            buffer: RefCell::new(Cursor::new(Vec::with_capacity(buffer_size))),
            header: RefCell::new(None),
            cond_map: RefCell::new(HashMap::new()),
            cond_pool: Pool::new(1024),
            read_completed_cond: Cond::new(),
            lock: Latch::new(),
        }
    }

    pub fn recv<F, R>(
        &self,
        sync: u64,
        payload_consumer: F,
        options: &Options,
    ) -> Result<Response<R>, Error>
    where
        F: FnOnce(&mut Cursor<Vec<u8>>, &Header) -> Result<R, Error>,
    {
        let cond_ref = PoolRef::new(&self.cond_pool, Cond::new());
        {
            let _lock = self.lock.lock();
            self.cond_map.borrow_mut().insert(sync, cond_ref.clone());
        }

        let is_signaled = match options.timeout {
            None => cond_ref.wait(),
            Some(timeout) => cond_ref.wait_timeout(timeout),
        };

        if is_signaled {
            let result = {
                let _lock = self.lock.lock();
                let header = self.header.replace(None).unwrap();
                if header.status_code != 0 {
                    return Err(decode_error(self.buffer.borrow_mut().by_ref())?.into());
                }

                payload_consumer(self.buffer.borrow_mut().by_ref(), &header)
                    .map(|payload| Response { payload, header })
            };
            self.read_completed_cond.signal();
            result
        } else {
            let _lock = self.lock.lock();
            self.cond_map.borrow_mut().remove(&sync);
            Err(io::Error::from(io::ErrorKind::TimedOut).into())
        }
    }

    pub fn pull(&self, stream: &mut impl Read) -> Result<(), Error> {
        let response_len = decode::read_u32(stream)?;
        let header = {
            let _lock = self.lock.lock();

            let mut buffer = self.buffer.borrow_mut();
            recv_message(stream, &mut *buffer, response_len as usize)?;
            decode_header(buffer.by_ref())?
        };

        let cond_ref = {
            let _lock = self.lock.lock();
            let sync = header.sync;
            self.header.replace(Some(header));
            self.cond_map.borrow_mut().remove(&sync)
        };

        if let Some(cond_ref) = cond_ref {
            cond_ref.signal();
            self.read_completed_cond.wait();
        }

        Ok(())
    }
}

pub fn recv_message(
    stream: &mut impl Read,
    buffer: &mut Cursor<Vec<u8>>,
    response_len: usize,
) -> Result<usize, Error> {
    buffer.set_position(0);
    {
        let buffer = buffer.get_mut();
        buffer.clear();
        buffer.reserve(response_len);
    }

    stream
        .take(response_len as u64)
        .read_to_end(buffer.get_mut())
        .map_err(|err| err.into())
}