// Copyright 2020 ChainSafe Systems
// SPDX-License-Identifier: Apache-2.0

#![allow(dead_code)]

use super::ticket::Ticket;
use super::TipSetKeys;
use address::Address;
use cid::Cid;
use clock::ChainEpoch;
use crypto::Signature;
use derive_builder::Builder;
use message::{SignedMessage, UnsignedMessage};
use multihash::Hash;
use std::fmt;

// DefaultHashFunction represents the default hashing function to use
// TODO SHOULD BE BLAKE2B
const DEFAULT_HASH_FUNCTION: Hash = Hash::Keccak256;
// TODO determine the purpose for these structures, currently spec includes them but with no definition
struct ChallengeTicketsCommitment {}
struct PoStCandidate {}
struct PoStRandomness {}
struct PoStProof {}

/// Header of a block
///
/// Usage:
/// ```
/// use blocks::{BlockHeader, TipSetKeys, Ticket, TxMeta};
/// use address::Address;
/// use cid::{Cid, Codec, Prefix, Version};
/// use clock::ChainEpoch;
///
/// BlockHeader::builder()
///     .parents(TipSetKeys::default())
///     .miner_address(Address::new_id(0).unwrap())
///     .bls_aggregate(vec![])
///     .weight(0) //optional
///     .epoch(ChainEpoch::default()) //optional
///     .messages(TxMeta::default()) //optional
///     .message_receipts(Cid::default()) //optional
///     .state_root(Cid::default()) //optional
///     .timestamp(0) //optional
///     .ticket(Ticket::default()) //optional
///     .build()
///     .unwrap();
/// ```
#[derive(Clone, Debug, PartialEq, Builder)]
#[builder(name = "BlockHeaderBuilder")]
pub struct BlockHeader {
    // CHAIN LINKING
    /// Parents is the set of parents this block was based on. Typically one,
    /// but can be several in the case where there were multiple winning ticket-
    /// holders for an epoch
    pub parents: TipSetKeys,

    /// weight is the aggregate chain weight of the parent set
    #[builder(default)]
    pub weight: u64,

    /// epoch is the period in which a new block is generated. There may be multiple rounds in an epoch
    #[builder(default)]
    pub epoch: ChainEpoch,

    // MINER INFO
    /// miner_address is the address of the miner actor that mined this block
    pub miner_address: Address,

    // STATE
    /// messages contains the merkle links for bls_messages and secp_messages
    #[builder(default)]
    pub messages: TxMeta,

    /// message_receipts is the Cid of the root of an array of MessageReceipts
    #[builder(default)]
    pub message_receipts: Cid,

    /// state_root is a cid pointer to the state tree after application of the transactions state transitions
    #[builder(default)]
    pub state_root: Cid,

    // CONSENSUS
    /// timestamp, in seconds since the Unix epoch, at which this block was created
    #[builder(default)]
    pub timestamp: u64,

    /// the ticket submitted with this block
    #[builder(default)]
    pub ticket: Ticket,

    // SIGNATURES
    /// aggregate signature of miner in block
    pub bls_aggregate: Signature,

    // CACHE
    /// stores the cid for the block after the first call to `cid()`
    #[builder(default)]
    pub cached_cid: Cid,
    /// stores the hashed bytes of the block after the fist call to `cid()`
    #[builder(default)]
    pub cached_bytes: Vec<u8>,
}

impl BlockHeader {
    pub fn builder() -> BlockHeaderBuilder {
        BlockHeaderBuilder::default()
    }
    /// cid returns the content id of this header
    pub fn cid(&mut self) -> Cid {
        // TODO Encode blockheader using CBOR into cache_bytes
        // Change DEFAULT_HASH_FUNCTION to utilize blake2b
        //
        // Currently content id for headers will be incomplete until encoding and supporting libraries are completed
        let new_cid = Cid::from_bytes_default(&self.cached_bytes).unwrap();
        self.cached_cid = new_cid;
        self.cached_cid.clone()
    }
}

/// A complete block
pub struct Block {
    header: BlockHeader,
    bls_messages: UnsignedMessage,
    secp_messages: SignedMessage,
}

/// Used to extract required encoded data and cid for persistent block storage
pub trait RawBlock {
    fn raw_data(&self) -> Vec<u8>;
    fn cid(&self) -> Cid;
    fn multihash(&self) -> Hash;
}

impl RawBlock for Block {
    /// returns the block raw contents as a byte array
    fn raw_data(&self) -> Vec<u8> {
        // TODO should serialize block header using CBOR encoding
        self.header.cached_bytes.clone()
    }
    /// returns the content identifier of the block
    fn cid(&self) -> Cid {
        self.header.clone().cid()
    }
    /// returns the hash contained in the block CID
    fn multihash(&self) -> Hash {
        self.header.cached_cid.prefix().mh_type
    }
}

/// human-readable string representation of a block CID
impl fmt::Display for Block {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "block: {:?}", self.header.cached_cid.clone())
    }
}

/// Tracks the merkleroots of both secp and bls messages separately
#[derive(Clone, Debug, PartialEq, Default)]
pub struct TxMeta {
    pub bls_messages: Cid,
    pub secp_messages: Cid,
}

/// ElectionPoStVerifyInfo seems to be connected to VRF
/// see https://github.com/filecoin-project/lotus/blob/master/chain/sync.go#L1099
struct ElectionPoStVerifyInfo {
    candidates: PoStCandidate,
    randomness: PoStRandomness,
    proof: PoStProof,
    messages: Vec<UnsignedMessage>,
}