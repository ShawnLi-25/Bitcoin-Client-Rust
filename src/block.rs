use hex;
use ring::digest;
use serde::{Serialize, Deserialize};
use chrono::prelude::DateTime;
use chrono::Utc;
use std::time::{UNIX_EPOCH, Duration};
use crate::crypto::hash::{H256, Hashable};
use crate::transaction::{SignedTransaction, PrintableTransaction};
use crate::crypto::merkle::MerkleTree;
use crate::config::DIFFICULTY;
use crate::helper::gen_difficulty_array;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Block {
    pub hash: H256,         // the hash of the header in this block
    pub index: usize,       // the distance from the genesis block
    pub header: Header,
    pub content: Content,   // transaction in this block
}

#[derive(Serialize, Deserialize)]
pub struct PrintableBlock {
    pub hash: String,
    pub parent_hash: String,
    pub index: usize,
    pub nonce: u32,
    pub difficulty: String,
    pub timestamp: String,
    pub merkle_root: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Header {
    pub parent: H256,
    pub nonce: u32,
    pub difficulty: H256,
    pub timestamp: u64,
    merkle_root: H256,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Content {
    pub trans: Vec<SignedTransaction>
}

#[derive(Serialize, Deserialize)]
pub struct PrintableContent {
    pub trans: Vec<PrintableTransaction>
}

impl Hashable for Block {
    fn hash(&self) -> H256 {
        self.hash.clone()
    }
}

impl PartialEq<Block> for Block {
    fn eq(&self, other: &Block) -> bool {
        let self_serialized_array = bincode::serialize(&self).unwrap();
        let other_serialized_array = bincode::serialize(other).unwrap();
        self_serialized_array == other_serialized_array
    }
}

impl Block {
    pub fn genesis() -> Self {
        let h: [u8; 32] = [0; 32];
        let difficulty: H256 = gen_difficulty_array(DIFFICULTY).into();

        let header = Header {
            parent: h.into(),
            nonce: 0,
            difficulty: difficulty,
            timestamp: 0,
            merkle_root: h.into(),
        };

        let content = Content {
            trans: Vec::<SignedTransaction>::new(),
        };

        Block {
            hash: h.into(),
            index: 0,
            header: header,
            content: content,
        }
    }

    pub fn new(header: Header, content: Content) -> Self {
        Self {
            hash: header.hash(),
            index: 0,
            header: header,
            content: content,
        }
    }

    pub fn get_hash(&self) -> H256 {
        self.hash.clone()
    }

    // Check transaction signature in content; if anyone fails, the whole block fails
    pub fn validate_trans(&self) -> bool {
        let trans = &self.content.trans;
        for t in trans.iter() {
            if !t.sign_check() {
                return false;
            }
        }
        true
    }

    #[cfg(any(test, test_utilities))]
    pub fn change_hash(&mut self, hash: &H256) {
        self.hash = hash.clone();
    }
}

impl PrintableBlock {
    pub fn from_block_vec(blocks: &Vec<Block>) -> Vec<PrintableBlock> {
        let mut pblocks = Vec::<PrintableBlock>::new();
        for b in blocks {
            let t = UNIX_EPOCH + Duration::from_millis(b.header.timestamp);
            let datetime = DateTime::<Utc>::from(t);
            let ts_str = datetime.format("%Y-%m-%d %H:%M:%S%.3f").to_string();
            let p = PrintableBlock {
                hash: hex::encode(&b.hash),
                parent_hash: hex::encode(&b.header.parent),
                index: b.index,
                nonce: b.header.nonce,
                difficulty: hex::encode(&b.header.difficulty),
                timestamp: ts_str,
                merkle_root: hex::encode(&b.header.merkle_root),
            };
            pblocks.push(p);
        }
        pblocks
    }
}

impl Header {
    pub fn new( parent: &H256, nonce: u32, timestamp: u128,
                difficulty: &H256, merkle_root: &H256) -> Self {
        Self {
            parent: parent.clone(),
            nonce: nonce,
            difficulty: difficulty.clone(),
            timestamp: timestamp as u64,
            merkle_root: merkle_root.clone(),
        }
    }

    pub fn hash(&self) -> H256 {
        let mut ctx = digest::Context::new(&digest::SHA256);
        ctx.update(self.parent.as_ref());
        ctx.update(&self.nonce.to_be_bytes());
        ctx.update(self.difficulty.as_ref());
        ctx.update(&self.timestamp.to_be_bytes());
        ctx.update(self.merkle_root.as_ref());
        ctx.finish().into()
    }

    pub fn change_nonce(&mut self) {
        self.nonce = self.nonce.overflowing_add(1).0;
    }
}

impl Content {
    pub fn new() -> Self {
        Self {
            trans: Vec::<SignedTransaction>::new(),
        }
    }

    pub fn new_with_trans(trans: &Vec<SignedTransaction>) -> Self {
        Self {
            trans: trans.clone(),
        }
    }

    pub fn add_tran(&mut self, tran: SignedTransaction) {
        self.trans.push(tran);
    }

    pub fn merkle_root(&self) -> H256 {
        let tree = MerkleTree::new(&self.trans);
        tree.root()
    }

    // Return a vector of hash for all transactions inside
    pub fn get_trans_hashes(&self) -> Vec<H256> {
        let hashes: Vec<H256> = self.trans.iter()
            .map(|t|t.hash).collect();
        hashes
    }
}

impl PrintableContent {
    pub fn from_content_vec(contents: &Vec<Content>) -> Vec<Self> {
        let mut pcontents = Vec::<Self>::new();
        for c in contents {
            let pts = PrintableTransaction::from_signedtx_vec(&c.trans);
            let pc = Self { trans: pts };
            pcontents.push(pc);
        }
        pcontents
    }
}

#[cfg(any(test, test_utilities))]
pub mod test {
    use super::*;
    use crate::crypto::hash::H256;
    use crate::helper::*;

    #[test]
    fn test_genesis() {
        let g = Block::genesis();
        assert_eq!(0, g.index);
        assert_eq!(g.hash, H256::from([0u8; 32]));
        // let array: [u8; 32] = g.header.difficulty.into();
        assert!(DIFFICULTY > 0);
        assert!(DIFFICULTY < 256);
    }

    #[test]
    fn test_content_new_with_trans() {
        let mut trans = Vec::<SignedTransaction>::new();
        for _ in 0..3 {
            trans.push(generate_random_signed_transaction());
        }
        let _content = Content::new_with_trans(&trans);
    }

    #[test]
    fn test_difficulty() {
        let test_array1 = gen_difficulty_array(8);
        assert_eq!(0, test_array1[0]);
        assert_eq!(255, test_array1[1]);
        assert_eq!(255, test_array1[31]);

        let test_array1 = gen_difficulty_array(9);
        assert_eq!(0, test_array1[0]);
        assert_eq!(0x7f, test_array1[1]);
        assert_eq!(255, test_array1[31]);

        let test_array2 = gen_difficulty_array(10);
        assert_eq!(0, test_array2[0]);
        assert_eq!(63, test_array2[1]);
        assert_eq!(255, test_array2[2]);

        let test_array3 = gen_difficulty_array(15);
        assert_eq!(0, test_array3[0]);
        assert_eq!(1, test_array3[1]);
        assert_eq!(0, test_array3[0]);
        assert_eq!(255, test_array1[31]);

        let test_array4 = gen_difficulty_array(21);
        assert_eq!(0, test_array4[0]);
        assert_eq!(0, test_array4[1]);
        assert_eq!(7, test_array4[2]);
    }

    #[test]
    fn test_block_equality() {
        let rand_1: [u8; 32] = [0; 32];
        let rand_2: [u8; 32] = [1; 32];

        let content_1 = generate_random_content();
        let content_2 = generate_random_content();
        let header_1 = generate_random_header(&rand_1.into(), &content_1);
        let header_2 = generate_random_header(&rand_1.into(), &content_2);
        let header_3 = generate_random_header(&rand_2.into(), &content_1);

        let block_1 = Block::new(header_1.clone(), content_1.clone());
        let block_2 = Block::new(header_2.clone(), content_1.clone());
        let block_3 = Block::new(header_3.clone(), content_1.clone());
        let block_4 = Block::new(header_1.clone(), content_2.clone());
        let block_5 = Block::new(header_1.clone(), content_1.clone());

        // different header
        assert_ne!(block_1, block_2);
        assert_ne!(block_1, block_3);
        // different content
        assert_ne!(block_1, block_4);
        // same
        assert_eq!(block_1, block_5);
    }

    #[test]
    fn test_get_trans_hashed() {
        let t_1 = generate_random_signed_transaction();
        let t_2 = generate_random_signed_transaction();
        let t_3 = generate_random_signed_transaction();
        let content = Content::new_with_trans(&vec![t_1.clone(), t_2.clone(), t_3.clone()]);
        let res = content.get_trans_hashes();
        assert_eq!(t_1.hash, res[0]);
        assert_eq!(t_2.hash, res[1]);
        assert_eq!(t_3.hash, res[2]);
    }
}
