use redb::{ReadableTable, ReadableTableMetadata};
use tracing::instrument;

use crate::{MorpheusProcess, Transaction, snapshots_table_default};

impl<Tr: Transaction> MorpheusProcess<Tr> {
    pub fn save_snapshot(&self, db: &redb::Database) -> u64 {
        let snapshot_writer = db.begin_write().unwrap();
        let event_count = snapshot_writer
            .open_table(self.recorded_events_table.unwrap())
            .unwrap()
            .len()
            .unwrap();
        {
            let mut snapshots = snapshot_writer
                .open_table(self.snapshots_table.unwrap().clone())
                .unwrap();
            snapshots.insert(event_count, self.clone()).unwrap();
        }
        snapshot_writer.commit().unwrap();
        event_count
    }

    pub fn load_snapshot(db: &redb::Database) -> Option<MorpheusProcess<Tr>> {
        let snapshot_reader = db.begin_read().unwrap();
        let snapshots_table = snapshots_table_default::<Tr>().unwrap();
        let snapshots = match snapshot_reader.open_table(snapshots_table) {
            Ok(table) => table,
            Err(redb::TableError::TableDoesNotExist(_)) => return None,
            Err(_) => panic!("Failed to open snapshots table"),
        };
        if let Ok(Some((_, snapshot))) = snapshots.last() {
            let mut process = snapshot.value();
            process.rebuild_bloom_filter(db);
            Some(process)
        } else {
            None
        }
    }

    pub fn load_snapshot_at(
        db: &redb::Database,
        message_count: u64,
    ) -> Option<MorpheusProcess<Tr>> {
        let snapshot_reader = db.begin_read().unwrap();
        let snapshots_table = snapshots_table_default::<Tr>().unwrap();
        let snapshots = match snapshot_reader.open_table(snapshots_table) {
            Ok(table) => table,
            Err(redb::TableError::TableDoesNotExist(_)) => return None,
            Err(_) => panic!("Failed to open snapshots table"),
        };

        // Find the snapshot with the exact message count
        if let Ok(Some(snapshot)) = snapshots.get(message_count) {
            let mut process = snapshot.value();
            // Rebuild the bloom filter to only include messages up to this point
            process.rebuild_bloom_filter(db);
            Some(process)
        } else {
            None
        }
    }

    /// Rebuild the bloom filter from the recorded events up to the current recorded_events count
    fn rebuild_bloom_filter(&mut self, db: &redb::Database) {
        use fastbloom::BloomFilter;
        
        // Clear and recreate the bloom filter
        self.seen_messages = BloomFilter::with_num_bits(8 * 1024 * 16)
            .seed(&0x8F3A57D2C19E4B7F0123456789ABCDEF)
            .expected_items(100_000);

        // Rebuild from recorded events up to our current point
        let tx = db.begin_read().unwrap();
        if let Ok(events_table) = tx.open_table(self.recorded_events_table.unwrap()) {
            for i in 0..self.recorded_events {
                if let Ok(Some(event)) = events_table.get(i) {
                    if let crate::Event::ProcessMessage { ref payload, .. } = event.value() {
                        self.seen_messages.insert(payload);
                    }
                }
            }
        }
    }

    #[instrument(skip(self, db, target_message_count))]
    fn replay_messages_inner(
        &mut self,
        db: &redb::Database,
        target_message_count: u64,
    ) -> Result<(), String> {
        let start_count = self.recorded_events;
        let tx = db.begin_read().unwrap();
        let messages_table = tx.open_table(self.recorded_events_table.unwrap()).unwrap();
        let event_count = messages_table.len().unwrap();
        if start_count >= target_message_count {
            return Err(format!(
                "Current processed message count {} is already at or past target {}",
                start_count, target_message_count
            ));
        }
        if event_count < target_message_count {
            return Err(format!(
                "Event log is too short: {} < {}",
                event_count, target_message_count
            ));
        }

        // Replay messages from current count to target
        for msg_id in start_count..target_message_count {
            match messages_table.get(msg_id) {
                Ok(Some(message)) => {
                    let evt = message.value();

                    // Process the message without sending new messages (pure replay)
                    let mut dummy_to_send = Vec::new();

                    match evt {
                        crate::Event::ProcessMessage { sender, payload } => {
                            self.process_message(db, payload, sender, &mut dummy_to_send);
                        }
                        crate::Event::SetNow(ts) => {
                            self.set_now(db, ts);
                        }
                        crate::Event::SetReadyTransactions(items) => {
                            self.set_ready_transactions(db, items);
                        }
                        crate::Event::CheckTimeouts => {
                            self.check_timeouts_recorded(db, &mut dummy_to_send);
                        }
                        crate::Event::CheckProduceBlocks => {
                            self.try_produce_blocks_recorded(db, &mut dummy_to_send);
                        }
                    }
                }
                Ok(None) => {
                    return Err(format!("Missing message at index {}", msg_id));
                }
                Err(e) => {
                    return Err(format!("Error reading message {}: {}", msg_id, e));
                }
            }
        }
        Ok(())
    }

    /// Replay messages from the current state up to a target message count
    pub fn replay_messages(
        &mut self,
        db: &redb::Database,
        target_message_count: u64,
    ) -> Result<(), String> {
        self.replaying = true;

        let res = self.replay_messages_inner(db, target_message_count);
        self.replaying = false;
        res
    }
}
