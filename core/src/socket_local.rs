/* Local socket interface

Processes local messages using pub/sub for testing and wasm
 */
use std::{
    collections::HashMap,
    iter::zip,
    sync::{
        mpsc::{sync_channel, Receiver, SyncSender, TryRecvError, TrySendError},
        Arc, Mutex,
    },
};

use rand::{thread_rng, Rng};

use crate::{Error, Network};

lazy_static! {
    static ref SENDERS: Arc<Mutex<HashMap<[u8; 4], Vec<SyncSender<Vec<u8>>>>>> =
        Arc::new(Mutex::new(HashMap::new()));
}

pub struct LocalInterface<const I: usize, const O: usize> {
    rx_directive: Receiver<Vec<u8>>,
    rx_jacks: Vec<Option<Receiver<Vec<u8>>>>,
    output_addrs: Vec<[u8; 4]>,
}

impl<const I: usize, const O: usize> LocalInterface<I, O> {
    pub fn new() -> Option<Self> {
        let (tx, rx) = sync_channel(50);
        let mut rng = thread_rng();
        let mut output_addrs = vec![];
        for _ in 0..O {
            output_addrs.push([
                239,
                rng.gen_range(0..255),
                rng.gen_range(0..255),
                rng.gen_range(0..255),
            ]);
        }
        let mut rx_jacks = vec![];
        for _ in 0..I {
            rx_jacks.push(None);
        }
        let mut senders = SENDERS.lock().unwrap();
        let key = [239, 0, 0, 0];
        senders.entry(key).or_insert(vec![]).push(tx);
        Some(LocalInterface {
            rx_directive: rx,
            rx_jacks,
            output_addrs,
        })
    }
}

fn send(key: [u8; 4], buf: &[u8]) {
    let mut senders = SENDERS.lock().unwrap();
    let vbuf = Vec::from(buf);
    if let Some(val) = senders.get_mut(&key) {
        val.retain(|tx| match tx.try_send(vbuf.clone()) {
            Ok(_) => true,
            Err(TrySendError::Full(_)) => true,
            Err(TrySendError::Disconnected(_)) => false,
        });
    }
}

impl<const I: usize, const O: usize> Network<I, O> for LocalInterface<I, O> {
    fn can_send(&mut self) -> bool {
        true
    }

    fn recv_directive(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        match self.rx_directive.try_recv() {
            Ok(vbuf) => {
                let n = vbuf.len();
                if n > buf.len() {
                    Err(Error::Network)
                } else {
                    for (b, v) in zip(buf, vbuf) {
                        *b = v;
                    }
                    Ok(n)
                }
            }
            Err(TryRecvError::Empty) => Err(Error::NoData),
            Err(TryRecvError::Disconnected) => Err(Error::Network),
        }
    }

    fn send_directive(&mut self, buf: &[u8]) -> Result<(), Error> {
        send([239, 0, 0, 0], buf);
        Ok(())
    }

    fn jack_connect(&mut self, jack_id: usize, addr: [u8; 4], _time: i64) -> Result<(), Error> {
        let (tx, rx) = sync_channel(2);
        match self.rx_jacks.get_mut(jack_id) {
            Some(v) => {
                *v = Some(rx);
                let mut senders = SENDERS.lock().unwrap();
                senders.entry(addr).or_insert(vec![]).push(tx);
                Ok(())
            }
            None => Err(Error::InvalidJackId),
        }
    }

    fn jack_recv(&mut self, jack_id: usize, buf: &mut [u8]) -> Result<usize, Error> {
        match self.rx_jacks.get(jack_id) {
            Some(Some(rx)) => match rx.try_recv() {
                Ok(vbuf) => {
                    let n = vbuf.len();
                    if n > buf.len() {
                        Err(Error::Network)
                    } else {
                        for (b, v) in zip(buf, vbuf) {
                            *b = v;
                        }
                        Ok(n)
                    }
                }
                Err(TryRecvError::Empty) => Err(Error::NoData),
                Err(TryRecvError::Disconnected) => Err(Error::Network),
            },
            Some(None) => Err(Error::NoData),
            None => Err(Error::InvalidJackId),
        }
    }

    fn jack_send(&mut self, jack_id: usize, buf: &[u8]) -> Result<(), Error> {
        send(self.jack_addr(jack_id)?, buf);
        Ok(())
    }

    fn jack_addr(&mut self, jack_id: usize) -> Result<[u8; 4], Error> {
        match self.output_addrs.get(jack_id) {
            Some(res) => Ok(*res),
            None => Err(Error::InvalidJackId),
        }
    }
}
