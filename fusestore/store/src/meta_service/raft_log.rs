// Copyright 2020-2021 The Datafuse Authors.
//
// SPDX-License-Identifier: Apache-2.0.

use std::ops::RangeBounds;

use async_raft::raft::Entry;
use common_tracing::tracing;

use crate::configs;
use crate::meta_service::sledkv;
use crate::meta_service::AsType;
use crate::meta_service::LogEntry;
use crate::meta_service::LogIndex;
use crate::meta_service::SledSerde;
use crate::meta_service::SledValueToKey;
use crate::meta_service::SledVarTypeTree;

const TREE_RAFT_LOG: &str = "raft_log";

/// RaftLog stores the logs of a raft node.
/// It is part of MetaStore.
pub struct RaftLog {
    pub(crate) inner: SledVarTypeTree,
}

impl SledSerde for Entry<LogEntry> {}

impl SledValueToKey<LogIndex> for Entry<LogEntry> {
    fn to_key(&self) -> LogIndex {
        self.log_id.index
    }
}

impl RaftLog {
    /// Open RaftLog
    #[tracing::instrument(level = "info", skip(db))]
    pub async fn open(
        db: &sled::Db,
        config: &configs::Config,
    ) -> common_exception::Result<RaftLog> {
        let inner = SledVarTypeTree::open(db, TREE_RAFT_LOG, config.meta_sync()).await?;
        let rl = RaftLog { inner };
        Ok(rl)
    }

    pub fn contains_key(&self, key: &LogIndex) -> common_exception::Result<bool> {
        self.logs().contains_key(key)
    }

    pub fn get(&self, key: &LogIndex) -> common_exception::Result<Option<Entry<LogEntry>>> {
        self.logs().get(key)
    }

    pub fn last(&self) -> common_exception::Result<Option<(LogIndex, Entry<LogEntry>)>> {
        self.logs().last()
    }

    /// Delete logs that are in `range`.
    ///
    /// When this function returns the logs are guaranteed to be fsync-ed.
    ///
    /// TODO(xp): in raft deleting logs may not need to be fsync-ed.
    ///
    /// 1. Deleting happens when cleaning applied logs, in which case, these logs will never be read:
    ///    The logs to clean are all included in a snapshot and state machine.
    ///    Replication will use the snapshot for sync, or create a new snapshot from the state machine for sync.
    ///    Thus these logs will never be read. If an un-fsync-ed delete is lost during server crash, it just wait for next delete to clean them up.
    ///
    /// 2. Overriding uncommitted logs of an old term by some new leader that did not see these logs:
    ///    In this case, atomic delete is quite enough(to not leave a hole).
    ///    If the system allows logs hole, non-atomic delete is quite enough(depends on the upper layer).
    ///
    pub async fn range_delete<R>(&self, range: R) -> common_exception::Result<()>
    where R: RangeBounds<LogIndex> {
        self.logs().range_delete(range, true).await
    }

    pub fn range_keys<R>(&self, range: R) -> common_exception::Result<Vec<LogIndex>>
    where R: RangeBounds<LogIndex> {
        self.logs().range_keys(range)
    }

    pub fn range_get<R>(&self, range: R) -> common_exception::Result<Vec<Entry<LogEntry>>>
    where R: RangeBounds<LogIndex> {
        self.logs().range_get(range)
    }

    /// Append logs into RaftLog.
    /// There is no consecutiveness check. It is the caller's responsibility to leave no holes(if it runs a standard raft:DDD).
    /// There is no overriding check either. It always overrides the existent ones.
    ///
    /// When this function returns the logs are guaranteed to be fsync-ed.
    pub async fn append(&self, logs: &[Entry<LogEntry>]) -> common_exception::Result<()> {
        self.logs().append_values(logs).await
    }

    /// Insert a single log.
    #[tracing::instrument(level = "debug", skip(self, log), fields(log_id=format!("{}",log.log_id).as_str()))]
    pub async fn insert(
        &self,
        log: &Entry<LogEntry>,
    ) -> common_exception::Result<Option<Entry<LogEntry>>> {
        self.logs().insert_value(log).await
    }

    fn logs(&self) -> AsType<sledkv::Logs> {
        self.inner.as_type()
    }
}
