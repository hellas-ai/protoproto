use redb::ReadableTable;

use crate::{MorpheusProcess, Transaction, snapshots_table_default};

impl<Tr: Transaction> MorpheusProcess<Tr> {
    pub fn save_snapshot(&self, db: &redb::Database) {
        let snapshot_writer = db.begin_write().unwrap();
        {
            let mut snapshots = snapshot_writer
                .open_table(self.snapshots_table.unwrap().clone())
                .unwrap();
            snapshots
                .insert(self.received_messages, self.clone())
                .unwrap();
        }
        snapshot_writer.commit().unwrap();
    }

    pub fn load_snapshot(db: &redb::Database) -> Option<MorpheusProcess<Tr>> {
        let snapshot_reader = db.begin_read().unwrap();
        let snapshots_table = snapshots_table_default::<Tr>().unwrap();
        let snapshots = snapshot_reader.open_table(snapshots_table).unwrap();
        if let Ok(Some((_, snapshot))) = snapshots.last() {
            Some(snapshot.value())
        } else {
            None
        }
    }
}
