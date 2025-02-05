use std::collections::HashSet;

use borsh::{BorshDeserialize, BorshSerialize};
use near_primitives::errors::StorageError;
use near_primitives::hash::CryptoHash;
use near_primitives::types::BlockHeight;

#[derive(BorshSerialize, BorshDeserialize, Clone, PartialEq, Eq)]
pub struct BlockInfo {
    pub hash: CryptoHash,
    pub height: BlockHeight,
    pub prev_hash: CryptoHash,
}

#[derive(strum::AsRefStr, Debug, PartialEq, Eq)]
pub enum FlatStorageError {
    /// This means we can't find a path from `flat_head` to the block. Includes `flat_head` hash and block hash,
    /// respectively.
    BlockNotSupported((CryptoHash, CryptoHash)),
    StorageInternalError,
}

impl From<FlatStorageError> for StorageError {
    fn from(err: FlatStorageError) -> Self {
        match err {
            FlatStorageError::BlockNotSupported((head_hash, block_hash)) => {
                StorageError::FlatStorageError(format!(
                    "FlatStorage with head {:?} does not support this block {:?}",
                    head_hash, block_hash
                ))
            }
            FlatStorageError::StorageInternalError => StorageError::StorageInternalError,
        }
    }
}

// Unfortunately we don't have access to ChainStore inside this file because of package
// dependencies, so we create this trait that provides the functions that FlatStorage needs
// to access chain information
pub trait ChainAccessForFlatStorage {
    fn get_block_info(&self, block_hash: &CryptoHash) -> BlockInfo;
    fn get_block_hashes_at_height(&self, block_height: BlockHeight) -> HashSet<CryptoHash>;
}

/// If a node has flat storage enabled but it didn't have flat storage data on disk, its creation should be initiated.
/// Because this is a heavy work requiring ~5h for testnet rpc node and ~10h for testnet archival node, we do it on
/// background during regular block processing.
/// This struct reveals what is the current status of creating flat storage data on disk.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FlatStorageCreationStatus {
    /// Flat storage state does not exist. We are saving `FlatStorageDelta`s to disk.
    /// During this step, we save current chain head, start saving all deltas for blocks after chain head and wait until
    /// final chain head moves after saved chain head.
    SavingDeltas,
    /// Flat storage state misses key-value pairs. We need to fetch Trie state to fill flat storage for some final chain
    /// head. It is the heaviest work, so it is done in multiple steps, see comment for `FetchingStateStatus` for more
    /// details.
    /// During each step we spawn background threads to fill some contiguous range of state keys.
    /// Status contains block hash for which we fetch the shard state and number of current step. Progress of each step
    /// is saved to disk, so if creation is interrupted during some step, we don't repeat previous steps, starting from
    /// the saved step again.
    FetchingState(FetchingStateStatus),
    /// Flat storage data exists on disk but block which is corresponds to is earlier than chain final head.
    /// We apply deltas from disk until the head reaches final head.
    /// Includes block hash of flat storage head.
    CatchingUp(CryptoHash),
    /// Flat storage is ready to use.
    Ready,
    /// Flat storage cannot be created.
    DontCreate,
}

impl Into<i64> for &FlatStorageCreationStatus {
    /// Converts status to integer to export to prometheus later.
    /// Cast inside enum does not work because it is not fieldless.
    fn into(self) -> i64 {
        match self {
            FlatStorageCreationStatus::SavingDeltas => 0,
            FlatStorageCreationStatus::FetchingState(_) => 1,
            FlatStorageCreationStatus::CatchingUp(_) => 2,
            FlatStorageCreationStatus::Ready => 3,
            FlatStorageCreationStatus::DontCreate => 4,
        }
    }
}

/// Current step of fetching state to fill flat storage.
#[derive(BorshSerialize, BorshDeserialize, Copy, Clone, Debug, PartialEq, Eq)]
pub struct FetchingStateStatus {
    /// Hash of block on top of which we create flat storage.
    pub block_hash: CryptoHash,
    /// Number of the first state part to be fetched in this step.
    pub part_id: u64,
    /// Number of parts fetched in one step.
    pub num_parts_in_step: u64,
    /// Total number of state parts.
    pub num_parts: u64,
}
