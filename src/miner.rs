use crate::network::server::Handle as ServerHandle;

use log::info;

use crossbeam::channel::{unbounded, Receiver, Sender, TryRecvError};
use std::time;
use std::time::SystemTime;

use std::thread;
use std::sync::{Arc, Mutex};

use crate::blockchain::Blockchain;
use crate::block::{Header, Block};
use crate::network::message::{Message};
use crate::crypto::hash::H256;
use crate::config::MINING_STEP;
use crate::mempool::MemPool;

enum ControlSignal {
    Start(u64), // the number controls the lambda of interval between block generation
    Exit,
    Paused,
}

enum OperatingState {
    Paused,
    Run(u64),
    ShutDown,
}

pub struct Context {
    /// Channel for receiving control signal
    control_chan: Receiver<ControlSignal>,
    operating_state: OperatingState,
    server: ServerHandle,
    blockchain: Arc<Mutex<Blockchain>>,
    mempool: Arc<Mutex<MemPool>>,
    pub nonce: u32,
    pub mined_num: usize,
}

#[derive(Clone)]
pub struct Handle {
    /// Channel for sending signal to the miner thread
    control_chan: Sender<ControlSignal>,
}

pub fn new(
    server: &ServerHandle,
    blockchain: &Arc<Mutex<Blockchain>>,
    mempool: &Arc<Mutex<MemPool>>,
) -> (Context, Handle) {
    let (signal_chan_sender, signal_chan_receiver) = unbounded();

    let ctx = Context {
        control_chan: signal_chan_receiver,
        operating_state: OperatingState::Paused,
        server: server.clone(),
        blockchain: Arc::clone(blockchain),
        mempool: Arc::clone(mempool),
        nonce: 0,
        mined_num: 0,
    };

    let handle = Handle {
        control_chan: signal_chan_sender,
    };

    (ctx, handle)
}

impl Handle {
    pub fn exit(&self) {
        self.control_chan.send(ControlSignal::Exit).unwrap();
    }

    pub fn start(&self, lambda: u64) {
        self.control_chan
            .send(ControlSignal::Start(lambda))
            .unwrap();
    }

    pub fn stop(&self) {
        self.control_chan
            .send(ControlSignal::Exit)
            .unwrap()
    }

    pub fn pause(&self) {
        self.control_chan
            .send(ControlSignal::Paused)
            .unwrap()
    }
}

impl Context {
    pub fn start(mut self) {
        thread::Builder::new()
            .name("miner".to_string())
            .spawn(move || {
                self.miner_loop();
            })
            .unwrap();
        info!("Miner initialized into paused mode");
    }

    fn handle_control_signal(&mut self, signal: ControlSignal) {
        match signal {
            ControlSignal::Exit => {
                info!("Miner shutting down");
                self.operating_state = OperatingState::ShutDown;
            }
            ControlSignal::Start(i) => {
                info!("Miner starting in continuous mode with lambda {}", i);
                self.operating_state = OperatingState::Run(i);
            }
            ControlSignal::Paused => {
                info!("Miner paused");
                self.operating_state = OperatingState::Paused;
            }
        }
    }

    fn miner_loop(&mut self) {
        // main mining loop
        loop {
            // check and react to control signals
            match self.operating_state {
                OperatingState::Paused => {
                    let signal = self.control_chan.recv().unwrap();
                    self.handle_control_signal(signal);
                    continue;
                }
                OperatingState::ShutDown => {
                    return;
                }
                _ => match self.control_chan.try_recv() {
                    Ok(signal) => {
                        self.handle_control_signal(signal);
                    }
                    Err(TryRecvError::Empty) => {}
                    Err(TryRecvError::Disconnected) => panic!("Miner control channel detached"),
                },
            }
            if let OperatingState::ShutDown = self.operating_state {
                return;
            }

            self.mining();

            if let OperatingState::Run(i) = self.operating_state {
                if i != 0 {
                    let interval = time::Duration::from_micros(i as u64);
                    thread::sleep(interval);
                }
            }
        }
    }

    // Procedures when new block found
    fn found(&mut self, block: Block) {
        let block_size = get_block_size(block.clone());
        info!("Found block: {:?}, number of transactions: {:?}, size: {:?}bytes", block.header, block.content.trans.len(), block_size);

        let hash_of_trans = block.content.get_trans_hashes();
        // insert block into chain
        let mut blockchain = self.blockchain.lock().unwrap();
        blockchain.insert(&block);
        drop(blockchain);

        // remove content's all transactions from mempool
        let mut mempool = self.mempool.lock().unwrap();
        mempool.remove_trans(&hash_of_trans);

        // add new mined block into total count
        self.mined_num += 1;
        info!("Mined {} blocks so far!", self.mined_num);

        // broadcast new block
        let vec = vec![block.hash.clone()];
        self.server.broadcast(Message::NewBlockHashes(vec));
    }

    // Mining process! Return true: mining a block successfully
    fn mining(&mut self) -> bool {
        let blockchain = self.blockchain.lock().unwrap();
        let tip = blockchain.tip();  // previous hash
        let difficulty = blockchain.difficulty();
        drop(blockchain);

        let mempool = self.mempool.lock().unwrap();

        // Empty mempool, go back to sleep for a while
        if mempool.size() == 0 {
            return false;
        }

        //Get content for new block from mempool
        let content = mempool.create_content();
        drop(mempool);

        let nonce = self.nonce;
        let ts = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)
                .unwrap().as_millis();
        let mut header = Header::new(&tip, nonce, ts,
                &difficulty, &content.merkle_root());

        let mut bingo = false;
        if mining_base(&mut header, difficulty) {
            let block = Block::new(header, content);
            self.found(block);
            bingo = true;
            self.nonce = 0;
        } else {
            self.nonce = header.nonce;
        }
        bingo
    }

    #[cfg(any(test, test_utilities))]
    fn change_difficulty(&mut self, new_difficulty: &H256) {
        let mut blockchain = self.blockchain.lock().unwrap();
        blockchain.change_difficulty(new_difficulty);
    }
}

// Perforn mining for MINING_STEP here
fn mining_base(header: &mut Header, difficulty: H256) -> bool {
    for _ in 0..MINING_STEP {
        if header.hash() < difficulty {
            return true;
        }
        header.change_nonce();
    }
    return false;
}

// for demo
pub fn get_block_size(block: Block) -> usize {
    let serialized_block = bincode::serialize(&block).unwrap();
    serialized_block.len()
}

#[cfg(any(test, test_utilities))]
pub mod tests {
    use super::mining_base;
    use crate::blockchain::Blockchain;
    use crate::miner;
    use crate::crypto::hash::H256;
    use crate::network::{worker, server};
    use crate::block::Block;
    use crate::helper::*;

    use log::{error, info};
    use std::sync::{Arc, Mutex};
    use std::time;
    use std::thread;
    use std::net::{SocketAddr, IpAddr, Ipv4Addr};
    use crossbeam::channel;
    use crate::mempool::MemPool;
    use crate::config::{BLOCK_SIZE_LIMIT, EASIEST_DIF};
    use crate::transaction_generator;

    fn gen_mined_block(parent_hash: &H256, difficulty: &H256) -> Block {
        let content = generate_random_content();
        let mut header = generate_header(parent_hash, &content, 0, difficulty);
        // assume a easy difficulty
        assert!(mining_base(&mut header, difficulty.clone()));
        Block::new(header, content)
    }

    #[test]
    fn test_miner() {
        let p2p_addr_1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 17010);
        let (_server_handle, mut miner, _, _blockchain, mempool) = new_server_env(p2p_addr_1);

        //Must-be-done difficulty
        let mut difficulty: H256 = gen_difficulty_array(0).into();
        miner.change_difficulty(&difficulty);
        let mut pool = mempool.lock().unwrap();
        for _ in 0..BLOCK_SIZE_LIMIT {
            let new_t = generate_random_signed_transaction();
            pool.add_with_check(&new_t);
        }
        drop(pool);
        assert_eq!(0, miner.nonce);
        assert!(miner.mining());
        assert_eq!(0, miner.nonce);

        //Impossible difficulty
        difficulty = gen_difficulty_array(256).into();
        miner.change_difficulty(&difficulty);
        let mut pool = mempool.lock().unwrap();
        for _ in 0..BLOCK_SIZE_LIMIT {
            let new_t = generate_random_signed_transaction();
            pool.add_with_check(&new_t);
        }
        drop(pool);
        assert!(!miner.mining());
        assert_eq!(miner::MINING_STEP, miner.nonce);
    }

    #[test]
    fn test_block_relay() {
        let p2p_addr_1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 17011);
        let p2p_addr_2 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 17012);
        let p2p_addr_3 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 17013);

        let (_server_1, mut miner_ctx_1, _, blockchain_1, _mempool_1) = new_server_env(p2p_addr_1);
        let (server_2, mut miner_ctx_2, _, blockchain_2, _mempool_2) = new_server_env(p2p_addr_2);
        let (server_3, mut miner_ctx_3, _, blockchain_3, _mempool_3) = new_server_env(p2p_addr_3);
        blockchain_1.lock().unwrap().set_check_trans(false);
        blockchain_2.lock().unwrap().set_check_trans(false);
        blockchain_3.lock().unwrap().set_check_trans(false);

        // bilateral connection!!
        let peers_1 = vec![p2p_addr_1];
        connect_peers(&server_2, peers_1.clone());
        let peers_2 = vec![p2p_addr_2];
        connect_peers(&server_3, peers_2.clone());

        let chain_1 = blockchain_1.lock().unwrap();
        let difficulty = chain_1.difficulty();
        let new_block_1 = gen_mined_block(&chain_1.tip(), &difficulty);
        drop(chain_1);
        miner_ctx_1.found(new_block_1);
        thread::sleep(time::Duration::from_millis(100));

        // test block broadcast
        let chain_1 = blockchain_1.lock().unwrap();
        let chain_2 = blockchain_2.lock().unwrap();
        let chain_3 = blockchain_3.lock().unwrap();
        assert_eq!(chain_1.length(), 2);
        assert_eq!(chain_1.length(), chain_2.length());
        assert_eq!(chain_1.length(), chain_3.length());
        assert_eq!(chain_1.get_block(&chain_1.tip()), chain_2.get_block(&chain_2.tip()));
        assert_eq!(chain_1.get_block(&chain_1.tip()), chain_3.get_block(&chain_3.tip()));
        drop(chain_1);
        drop(chain_2);
        drop(chain_3);

        let chain_2 = blockchain_1.lock().unwrap();
        let new_block_2 = gen_mined_block(&chain_2.tip(), &difficulty);
        miner_ctx_2.found(new_block_2);
        drop(chain_2);
        thread::sleep(time::Duration::from_millis(100));

        let chain_1 = blockchain_1.lock().unwrap();
        let chain_2 = blockchain_2.lock().unwrap();
        let chain_3 = blockchain_3.lock().unwrap();
        assert_eq!(chain_1.length(), 3);
        assert_eq!(chain_1.length(), chain_2.length());
        assert_eq!(chain_1.length(), chain_3.length());
        assert_eq!(chain_1.get_block(&chain_1.tip()), chain_2.get_block(&chain_2.tip()));
        assert_eq!(chain_1.get_block(&chain_1.tip()), chain_3.get_block(&chain_3.tip()));
        drop(chain_1);
        drop(chain_2);
        drop(chain_3);

        let chain_3 = blockchain_1.lock().unwrap();
        let new_block_3 = gen_mined_block(&chain_3.tip(), &difficulty);
        miner_ctx_3.found(new_block_3);
        drop(chain_3);
        thread::sleep(time::Duration::from_millis(100));

        let chain_1 = blockchain_1.lock().unwrap();
        let chain_2 = blockchain_2.lock().unwrap();
        let chain_3 = blockchain_3.lock().unwrap();
        assert_eq!(chain_1.length(), 4);
        assert_eq!(chain_1.length(), chain_2.length());
        assert_eq!(chain_1.length(), chain_3.length());
        assert_eq!(chain_1.get_block(&chain_1.tip()), chain_2.get_block(&chain_2.tip()));
        assert_eq!(chain_1.get_block(&chain_1.tip()), chain_3.get_block(&chain_3.tip()));

        let total_mined_num : usize = miner_ctx_1.mined_num + miner_ctx_2.mined_num + miner_ctx_3.mined_num;
        assert_eq!(chain_1.length(), total_mined_num + 1);
        drop(chain_1);
        drop(chain_2);
        drop(chain_3);

        // test get missing parent
        let mut chain_1 = blockchain_1.lock().unwrap();
        let new_block_1 = gen_mined_block(&chain_1.tip(), &difficulty);
        chain_1.insert(&new_block_1);
        drop(chain_1);
        assert_eq!(5, blockchain_1.lock().unwrap().length());
        assert_eq!(4, blockchain_2.lock().unwrap().length());
        assert_eq!(4, blockchain_3.lock().unwrap().length());

        let new_block_2 = gen_mined_block(&new_block_1.hash, &difficulty);
        miner_ctx_1.found(new_block_2);
        thread::sleep(time::Duration::from_millis(100));
        assert_eq!(6, blockchain_1.lock().unwrap().length());
        assert_eq!(6, blockchain_2.lock().unwrap().length());
        assert_eq!(6, blockchain_3.lock().unwrap().length());

        // test insert_with_check
        let mut chain_1 = blockchain_1.lock().unwrap();
        let wrong_difficulty: H256 = gen_difficulty_array(1).into();
        let wrong_block = gen_mined_block(&chain_1.tip(), &wrong_difficulty);
        assert!(!chain_1.insert_with_check(&wrong_block));
        assert!(!chain_1.insert_with_check(&new_block_1));
        let correct_difficulty: H256 = gen_difficulty_array(EASIEST_DIF).into();
        let correct_block = gen_mined_block(&chain_1.tip(), &correct_difficulty);
        assert!(chain_1.insert_with_check(&correct_block));
    }

    pub fn new_server_env(ipv4_addr: SocketAddr) -> (server::Handle, miner::Context, transaction_generator::Context, Arc<Mutex<Blockchain>>, Arc<Mutex<MemPool>>) {
        let (sender, receiver) = channel::unbounded();
        let (server_ctx, server) = server::new(ipv4_addr, sender).unwrap();
        server_ctx.start().unwrap();

        let mut blockchain = Blockchain::new();
        let difficulty: H256 = gen_difficulty_array(EASIEST_DIF).into();
        blockchain.change_difficulty(&difficulty);
        let blockchain =  Arc::new(Mutex::new(blockchain));

        let mempool = MemPool::new();
        let mempool = Arc::new(Mutex::new(mempool));

        let worker_ctx = worker::new(4, receiver, &server, &blockchain, &mempool);
        worker_ctx.start();

        let (miner_ctx, _miner) = miner::new(&server, &blockchain, &mempool);

        let transaction_generator_ctx = transaction_generator::new(&server, &mempool);

        (server, miner_ctx, transaction_generator_ctx, blockchain, mempool)
    }

    pub fn connect_peers(server: &server::Handle, known_peers: Vec<SocketAddr>) {
        for peer_addr in known_peers {
            match server.connect(peer_addr) {
                Ok(_) => {
                    info!("Connected to outgoing peer {}", &peer_addr);
                }
                Err(e) => {
                    error!(
                        "Error connecting to peer {}, retrying in one second: {}",
                        peer_addr, e
                    );
                }
            }
        }
    }
}