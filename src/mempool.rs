use crate::crypto::hash::{H256, Hashable};
use crate::transaction::SignedTransaction;
use crate::block::Content;
use crate::config::{POOL_SIZE_LIMIT, BLOCK_SIZE_LIMIT};

use std::collections::HashMap;
use std::cmp::min;
use log::debug;

pub struct MemPool {
    pub transactions: HashMap<H256, SignedTransaction>,
}

impl MemPool {
    // Create an empty mempool
    pub fn new() -> Self {
        let transactions: HashMap<H256, SignedTransaction> = HashMap::new();
        Self {
            transactions,
        }
    }

    // Randomly create and init with n trans
    pub fn new_with_trans(trans: &Vec<SignedTransaction>) -> MemPool {
        let mut transactions: HashMap<H256, SignedTransaction> = HashMap::new();
        for new_t in trans.iter()  {
            transactions.insert(new_t.hash(), new_t.clone());
        }
        MemPool {
            transactions,
        }
    }

    // Add a valid transaction after signature check
    pub fn add_with_check(&mut self, tran: &SignedTransaction) -> bool {
        let hash = tran.hash();
        if self.exist(&hash) || !tran.sign_check() || self.size() >= POOL_SIZE_LIMIT {
            return false;
        }
        self.transactions.insert(hash, tran.clone());
        true
    }

    // Remove transactions from pool and return true when succeed
    pub fn remove_trans(&mut self, trans: &Vec<H256>) {
        for hash in trans.iter() {
            if let Some(_) = self.transactions.get(&hash) {
                self.transactions.remove(&hash);
            } else {
                debug!("{:?} not exist in the mempool!", hash);
            }
        }
        if self.empty() {
            debug!("Mempool is empty!");
        }
    }

    // Create content for miner's block to include as many transactions as possible
    pub fn create_content(&self) -> Content {
        let mut trans = Vec::<SignedTransaction>::new();
        let trans_num: usize = min(BLOCK_SIZE_LIMIT, self.size());
        for (_, tran) in self.transactions.iter() {
            if trans.len() < trans_num {
                trans.push(tran.clone());
            }
        }
        Content::new_with_trans(&trans)
    }

    // check existence of a hash
    pub fn exist(&self, hash: &H256) -> bool {
        self.transactions.contains_key(hash)
    }

    // Given hashes, get transactions from mempool
    pub fn get_trans(&self, hashes: &Vec<H256>) -> Vec<SignedTransaction> {
        let mut trans = Vec::<SignedTransaction>::new();
        for h in hashes.iter() {
            if let Some(t) = self.transactions.get(h) {
                trans.push(t.clone());
            }
        }
        trans
    }

    // Number of available transactions
    pub fn size(&self) -> usize {
        self.transactions.len()
    }

    // Check if no transaction in pool
    pub fn empty(&self) -> bool {
        self.transactions.is_empty()
    }
}

#[cfg(any(test, test_utilities))]
mod tests {
    use super::*;
    use crate::helper::*;
    use crate::miner::tests::{new_server_env, connect_peers};
    use crate::block::{Block, Content};
    use crate::network::message::Message;
    use crate::config::EASIEST_DIF;
    use std::net::{SocketAddr, IpAddr, Ipv4Addr};
    use std::thread::sleep;
    use std::time;

    #[test]
    fn test_add_with_check() {
        let mut mempool = MemPool::new();
        assert!(mempool.empty());
        let t = generate_random_signed_transaction();
        let t_2 = generate_random_signed_transaction();
        assert!(mempool.add_with_check(&t));
        assert_eq!(mempool.size(), 1);
        assert!(mempool.exist(&t.hash()));
        assert!(!mempool.exist(&t_2.hash()));
        assert!(!mempool.add_with_check(&t));
        assert!(mempool.add_with_check(&t_2));
        assert_eq!(mempool.size(), 2);
        assert_eq!(mempool.get_trans(&vec![t.hash(), t_2.hash()]).len(), 2);
    }

    #[test]
    fn test_remove_trans() {
        let mut mempool = MemPool::new();
        let t = generate_random_signed_transaction();
        let t_2 = generate_random_signed_transaction();
        let t_3 = generate_random_signed_transaction();

        mempool.add_with_check(&t);
        mempool.remove_trans(&vec![t.hash(), t_2.hash()]);
        assert!(mempool.empty());

        mempool.add_with_check(&t_2);
        mempool.add_with_check(&t_3);
        assert_eq!(mempool.size(), 2);
        assert!(!mempool.exist(&t.hash()));
        mempool.remove_trans(&vec![t.hash(), t_2.hash()]);
        assert_eq!(mempool.size(), 1);
        assert!(mempool.exist(&t_3.hash()));
    }

    #[test]
    fn test_create_trans() {
        let mut mempool = MemPool::new();
        let mut t = generate_random_signed_transaction();
        mempool.add_with_check(&t);
        t = generate_random_signed_transaction();
        mempool.add_with_check(&t);
        t = generate_random_signed_transaction();
        mempool.add_with_check(&t);

        let content = mempool.create_content();
        assert_eq!(content.trans.len(), 3);
    }

    #[test]
    fn test_mempool_clear() {
        let p2p_addr_1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 17031);
        let p2p_addr_2 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 17032);
        let p2p_addr_3 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 17033);

        let (_server_1, _miner_ctx_1, mut _generator_1,  _blockchain_1, mempool_1) = new_server_env(p2p_addr_1);
        let (server_2, _miner_ctx_2, mut _generator_2, _blockchain_2, mempool_2) = new_server_env(p2p_addr_2);
        let (server_3, _miner_ctx_3, mut _generator_3, blockchain_3, _mempool_3) = new_server_env(p2p_addr_3);

        let peers_1 = vec![p2p_addr_1];
        connect_peers(&server_2, peers_1);
        let peers_2 = vec![p2p_addr_2];
        connect_peers(&server_3, peers_2);

        let t_1 = generate_random_signed_transaction();
        let t_2 = generate_random_signed_transaction();
        let t_3 = generate_random_signed_transaction();

        let mut pool_1 = mempool_1.lock().unwrap();
        pool_1.add_with_check(&t_1);
        pool_1.add_with_check(&t_2);
        pool_1.add_with_check(&t_3);
        drop(pool_1);

        let mut pool_2 = mempool_2.lock().unwrap();
        pool_2.add_with_check(&t_1);
        pool_2.add_with_check(&t_2);
        pool_2.add_with_check(&t_3);
        drop(pool_2);

        let mut chain_3 = blockchain_3.lock().unwrap();
        let difficulty: H256 = gen_difficulty_array(EASIEST_DIF).into();
        let content = Content::new_with_trans(&vec![t_1, t_2, t_3]);
        let header = generate_header(&chain_3.tip(), &content, 0, &difficulty);
        let new_block = Block::new(header, content);
        chain_3.insert(&new_block);
        drop(chain_3);

        // Server3 Only broadcasts a new block
        server_3.broadcast(Message::NewBlockHashes(vec![new_block.hash()]));
        sleep(time::Duration::from_millis(100));
        // Check server1&2 remove all the transactions within this new block
        pool_1 = mempool_1.lock().unwrap();
        pool_2 = mempool_2.lock().unwrap();
        assert!(pool_2.empty());
        assert!(pool_1.empty());
        drop(pool_1);
        drop(pool_2);
    }
}